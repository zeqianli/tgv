//! Serves a session over a local Unix socket, and connects to sessions that viewers serve.
//!
//! A viewer listens on `<pid>.sock` in a directory that only the current user can open, so a
//! `tgv mcp` process started by an agent can find the viewer and drive it. Each line on a
//! connection holds one JSON [`Call`], and the session answers each with one JSON reply line.

use crate::{error::SessionError, schema::*, session::SessionHandle};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs, io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{
        UnixListener, UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
    task::JoinHandle,
};

/// A session request in serializable form, for socket connections and in-process clients.
#[derive(Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Call {
    Describe,
    LoadDataset(DatasetRequest),
    Inspect(InspectRequest),
    Query(QueryRequest),
    Navigate(NavigateRequest),
    Highlight(HighlightRequest),
    ClearHighlights,
    ViewState,
}

/// The answer to one [`Call`] on a socket connection.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Reply {
    Ok(Value),
    Error {
        code: String,
        message: String,
        field: Option<String>,
    },
}

impl From<Result<Value, SessionError>> for Reply {
    fn from(result: Result<Value, SessionError>) -> Self {
        match result {
            Ok(value) => Self::Ok(value),
            Err(error) => Self::Error {
                code: error.code().to_owned(),
                message: error.to_string(),
                field: error.field().map(str::to_owned),
            },
        }
    }
}

impl SessionHandle {
    /// Runs a call and returns its reply as JSON. `Describe` returns `null` before the first
    /// load, and calls without a result return `null`.
    pub async fn call(&self, call: Call) -> Result<Value, SessionError> {
        fn json(value: impl Serialize) -> Result<Value, SessionError> {
            serde_json::to_value(value).map_err(|error| SessionError::Core(error.into()))
        }
        match call {
            Call::Describe => json(self.describe().await?),
            Call::LoadDataset(request) => json(self.load_dataset(request).await?),
            Call::Inspect(request) => json(self.inspect(request).await?),
            Call::Query(request) => json(self.query(request).await?),
            Call::Navigate(request) => json(self.navigate(request).await?),
            Call::Highlight(request) => json(self.highlight(request).await?),
            Call::ClearHighlights => json(self.clear_highlights().await?),
            Call::ViewState => json(self.view_state().await?),
        }
    }
}

/// Returns the directory where viewers listen, which only the current user can open.
fn socket_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(runtime) => PathBuf::from(runtime).join("tgv"),
        None => {
            std::env::temp_dir().join(format!("tgv-{}", std::env::var("USER").unwrap_or_default()))
        }
    }
}

/// Serves a session to socket connections, until dropped. Dropping it also removes the socket
/// file.
pub struct SessionSocket {
    pub path: PathBuf,
    task: JoinHandle<()>,
}

impl SessionSocket {
    /// Listens on this process's socket in the viewer directory.
    pub fn bind(session: SessionHandle) -> io::Result<Self> {
        let dir = socket_dir();
        fs::create_dir_all(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        let path = dir.join(format!("{}.sock", std::process::id()));
        // A file left by an earlier process with the same ID would block binding.
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        log::info!("Serving the session at {}", path.display());
        let task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        tokio::spawn(serve_connection(stream, session.clone()));
                    }
                    Err(error) => {
                        log::warn!("The session socket stops accepting connections: {error}");
                        break;
                    }
                }
            }
        });
        Ok(Self { path, task })
    }
}

impl Drop for SessionSocket {
    fn drop(&mut self) {
        self.task.abort();
        let _ = fs::remove_file(&self.path);
    }
}

/// Answers the calls on one connection, in order, until the client disconnects.
async fn serve_connection(stream: UnixStream, session: SessionHandle) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let reply = match serde_json::from_str::<Call>(&line) {
            Ok(call) => Reply::from(session.call(call).await),
            Err(error) => Reply::Error {
                code: "invalid_input".to_owned(),
                message: format!("The request is not a valid call: {error}"),
                field: None,
            },
        };
        let mut line = serde_json::to_string(&reply).expect("replies serialize to JSON");
        line.push('\n');
        if write.write_all(line.as_bytes()).await.is_err() {
            break;
        }
    }
}

/// A connection to a session that another process serves.
pub struct SessionConnection {
    pub path: PathBuf,
    lines: Lines<BufReader<OwnedReadHalf>>,
    write: OwnedWriteHalf,
}

impl SessionConnection {
    pub async fn connect(path: &Path) -> io::Result<Self> {
        let (read, write) = UnixStream::connect(path).await?.into_split();
        Ok(Self {
            path: path.to_owned(),
            lines: BufReader::new(read).lines(),
            write,
        })
    }

    /// Connects to the most recently started viewer that accepts connections, and removes
    /// socket files that no process listens on.
    pub async fn find_viewer() -> Option<Self> {
        let mut sockets: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(socket_dir())
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "sock")
            })
            .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
            .collect();
        sockets.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, path) in sockets {
            match Self::connect(&path).await {
                Ok(connection) => return Some(connection),
                // A viewer that exits without cleaning up leaves its socket behind.
                Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                    let _ = fs::remove_file(&path);
                }
                Err(error) => log::warn!("Cannot connect to {}: {error}", path.display()),
            }
        }
        None
    }

    /// Sends a call and waits for its reply.
    pub async fn call(&mut self, call: &Call) -> Result<Value, SessionError> {
        let connection = |error: io::Error| SessionError::Connection(error.to_string());
        let mut line =
            serde_json::to_string(call).map_err(|error| SessionError::Core(error.into()))?;
        line.push('\n');
        self.write
            .write_all(line.as_bytes())
            .await
            .map_err(connection)?;
        let reply = self
            .lines
            .next_line()
            .await
            .map_err(connection)?
            .ok_or_else(|| {
                SessionError::Connection("the viewer closed the connection".to_owned())
            })?;
        match serde_json::from_str(&reply).map_err(|error| SessionError::Core(error.into()))? {
            Reply::Ok(value) => Ok(value),
            Reply::Error {
                code,
                message,
                field,
            } => Err(SessionError::Remote {
                code,
                message,
                field,
            }),
        }
    }
}

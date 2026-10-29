//! MCP tools over a TGV session, served over standard input and output.
//!
//! Each tool translates an MCP call into a session [`Call`] and its reply into a tool result.
//! Calls go to a running TGV viewer when one is found, so the agent sees and moves what the
//! user sees. Otherwise they go to a headless session in this process. When several viewers
//! are open, such as a terminal and a window, the agent chooses one with `attach`.

use gv_core::{error::TGVError, settings::Settings};
use gv_session::*;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    service::QuitReason,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

/// A failure to run the MCP service.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("The MCP service fails to initialize: {0}")]
    Initialize(String),

    #[error("The MCP service fails: {0}")]
    Service(String),

    #[error("The MCP service stops unexpectedly: {0}")]
    UnexpectedStop(String),

    #[error("The session worker fails: {0}")]
    Worker(String),

    #[error(transparent)]
    Core(#[from] TGVError),
}

impl From<McpError> for TGVError {
    fn from(error: McpError) -> Self {
        match error {
            McpError::Core(error) => error,
            other => TGVError::StateError(other.to_string()),
        }
    }
}

/// Where tool calls go.
struct Target {
    /// The viewer's session and process ID, once attached.
    viewer: Option<(SessionConnection, u32)>,
    /// Whether the agent has loaded a dataset into the headless session. From then on, calls
    /// stay headless, so the agent keeps the dataset it loaded.
    headless_loaded: bool,
}

#[derive(Clone)]
struct McpHandler {
    headless: SessionHandle,
    target: Arc<Mutex<Target>>,
}

impl McpHandler {
    /// Sends a call to the viewer if one is attached or can be found, and otherwise to the
    /// headless session.
    ///
    /// Until the agent loads a dataset headlessly, each call looks for a viewer, so TGV can be
    /// opened after the agent starts. With one viewer open, the call goes to it. With several,
    /// the call fails with `viewer_ambiguous` until the agent attaches to one.
    async fn call(&self, call: Call) -> Result<Value, SessionError> {
        let mut target = self.target.lock().await;
        if target.viewer.is_none() && !target.headless_loaded {
            let mut viewers = SessionConnection::list_viewers().await;
            match viewers.len() {
                0 => {}
                1 => {
                    let (connection, info) = viewers.remove(0);
                    log::info!("Attached to the viewer at {}", connection.path.display());
                    target.viewer = Some((connection, info.pid));
                }
                _ => {
                    return Err(SessionError::ViewerAmbiguous(
                        viewers.into_iter().map(|(_, info)| info).collect(),
                    ));
                }
            }
        }
        if let Some((viewer, _)) = target.viewer.as_mut() {
            let result = viewer.call(&call).await;
            if matches!(result, Err(SessionError::Connection(_))) {
                log::warn!("Detached from the viewer at {}", viewer.path.display());
                target.viewer = None;
            }
            return result;
        }
        let loads = matches!(call, Call::LoadDataset(_));
        let result = self.headless.call(call).await;
        if loads && result.is_ok() {
            target.headless_loaded = true;
        }
        result
    }
}

/// Chooses a viewer by its process ID.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AttachRequest {
    /// The viewer's process ID, from `list_viewers`.
    pid: u32,
}

#[tool_router]
impl McpHandler {
    #[tool(
        description = "List the open TGV viewers: each one's pid, kind (tui or gui), reference, files, displayed locus, and start time, and which one this server uses. Use it to choose a viewer when several are open."
    )]
    async fn list_viewers(&self) -> CallToolResult {
        let viewers = SessionConnection::list_viewers()
            .await
            .into_iter()
            .map(|(_, info)| info)
            .collect::<Vec<_>>();
        let target = self.target.lock().await;
        tool_result(Ok(json!({
            "viewers": viewers,
            "attached": target.viewer.as_ref().map(|(_, pid)| pid),
        })))
    }

    #[tool(
        description = "Send later calls to the TGV viewer with this pid, from list_viewers. When several viewers are open and the question doesn't say which one, ask the user before attaching."
    )]
    async fn attach(&self, Parameters(request): Parameters<AttachRequest>) -> CallToolResult {
        let result = match SessionConnection::connect_to(request.pid).await {
            Ok(mut connection) => match connection.call(&Call::ViewerInfo).await {
                Ok(info) => {
                    log::info!("Attached to the viewer at {}", connection.path.display());
                    let mut target = self.target.lock().await;
                    target.viewer = Some((connection, request.pid));
                    target.headless_loaded = false;
                    Ok(info)
                }
                Err(_) => Err(SessionError::ViewerNotFound(request.pid)),
            },
            Err(_) => Err(SessionError::ViewerNotFound(request.pid)),
        };
        tool_result(result)
    }

    #[tool(
        description = "Describe the loaded reference and tracks, or report that no dataset is loaded."
    )]
    async fn get_dataset(&self) -> CallToolResult {
        match self.call(Call::Describe).await {
            Ok(Value::Null) => CallToolResult::structured(json!({"loaded": false})),
            result => tool_result(result),
        }
    }

    #[tool(
        description = "Load or replace a dataset from files on this computer. Track IDs reset on replacement. A dataset that a TGV viewer displays can't be replaced."
    )]
    async fn load_dataset(
        &self,
        Parameters(request): Parameters<DatasetRequest>,
    ) -> CallToolResult {
        tool_result(self.call(Call::LoadDataset(request)).await)
    }

    #[tool(
        description = "Inspect a 1-based inclusive genome interval and return track, gene, and coverage statistics."
    )]
    async fn inspect_interval(
        &self,
        Parameters(request): Parameters<InspectRequest>,
    ) -> CallToolResult {
        tool_result(self.call(Call::Inspect(request)).await)
    }

    #[tool(
        description = "Describe the SQL tables available to the query tool: columns, types, keys, scopes, usage notes, and example queries."
    )]
    async fn describe_tables(&self) -> CallToolResult {
        tool_result(TablesResponse::new().and_then(|tables| {
            serde_json::to_value(tables).map_err(|error| SessionError::Core(error.into()))
        }))
    }

    #[tool(
        description = "Run a read-only Polars SQL query over the dataset's reads, CIGAR operations, mismatches, base modifications, coverage, reference, variants, BED intervals, and genes. Call describe_tables first. Region tables need a 1-based inclusive region of at most 100,000 bases."
    )]
    async fn query(&self, Parameters(request): Parameters<QueryRequest>) -> CallToolResult {
        tool_result(self.call(Call::Query(request)).await)
    }

    #[tool(
        description = "Show a 1-based inclusive region in the user's TGV viewer, fitted to the screen. The viewer tells the user how to go back. Needs a running TGV viewer."
    )]
    async fn navigate(&self, Parameters(request): Parameters<NavigateRequest>) -> CallToolResult {
        tool_result(self.call(Call::Navigate(request)).await)
    }

    #[tool(
        description = "Report the region and zoom that the user's TGV viewer shows. Needs a running TGV viewer."
    )]
    async fn view_state(&self) -> CallToolResult {
        tool_result(self.call(Call::ViewState).await)
    }
}

#[tool_handler(
    name = "tgv",
    instructions = "When the user has TGV open, tools use the dataset it displays, and navigate shows results in it. Otherwise, load a dataset before inspecting or querying. File paths refer to the server's filesystem. If several TGV viewers are open, calls fail with viewer_ambiguous: choose the viewer from the question's context, such as a file, a locus, or the TUI or the GUI, ask the user when it is still unclear, and call attach."
)]
impl ServerHandler for McpHandler {}

/// Serves MCP over standard input and output, through a running viewer or a headless session.
///
/// `settings` provides the reference, backend, cache, and host defaults; datasets are loaded
/// only through the `load_dataset` tool.
pub async fn serve(settings: Settings) -> Result<(), McpError> {
    let (session, worker) = Session::spawn(settings);
    let service = McpHandler {
        headless: session.clone(),
        target: Arc::new(Mutex::new(Target {
            viewer: None,
            headless_loaded: false,
        })),
    }
    .serve(stdio())
    .await;
    let service_result = match service {
        Ok(running) => match running.waiting().await {
            Ok(QuitReason::Closed | QuitReason::Cancelled) => Ok(()),
            Ok(QuitReason::JoinError(error)) => Err(McpError::Service(error.to_string())),
            Err(error) => Err(McpError::Service(error.to_string())),
            Ok(other) => Err(McpError::UnexpectedStop(format!("{other:?}"))),
        },
        Err(error) => Err(McpError::Initialize(error.to_string())),
    };

    session.shutdown().await;
    let worker_result = worker
        .await
        .map_err(|error| McpError::Worker(error.to_string()))?;
    service_result?;
    Ok(worker_result?)
}

/// Converts a session result to a tool result with structured content. Calls without a
/// result report an empty object.
fn tool_result(result: Result<Value, SessionError>) -> CallToolResult {
    let error = match result {
        Ok(Value::Null) => return CallToolResult::structured(json!({})),
        Ok(value) => return CallToolResult::structured(value),
        Err(error) => error,
    };
    let (code, field, message) = (error.code(), error.field(), error.to_string());
    let mut result = CallToolResult::structured_error(
        json!({"error": {"code": code, "message": message, "field": field}}),
    );
    result.content = vec![ContentBlock::text(format!("{code}: {message}"))];
    result
}

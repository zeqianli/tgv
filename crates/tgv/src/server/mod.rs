//! Local MCP access to one dataset, serialized through a single server owner.

mod dataset_state;
mod schema;

use self::{dataset_state::DatasetState, schema::*};
use crate::settings::{Cli, Settings};
use axum::Router;
use gv_core::prelude::*;
use rmcp::{
    ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

enum Command {
    Describe,
    Replace(DatasetRequest),
    Inspect(InspectRequest),
    Draw(DrawRequest),
    Shutdown,
}

type Reply = Result<Value, TGVError>;
type Sender = mpsc::Sender<(Command, oneshot::Sender<Reply>)>;

#[derive(Clone)]
struct McpHandler {
    sender: Sender,
}

#[tool_router]
impl McpHandler {
    #[tool(
        description = "Describe the loaded reference and tracks, or report that no dataset is loaded."
    )]
    async fn get_dataset(&self) -> CallToolResult {
        self.dispatch(Command::Describe).await
    }

    #[tool(
        description = "Load or replace a dataset from files on this computer. Track IDs reset on replacement."
    )]
    async fn load_dataset(
        &self,
        Parameters(request): Parameters<DatasetRequest>,
    ) -> CallToolResult {
        self.dispatch(Command::Replace(request)).await
    }

    #[tool(
        description = "Inspect a 1-based inclusive genome interval and return track, gene, and coverage statistics."
    )]
    async fn inspect_interval(
        &self,
        Parameters(request): Parameters<InspectRequest>,
    ) -> CallToolResult {
        self.dispatch(Command::Inspect(request)).await
    }

    #[tool(description = "Draw a genome viewport as plain text or ANSI-colored terminal output.")]
    async fn draw_viewport(&self, Parameters(request): Parameters<DrawRequest>) -> CallToolResult {
        self.dispatch(Command::Draw(request)).await
    }

    async fn dispatch(&self, command: Command) -> CallToolResult {
        let (reply, response) = oneshot::channel();
        if self.sender.send((command, reply)).await.is_err() {
            return tool_result(Err(TGVError::McpInternal {
                message: "The dataset worker is unavailable.".to_owned(),
            }));
        }
        match response.await {
            Ok(result) => tool_result(result),
            Err(_) => tool_result(Err(TGVError::McpInternal {
                message: "The dataset worker stops before replying.".to_owned(),
            })),
        }
    }
}

#[tool_handler(
    name = "tgv",
    instructions = "Load a dataset before inspecting intervals or drawing viewports. File paths refer to the server's filesystem."
)]
impl ServerHandler for McpHandler {}

/// Serve MCP without initializing a terminal or reading a saved session.
pub async fn serve(cli: &Cli, port: u16) -> Result<(), TGVError> {
    let mut settings = Settings::default();
    cli.apply_overrides(&mut settings)?;
    if !settings.core.file_paths.is_empty() || cli.session.is_some() {
        return Err(TGVError::CliError(
            "Use the load_dataset MCP tool to load files when serving.".into(),
        ));
    }

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|source| TGVError::ServerBindError { port, source })?;
    let (sender, receiver) = mpsc::channel::<(Command, oneshot::Sender<Reply>)>(16);
    let shutdown_sender = sender.clone();
    let cancellation = CancellationToken::new();
    let config = StreamableHttpServerConfig::default()
        .with_cancellation_token(cancellation.child_token())
        .with_json_response(true)
        .with_max_request_body_bytes(1024 * 1024)
        .with_allowed_origins(["http://localhost:*", "http://127.0.0.1:*"]);
    let service = StreamableHttpService::new(
        move || {
            Ok(McpHandler {
                sender: sender.clone(),
            })
        },
        LocalSessionManager::default().into(),
        config,
    );
    let router = Router::new().nest_service("/mcp", service);
    println!("TGV serves MCP at http://{}/mcp", listener.local_addr()?);

    // Synchronous readers and coverage work must not block the HTTP accept loop.
    // Construct the dataset state inside this thread because repository types need not be Send.
    let runtime = tokio::runtime::Handle::current();
    let worker = tokio::task::spawn_blocking(move || {
        runtime.block_on(DatasetState::run(settings, receiver))
    });
    let shutdown_token = cancellation.clone();
    let http_result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown().await;
            shutdown_token.cancel();
        })
        .await;
    cancellation.cancel();
    let (reply, _) = oneshot::channel();
    let _ = shutdown_sender.send((Command::Shutdown, reply)).await;
    let worker_result = worker
        .await
        .map_err(|error| TGVError::StateError(format!("The dataset worker fails: {error}")))?;
    http_result?;
    worker_result
}

async fn shutdown() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => { if let Err(error) = result { log::error!("Failed to listen for Ctrl-C: {error}"); } },
                    _ = terminate.recv() => {},
                }
            }
            Err(error) => {
                log::error!("Failed to listen for termination: {error}");
                if let Err(error) = tokio::signal::ctrl_c().await {
                    log::error!("Failed to listen for Ctrl-C: {error}");
                }
            }
        }
    }
    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        log::error!("Failed to listen for Ctrl-C: {error}");
    }
}

fn tool_result(result: Reply) -> CallToolResult {
    match result {
        Ok(value) => CallToolResult::structured(value),
        Err(error) => {
            let (code, field) = match &error {
                TGVError::McpInvalidInput { field, .. } => ("invalid_input", Some(*field)),
                TGVError::McpNoDataset { .. } => ("no_dataset", None),
                _ => ("internal_error", None),
            };
            let message = error.to_string();
            let mut result = CallToolResult::structured_error(
                json!({"error": {"code": code, "message": message, "field": field}}),
            );
            result.content = vec![ContentBlock::text(format!("{code}: {message}"))];
            result
        }
    }
}

fn as_json(value: &impl Serialize) -> Reply {
    serde_json::to_value(value).map_err(|error| TGVError::McpInternal {
        message: error.to_string(),
    })
}

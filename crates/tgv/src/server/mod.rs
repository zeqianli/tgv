//! Local stdio MCP access to one dataset, serialized through a single owner.

mod dataset_state;
mod schema;
mod tables;

use self::{dataset_state::DatasetState, schema::*};
use crate::settings::{Cli, Settings};
use gv_core::prelude::*;
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    service::QuitReason,
    tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

enum Command {
    Describe,
    Replace(DatasetRequest),
    Inspect(InspectRequest),
    Query(QueryRequest),
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

    #[tool(
        description = "Describe the SQL tables available to the query tool: columns, types, keys, scopes, usage notes, and example queries."
    )]
    async fn describe_tables(&self) -> CallToolResult {
        tool_result(TablesResponse::new().and_then(|tables| as_json(&tables)))
    }

    #[tool(
        description = "Run a read-only Polars SQL query over the dataset's reads, CIGAR operations, mismatches, base modifications, coverage, reference, variants, BED intervals, and genes. Call describe_tables first. Region tables need a 1-based inclusive region of at most 100,000 bases."
    )]
    async fn query(&self, Parameters(request): Parameters<QueryRequest>) -> CallToolResult {
        self.dispatch(Command::Query(request)).await
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

/// Serve MCP over stdin and stdout without initializing a terminal or reading a saved session.
pub async fn serve(cli: &Cli) -> Result<(), TGVError> {
    let mut settings = Settings::default();
    cli.apply_overrides(&mut settings)?;
    if !settings.core.file_paths.is_empty() || cli.resume.is_some() {
        return Err(TGVError::CliError(
            "Use the load_dataset MCP tool to load files when serving.".into(),
        ));
    }

    let (sender, receiver) = mpsc::channel::<(Command, oneshot::Sender<Reply>)>(16);
    let running = (McpHandler {
        sender: sender.clone(),
    })
    .serve(stdio())
    .await
    .map_err(|error| {
        TGVError::StateError(format!("The MCP service fails to initialize: {error}"))
    })?;

    // Synchronous readers and coverage work must not block MCP message handling.
    // Construct the dataset state inside this thread because repository types need not be Send.
    let runtime = tokio::runtime::Handle::current();
    let worker = tokio::task::spawn_blocking(move || {
        runtime.block_on(DatasetState::run(settings, receiver))
    });

    let reason = running.waiting().await;
    let service_result = match reason {
        Ok(QuitReason::Closed | QuitReason::Cancelled) => Ok(()),
        Ok(QuitReason::JoinError(error)) | Err(error) => Err(TGVError::StateError(format!(
            "The MCP service fails: {error}"
        ))),
        Ok(other) => Err(TGVError::StateError(format!(
            "The MCP service stops unexpectedly: {other:?}"
        ))),
    };

    let (reply, _) = oneshot::channel();
    let _ = sender.send((Command::Shutdown, reply)).await;
    let worker_result = worker
        .await
        .map_err(|error| TGVError::StateError(format!("The dataset worker fails: {error}")))?;
    service_result?;
    worker_result
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

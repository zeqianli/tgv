//! MCP tools over a TGV session, served over standard input and output.
//!
//! Each tool translates an MCP call into a session command and its typed reply into a tool
//! result. The session runs in this process, headless.

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
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

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

#[derive(Clone)]
struct McpHandler {
    session: SessionHandle,
}

#[tool_router]
impl McpHandler {
    #[tool(
        description = "Describe the loaded reference and tracks, or report that no dataset is loaded."
    )]
    async fn get_dataset(&self) -> CallToolResult {
        match self.session.describe().await {
            Ok(None) => CallToolResult::structured(json!({"loaded": false})),
            result => tool_result(result),
        }
    }

    #[tool(
        description = "Load or replace a dataset from files on this computer. Track IDs reset on replacement."
    )]
    async fn load_dataset(
        &self,
        Parameters(request): Parameters<DatasetRequest>,
    ) -> CallToolResult {
        tool_result(self.session.load_dataset(request).await)
    }

    #[tool(
        description = "Inspect a 1-based inclusive genome interval and return track, gene, and coverage statistics."
    )]
    async fn inspect_interval(
        &self,
        Parameters(request): Parameters<InspectRequest>,
    ) -> CallToolResult {
        tool_result(self.session.inspect(request).await)
    }

    #[tool(
        description = "Describe the SQL tables available to the query tool: columns, types, keys, scopes, usage notes, and example queries."
    )]
    async fn describe_tables(&self) -> CallToolResult {
        tool_result(TablesResponse::new())
    }

    #[tool(
        description = "Run a read-only Polars SQL query over the dataset's reads, CIGAR operations, mismatches, base modifications, coverage, reference, variants, BED intervals, and genes. Call describe_tables first. Region tables need a 1-based inclusive region of at most 100,000 bases."
    )]
    async fn query(&self, Parameters(request): Parameters<QueryRequest>) -> CallToolResult {
        tool_result(self.session.query(request).await)
    }
}

#[tool_handler(
    name = "tgv",
    instructions = "Load a dataset before inspecting or querying. File paths refer to the server's filesystem."
)]
impl ServerHandler for McpHandler {}

/// Serves MCP over standard input and output with a headless session.
///
/// `settings` provides the reference, backend, cache, and host defaults; datasets are loaded
/// only through the `load_dataset` tool.
pub async fn serve(settings: Settings) -> Result<(), McpError> {
    let (session, worker) = Session::spawn(settings);
    let service = McpHandler {
        session: session.clone(),
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

/// Converts a session result to a tool result with structured content.
fn tool_result(result: Result<impl Serialize, SessionError>) -> CallToolResult {
    let error = match result.map(|value| serde_json::to_value(&value)) {
        Ok(Ok(value)) => return CallToolResult::structured(value),
        Ok(Err(error)) => SessionError::Core(error.into()),
        Err(error) => error,
    };
    let (code, field) = match &error {
        SessionError::InvalidInput { field, .. } => ("invalid_input", Some(*field)),
        SessionError::NoDataset { .. } => ("no_dataset", None),
        SessionError::Unavailable | SessionError::Stopped | SessionError::Core(_) => {
            ("internal_error", None)
        }
    };
    let message = error.to_string();
    let mut result = CallToolResult::structured_error(
        json!({"error": {"code": code, "message": message, "field": field}}),
    );
    result.content = vec![ContentBlock::text(format!("{code}: {message}"))];
    result
}

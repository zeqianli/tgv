//! Local HTTP access to one dataset, serialized through a single server owner.

mod error;
mod schema;
mod server;

use self::{error::*, schema::*, server::Server};
use crate::settings::{Cli, Settings};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State, rejection::JsonRejection},
    http::StatusCode,
    routing::{get, post},
};
use gv_core::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

enum Command {
    Describe,
    Replace(DatasetRequest),
    Inspect(InspectRequest),
    Draw(DrawRequest),
}

type Reply = Result<Value, ApiError>;
type Sender = mpsc::Sender<(Command, oneshot::Sender<Reply>)>;

/// Serve HTTP without initializing a terminal or reading a saved session.
pub async fn serve(cli: &Cli, port: u16) -> Result<(), TGVError> {
    let mut settings = Settings::default();
    cli.apply_overrides(&mut settings)?;
    if !settings.core.file_paths.is_empty() || cli.session.is_some() {
        return Err(TGVError::CliError(
            "Use PUT /v1/dataset to load files when serving.".into(),
        ));
    }

    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let (sender, receiver) = mpsc::channel::<(Command, oneshot::Sender<Reply>)>(16);
    let router = Router::new()
        .route(
            "/v1/health",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route("/v1/dataset", get(describe).put(replace))
        .route("/v1/inspect", post(inspect))
        .route("/v1/draw", post(draw))
        .fallback(|| async { ApiError::not_found() })
        .method_not_allowed_fallback(|| async { ApiError::method_not_allowed() })
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(sender);
    println!("TGV serves http://{}", listener.local_addr()?);

    // Synchronous readers and coverage work must not block the HTTP accept loop.
    // Construct the server inside this thread because repository types need not be Send.
    let runtime = tokio::runtime::Handle::current();
    let worker =
        tokio::task::spawn_blocking(move || runtime.block_on(Server::run(settings, receiver)));
    let http = axum::serve(listener, router).with_graceful_shutdown(shutdown());
    let (http_result, worker_result) = tokio::join!(http, worker);
    http_result?;
    worker_result
        .map_err(|error| TGVError::StateError(format!("The dataset worker fails: {error}")))?
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

async fn dispatch(sender: Sender, command: Command) -> Result<Json<Value>, ApiError> {
    let (reply, response) = oneshot::channel();
    sender
        .send((command, reply))
        .await
        .map_err(|_| ApiError::internal("The dataset worker is unavailable."))?;
    response
        .await
        .map_err(|_| ApiError::internal("The dataset worker stopped before replying."))?
        .map(Json)
}

async fn describe(State(sender): State<Sender>) -> Result<Json<Value>, ApiError> {
    dispatch(sender, Command::Describe).await
}

async fn replace(
    State(sender): State<Sender>,
    request: Result<Json<DatasetRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    dispatch(sender, Command::Replace(request.map_err(json_error)?.0)).await
}

async fn inspect(
    State(sender): State<Sender>,
    request: Result<Json<InspectRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    dispatch(sender, Command::Inspect(request.map_err(json_error)?.0)).await
}

async fn draw(
    State(sender): State<Sender>,
    request: Result<Json<DrawRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    dispatch(sender, Command::Draw(request.map_err(json_error)?.0)).await
}

fn json_error(error: JsonRejection) -> ApiError {
    ApiError {
        status: StatusCode::BAD_REQUEST,
        code: "invalid_request",
        message: error.body_text(),
        field: None,
    }
}

fn as_json(value: &impl Serialize) -> Reply {
    serde_json::to_value(value).map_err(ApiError::internal)
}

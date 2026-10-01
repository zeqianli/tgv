use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use gv_core::prelude::*;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("{message}")]
pub(super) struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub field: Option<&'static str>,
}

impl ApiError {
    pub fn invalid(field: &'static str, message: impl ToString) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "invalid_input",
            message: message.to_string(),
            field: Some(field),
        }
    }

    pub fn internal(message: impl ToString) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: message.to_string(),
            field: None,
        }
    }

    pub fn conflict(code: &'static str, message: impl ToString) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code,
            message: message.to_string(),
            field: None,
        }
    }

    pub fn not_found() -> Self {
        ApiError {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: "The endpoint does not exist.".into(),
            field: None,
        }
    }

    pub fn method_not_allowed() -> Self {
        ApiError {
            status: StatusCode::METHOD_NOT_ALLOWED,
            code: "method_not_allowed",
            message: "The endpoint does not support this HTTP method.".into(),
            field: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(
                json!({"error": {"code": self.code, "message": self.message, "field": self.field}}),
            ),
        )
            .into_response()
    }
}

impl From<TGVError> for ApiError {
    fn from(value: TGVError) -> Self {
        Self::internal(value)
    }
}

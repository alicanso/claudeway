use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiError {
    pub error: String,
    pub code: &'static str,
}

#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub body: ApiError,
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> AppError {
        AppError {
            status: StatusCode::BAD_REQUEST,
            body: ApiError {
                error: msg.into(),
                code: "bad_request",
            },
        }
    }

    pub fn unauthorized() -> AppError {
        AppError {
            status: StatusCode::UNAUTHORIZED,
            body: ApiError {
                error: "Unauthorized".to_string(),
                code: "unauthorized",
            },
        }
    }

    pub fn not_found(msg: impl Into<String>) -> AppError {
        AppError {
            status: StatusCode::NOT_FOUND,
            body: ApiError {
                error: msg.into(),
                code: "not_found",
            },
        }
    }

    pub fn timeout() -> AppError {
        AppError {
            status: StatusCode::REQUEST_TIMEOUT,
            body: ApiError {
                error: "Request timed out".to_string(),
                code: "timeout",
            },
        }
    }

    pub fn internal(msg: impl Into<String>) -> AppError {
        AppError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: ApiError {
                error: msg.into(),
                code: "internal_error",
            },
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::to_string(&self.body).unwrap_or_else(|_| {
            r#"{"error":"Internal server error","code":"internal_error"}"#.to_string()
        });

        Response::builder()
            .status(self.status)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap()
    }
}

//! One error type for all handlers, rendered as [`musicus_api::ApiError`] JSON.

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use musicus_api::{ApiError, ErrorCode, Field};

pub type ApiResult<T> = Result<T, AppError>;

/// An HTTP status plus the [`ErrorCode`] sent to the client. Variants take
/// codes only, never text: the server builds no prose for users (a-error-codes,
/// R-I18N-004). `Internal` carries a detail for the log, never sent.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("login required")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(ErrorCode),
    #[error("{0}")]
    BadRequest(ErrorCode),
    #[error("{0}")]
    Conflict(ErrorCode),
    /// A JSON body other than [`ApiError`], e.g. [`musicus_api::PageConflict`].
    #[error("conflict")]
    ConflictWith(serde_json::Value),
    #[error("{0}")]
    Unprocessable(ErrorCode),
    #[error("request body larger than {0} bytes")]
    PayloadTooLarge(u64),
    #[error("{0}")]
    UnsupportedMediaType(ErrorCode),
    #[error("{0}")]
    PreconditionRequired(ErrorCode),
    /// Minutes to wait.
    #[error("too many attempts, try again in {0} minutes")]
    TooManyRequests(u64),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        AppError::Internal(e.to_string())
    }

    /// The code sent to the client (`ConflictWith` has its own body: `Unknown`).
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::NotFound => ErrorCode::NotFound,
            AppError::Unauthorized => ErrorCode::Unauthorized,
            AppError::Forbidden(c)
            | AppError::BadRequest(c)
            | AppError::Conflict(c)
            | AppError::Unprocessable(c)
            | AppError::UnsupportedMediaType(c)
            | AppError::PreconditionRequired(c) => c.clone(),
            AppError::ConflictWith(_) => ErrorCode::Unknown,
            AppError::PayloadTooLarge(max_bytes) => ErrorCode::PayloadTooLarge { max_bytes: *max_bytes },
            AppError::TooManyRequests(minutes) => ErrorCode::TooManyAttempts { minutes: *minutes },
            AppError::Internal(_) => ErrorCode::Internal,
        }
    }

    fn status_and_class(&self) -> (StatusCode, &'static str) {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Conflict(_) | AppError::ConflictWith(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::Unprocessable(_) => (StatusCode::UNPROCESSABLE_ENTITY, "unprocessable"),
            AppError::PayloadTooLarge(_) => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            AppError::UnsupportedMediaType(_) => (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type"),
            AppError::PreconditionRequired(_) => (StatusCode::PRECONDITION_REQUIRED, "precondition_required"),
            AppError::TooManyRequests(_) => (StatusCode::TOO_MANY_REQUESTS, "too_many_requests"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, class) = self.status_and_class();
        if let AppError::Internal(detail) = &self {
            tracing::error!(%detail, "internal error");
        }
        let mut response = match self {
            AppError::ConflictWith(body) => (status, Json(body)).into_response(),
            other => (status, Json(ApiError::new(class, other.code()))).into_response(),
        };
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::RowNotFound => AppError::NotFound,
            sqlx::Error::Database(db) if db.is_unique_violation() => AppError::Conflict(ErrorCode::AlreadyExists),
            sqlx::Error::Database(db) if db.is_foreign_key_violation() || is_restrict_violation(db.as_ref()) => {
                AppError::Conflict(ErrorCode::InUseOrUnknown)
            }
            _ => AppError::internal(e),
        }
    }
}

/// SQLite reports a failed `ON DELETE RESTRICT` (e.g. deleting an instrument
/// that parts still use) as `SQLITE_CONSTRAINT_TRIGGER` (extended code 1811),
/// which sqlx does not classify as a foreign key violation.
fn is_restrict_violation(db: &dyn sqlx::error::DatabaseError) -> bool {
    db.code().as_deref() == Some("1811") && db.message().contains("FOREIGN KEY")
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::internal(e)
    }
}

/// Trims a required text field and rejects empty or overlong values.
pub fn required(field: Field, value: &str, max_chars: usize) -> ApiResult<String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(AppError::BadRequest(ErrorCode::FieldEmpty { field }));
    }
    if v.chars().count() > max_chars {
        return Err(too_long(field, max_chars));
    }
    Ok(v.to_owned())
}

/// Trims an optional text field; empty becomes `None`.
pub fn optional(field: Field, value: Option<&str>, max_chars: usize) -> ApiResult<Option<String>> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(v) if v.chars().count() > max_chars => Err(too_long(field, max_chars)),
        Some(v) => Ok(Some(v.to_owned())),
    }
}

/// 400 [`ErrorCode::FieldTooLong`].
pub fn too_long(field: Field, max_chars: usize) -> AppError {
    AppError::BadRequest(ErrorCode::FieldTooLong { field, max: max_chars as u32 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body(e: AppError) -> (StatusCode, serde_json::Value) {
        let r = e.into_response();
        let status = r.status();
        (status, serde_json::from_slice(&to_bytes(r.into_body(), 1 << 16).await.unwrap()).unwrap())
    }

    #[tokio::test]
    async fn responses_carry_code_args_and_an_english_message() {
        let (status, v) = body(AppError::BadRequest(ErrorCode::TooManyTags { max: 30 })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            v,
            serde_json::json!({"error": "bad_request", "code": "too_many_tags", "args": {"max": 30},
                               "message": "at most 30 tags"})
        );
        let (_, v) = body(AppError::TooManyRequests(15)).await;
        assert_eq!((v["code"].as_str(), v["args"]["minutes"].as_u64()), (Some("too_many_attempts"), Some(15)));
    }

    #[tokio::test]
    async fn internal_details_stay_in_the_log() {
        let (status, v) = body(AppError::internal("disk /secret/path full")).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(v, serde_json::json!({"error": "internal", "code": "internal", "message": "internal error"}));
    }
}

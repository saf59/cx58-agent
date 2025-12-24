// shared/src/error.rs

use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// Main Error Type
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    // Convenience constructors
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::NotFound,
            format!("{} not found", resource.into()),
        )
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthorized, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, message)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ValidationError, message)
    }

    pub fn service_unavailable(service: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::ServiceUnavailable,
            format!("{} service unavailable", service.into()),
        )
    }

    pub fn rate_limit() -> Self {
        Self::new(ErrorCode::RateLimitExceeded, "Rate limit exceeded")
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

// ============================================================================
// Error Codes
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    // Client errors (4xx)
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    ValidationError,
    RateLimitExceeded,
    PayloadTooLarge,
    UnsupportedMediaType,

    // Server errors (5xx)
    Internal,
    ServiceUnavailable,
    DatabaseError,
    StorageError,
    ExternalServiceError,

    // Domain specific
    ImageProcessingError,
    EmbeddingGenerationError,
    ModelError,
    TreeOperationError,
}

impl ErrorCode {
    pub fn http_status(&self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::Conflict => 409,
            Self::ValidationError => 422,
            Self::RateLimitExceeded => 429,
            Self::PayloadTooLarge => 413,
            Self::UnsupportedMediaType => 415,
            Self::Internal => 500,
            Self::ServiceUnavailable => 503,
            Self::DatabaseError => 500,
            Self::StorageError => 500,
            Self::ExternalServiceError => 502,
            Self::ImageProcessingError => 500,
            Self::EmbeddingGenerationError => 500,
            Self::ModelError => 500,
            Self::TreeOperationError => 500,
        }
    }

    pub fn is_client_error(&self) -> bool {
        self.http_status() < 500
    }

    pub fn is_server_error(&self) -> bool {
        self.http_status() >= 500
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::BadRequest => "BAD_REQUEST",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::ValidationError => "VALIDATION_ERROR",
            Self::RateLimitExceeded => "RATE_LIMIT_EXCEEDED",
            Self::PayloadTooLarge => "PAYLOAD_TOO_LARGE",
            Self::UnsupportedMediaType => "UNSUPPORTED_MEDIA_TYPE",
            Self::Internal => "INTERNAL_ERROR",
            Self::ServiceUnavailable => "SERVICE_UNAVAILABLE",
            Self::DatabaseError => "DATABASE_ERROR",
            Self::StorageError => "STORAGE_ERROR",
            Self::ExternalServiceError => "EXTERNAL_SERVICE_ERROR",
            Self::ImageProcessingError => "IMAGE_PROCESSING_ERROR",
            Self::EmbeddingGenerationError => "EMBEDDING_GENERATION_ERROR",
            Self::ModelError => "MODEL_ERROR",
            Self::TreeOperationError => "TREE_OPERATION_ERROR",
        };
        write!(f, "{}", s)
    }
}

// ============================================================================
// Result Type Alias
// ============================================================================

pub type Result<T> = std::result::Result<T, AppError>;

// ============================================================================
// Error Response for HTTP
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: AppError,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub timestamp: String,
}

impl ErrorResponse {
    pub fn new(error: AppError) -> Self {
        Self {
            error,
            request_id: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }
}

// ============================================================================
// Validation Error Details
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub code: String,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            code: "INVALID".to_string(),
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = code.into();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationErrors {
    pub errors: Vec<ValidationError>,
}

impl ValidationErrors {
    pub fn new() -> Self {
        Self { errors: vec![] }
    }

    pub fn add(&mut self, error: ValidationError) {
        self.errors.push(error);
    }

    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn into_app_error(self) -> AppError {
        AppError::new(ErrorCode::ValidationError, "Validation failed")
            .with_details(serde_json::to_value(self).unwrap())
    }
}

impl Default for ValidationErrors {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Error Conversion Implementations
// ============================================================================

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => Self::not_found("Resource"),
            sqlx::Error::Database(e) => {
                if e.is_unique_violation() {
                    Self::conflict("Resource already exists")
                } else {
                    Self::new(ErrorCode::DatabaseError, format!("Database error: {}", e))
                }
            }
            _ => Self::new(ErrorCode::DatabaseError, format!("Database error: {}", err)),
        }
    }
}

impl From<redis::RedisError> for AppError {
    fn from(err: redis::RedisError) -> Self {
        Self::new(
            ErrorCode::ExternalServiceError,
            format!("Redis error: {}", err),
        )
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::bad_request(format!("JSON error: {}", err))
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::internal(format!("IO error: {}", err))
    }
}

// ============================================================================
// Backend-specific HTTP Response Conversion
// ============================================================================

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        use axum::Json;

        let status = StatusCode::from_u16(self.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let response = ErrorResponse::new(self);

        (status, Json(response)).into_response()
    }
}

// ============================================================================
// Error Context Extension
// ============================================================================

pub trait ErrorContext<T> {
    fn context(self, context: impl Into<String>) -> Result<T>;
}

impl<T, E: Into<AppError>> ErrorContext<T> for std::result::Result<T, E> {
    fn context(self, context: impl Into<String>) -> Result<T> {
        self.map_err(|e| {
            let mut err = e.into();
            err.message = format!("{}: {}", context.into(), err.message);
            err
        })
    }
}

// ============================================================================
// Helpers
// ============================================================================

pub fn log_error(error: &AppError) {
    if error.code.is_server_error() {
        log::error!("{}", error);
    } else {
        log::warn!("{}", error);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = AppError::not_found("User");
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(err.message.contains("User"));
    }

    #[test]
    fn test_error_with_details() {
        let err = AppError::validation("Invalid input")
            .with_details(serde_json::json!({"field": "email"}));
        assert!(err.details.is_some());
    }

    #[test]
    fn test_http_status() {
        assert_eq!(ErrorCode::NotFound.http_status(), 404);
        assert_eq!(ErrorCode::Internal.http_status(), 500);
    }

    #[test]
    fn test_error_classification() {
        assert!(ErrorCode::BadRequest.is_client_error());
        assert!(ErrorCode::Internal.is_server_error());
    }

    #[test]
    fn test_validation_errors() {
        let mut errors = ValidationErrors::new();
        errors.add(ValidationError::new("email", "Invalid email"));
        errors.add(ValidationError::new("password", "Too short"));
        assert_eq!(errors.errors.len(), 2);

        let app_error = errors.into_app_error();
        assert_eq!(app_error.code, ErrorCode::ValidationError);
    }

    #[test]
    fn test_error_display() {
        let err = AppError::not_found("Resource");
        let display = format!("{}", err);
        assert!(display.contains("NOT_FOUND"));
        assert!(display.contains("Resource"));
    }

    #[test]
    fn test_json_serialization() {
        let err = AppError::bad_request("Invalid data");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("BAD_REQUEST"));
    }
}
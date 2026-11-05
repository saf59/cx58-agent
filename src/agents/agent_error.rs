// src/agents/agent_error.rs
//
// Unified, localization-aware error type for the agent subsystem.
//
// ## Design
// - `AgentError` is an enum that covers every distinct failure mode across all agents.
// - Each variant carries only the minimal data needed to render a human-readable,
//   language-specific message via `LocalizationManager` + Fluent `.ftl` bundles.
// - The top-level request owner renders each error and emits the only terminal event.
//
// ## Usage in agents
// ```rust
// use crate::agents::agent_error::AgentError;
//
// // Return from any agent:
// return Err( AgentError::MissingObjectId.into());
//
// ```

use std::fmt;

use crate::agents::LocalizationManager;
use fluent_bundle::FluentArgs;

// ---------------------------------------------------------------------------
// AgentError
// ---------------------------------------------------------------------------

/// Typed error enum covering all agent failure modes.
///
/// Each variant maps 1-to-1 to a Fluent message key in
/// `locales/{lang}/messages.ftl`.  The key convention is:
///   `error-<kebab-case-variant-name>`
///
/// Variants that carry dynamic data expose it as named Fluent variables
/// (see `.fluent_args()` below).
#[derive(Debug, Clone)]
pub enum AgentError {
    // --- Context / input errors ---
    /// The request was canceled by the client.
    Cancelled,

    /// `object_id` was not provided in `AgentContext`.
    MissingObjectId,

    /// A UUID string could not be parsed.
    /// * `raw` – the raw string that failed parsing.
    InvalidUuid { raw: String },

    /// The requested object was not found in the database.
    /// * `id` – the object id that was looked up.
    ObjectNotFound { id: String },

    /// More than one accessible object has the requested display name.
    AmbiguousObject { name: String },

    /// No documents / reports match the query criteria.
    NoDocumentsFound,

    /// A request asks for more records than the bounded response contract permits.
    ResultLimitExceeded { limit: usize },

    /// Request fields violate the public input contract.
    InvalidRequest,

    // --- LLM / prompt errors ---
    /// The LLM returned a response that could not be parsed as the expected JSON.
    /// * `detail` – a short human-readable description of the parse failure.
    LlmJsonParseError { detail: String },

    /// A required prompt template could not be rendered.
    /// * `template` – the template name.
    TemplateRenderError { template: String },

    /// A localization message key was not found.
    /// * `key` – the missing key.
    LocalizationKeyMissing { key: String },

    /// A bounded runtime stage exceeded its configured deadline.
    DeadlineExceeded { stage: &'static str },

    // --- Date / time errors ---
    /// A date string could not be parsed.
    /// * `raw` – the raw string that failed parsing.
    DateParseError { raw: String },

    // --- Storage errors ---
    /// The S3 storage operation failed.
    /// * `detail` – short description from the underlying error.
    StorageError { detail: String },

    // --- Generic / internal ---
    /// A comparison requires at least two descriptions but fewer were supplied.
    InsufficientDescriptions { found: usize },

    /// Catch-all for unexpected internal failures.
    /// * `detail` – original error message.
    Internal { detail: String },
}

// ---------------------------------------------------------------------------
// Display / std::error::Error
// ---------------------------------------------------------------------------

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Fallback English messages used when the localization layer itself fails.
        match self {
            Self::Cancelled => write!(f, "Request was cancelled"),
            Self::MissingObjectId => write!(f, "Object ID is required but was not provided"),
            Self::InvalidUuid { raw } => write!(f, "Invalid UUID: '{}'", raw),
            Self::ObjectNotFound { id } => write!(f, "Object '{}' not found", id),
            Self::AmbiguousObject { name } => {
                write!(f, "More than one object is named '{}'", name)
            }
            Self::NoDocumentsFound => write!(f, "No documents found matching the criteria"),
            Self::ResultLimitExceeded { limit } => {
                write!(f, "Result exceeds the limit of {} records", limit)
            }
            Self::InvalidRequest => write!(f, "Invalid agent request"),
            Self::LlmJsonParseError { detail } => {
                write!(f, "Failed to parse LLM response: {}", detail)
            }
            Self::TemplateRenderError { template } => {
                write!(f, "Template render error: '{}'", template)
            }
            Self::LocalizationKeyMissing { key } => {
                write!(f, "Missing localization key: '{}'", key)
            }
            Self::DeadlineExceeded { stage } => {
                write!(f, "Agent stage '{}' exceeded its deadline", stage)
            }
            Self::DateParseError { raw } => write!(f, "Failed to parse date: '{}'", raw),
            Self::StorageError { detail } => write!(f, "Storage error: {}", detail),
            Self::InsufficientDescriptions { found } => {
                write!(
                    f,
                    "Need at least 2 descriptions for comparison, got {}",
                    found
                )
            }
            Self::Internal { detail } => write!(f, "Internal agent error: {}", detail),
        }
    }
}

impl std::error::Error for AgentError {}

// Note: `From<AgentError> for Box<dyn Error + Send + Sync>` is provided
// automatically by the standard library because AgentError implements
// Error + Send + Sync + 'static. An explicit impl would conflict.

// ---------------------------------------------------------------------------
// Fluent key + variable helpers
// ---------------------------------------------------------------------------

impl AgentError {
    /// Returns the Fluent message key for this error variant.
    /// Must match a key defined in `locales/{lang}/messages.ftl`.
    pub fn fluent_key(&self) -> &'static str {
        match self {
            Self::Cancelled => "error-cancelled",
            Self::MissingObjectId => "error-missing-object-id",
            Self::InvalidUuid { .. } => "error-invalid-uuid",
            Self::ObjectNotFound { .. } => "error-object-not-found",
            Self::AmbiguousObject { .. } => "error-ambiguous-object",
            Self::NoDocumentsFound => "error-no-documents-found",
            Self::ResultLimitExceeded { .. } => "error-result-limit-exceeded",
            Self::InvalidRequest => "error-invalid-request",
            Self::LlmJsonParseError { .. } => "error-llm-json-parse",
            Self::TemplateRenderError { .. } => "error-template-render",
            Self::LocalizationKeyMissing { .. } => "error-localization-key-missing",
            Self::DeadlineExceeded { .. } => "error-deadline-exceeded",
            Self::DateParseError { .. } => "error-date-parse",
            Self::StorageError { .. } => "error-storage",
            Self::InsufficientDescriptions { .. } => "error-insufficient-descriptions",
            Self::Internal { .. } => "error-internal",
        }
    }

    /// Builds a `FluentArgs` map for message variable substitution.
    ///
    /// Matches the signature expected by `LocalizationManager::get_msg_with_args`.
    /// Returns `None` for parameter-less variants so callers can skip arg building.
    pub fn fluent_args(&self) -> Option<FluentArgs<'static>> {
        // Helper: allocate args only when there are actual parameters.
        macro_rules! args {
            ($($k:literal => $v:expr),+ $(,)?) => {{
                let mut a = FluentArgs::new();
                $( a.set($k, $v); )+
                Some(a)
            }};
        }

        match self {
            Self::Cancelled => None,
            Self::InvalidUuid { raw } => args!("raw"      => raw.clone()),
            Self::ObjectNotFound { id } => args!("id"       => id.clone()),
            Self::AmbiguousObject { name } => args!("name" => name.clone()),
            Self::LlmJsonParseError { .. } => None,
            Self::TemplateRenderError { template } => args!("template" => template.clone()),
            Self::LocalizationKeyMissing { key } => args!("key"      => key.clone()),
            Self::DeadlineExceeded { .. } => None,
            Self::DateParseError { raw } => args!("raw"      => raw.clone()),
            Self::StorageError { .. } => None,
            Self::InsufficientDescriptions { found } => args!("found"    => found.to_string()),
            Self::ResultLimitExceeded { limit } => args!("limit" => limit.to_string()),
            Self::Internal { .. } => None,
            // Parameter-less variants
            Self::MissingObjectId | Self::NoDocumentsFound | Self::InvalidRequest => None,
        }
    }

    // -----------------------------------------------------------------------
    // Localized message rendering
    // -----------------------------------------------------------------------

    /// Renders a localized human-readable error message.
    ///
    /// Delegates to the real `LocalizationManager`:
    /// - with args  → `get_msg_with_args(lang, key, args)`
    /// - no args    → `get_msg(lang, key)`
    ///
    /// Falls back to `Display` (English) if the manager returns an empty /
    /// placeholder string (e.g. missing key).
    pub fn localized_message(&self, lang: &str, lang_manager: &LocalizationManager) -> String {
        let key = self.fluent_key();

        let rendered = match self.fluent_args() {
            Some(args) => lang_manager.get_msg_with_args(lang, key, args),
            None => lang_manager.get_msg(lang, key),
        };

        // If the manager returned a "Missing message: …" placeholder or an
        // empty string, fall back to the Display impl so the client always
        // sees something meaningful.
        if rendered.is_empty() || rendered.starts_with("Missing message:") {
            self.to_string()
        } else {
            rendered
        }
    }

    // -----------------------------------------------------------------------
    // Convenience constructors (keep error sites readable)
    // -----------------------------------------------------------------------

    /// Wraps any `std::error::Error` as `AgentError::Internal`.
    pub fn internal(e: impl fmt::Display) -> Self {
        Self::Internal {
            detail: e.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// ResultExt – ergonomic `.agent_err()` on Result
// ---------------------------------------------------------------------------

/// Extension trait that lets any `Result<T, E: Display>` be converted to
/// `Result<T, AgentError>` without a verbose `map_err` closure.
pub trait ResultExt<T> {
    fn agent_err(self) -> Result<T, AgentError>;
}

impl<T, E: fmt::Display> ResultExt<T> for Result<T, E> {
    fn agent_err(self) -> Result<T, AgentError> {
        self.map_err(|e| AgentError::internal(e))
    }
}
impl From<rig::completion::CompletionError> for AgentError {
    fn from(e: rig::completion::CompletionError) -> Self {
        AgentError::internal(e)
    }
}

impl From<anyhow::Error> for AgentError {
    fn from(e: anyhow::Error) -> Self {
        AgentError::internal(e)
    }
}
impl From<sqlx::Error> for AgentError {
    fn from(e: sqlx::Error) -> Self {
        AgentError::Internal {
            detail: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AgentError;
    use crate::agents::LocalizationManager;

    /// Verifies that internal diagnostic details never reach localized client text.
    #[test]
    fn internal_error_message_is_redacted() {
        let error = AgentError::Internal {
            detail: "secret model response and signed URL".to_string(),
        };
        let manager = LocalizationManager::new();

        let english = error.localized_message("en", &manager);
        let german = error.localized_message("de", &manager);

        assert!(!english.contains("secret model response"));
        assert!(!german.contains("secret model response"));
        assert!(english.contains("internal error"));
        assert!(german.contains("interner Fehler"));
    }

    /// Verifies that provider parse details are hidden from localized client text.
    #[test]
    fn llm_parse_error_message_is_redacted() {
        let error = AgentError::LlmJsonParseError {
            detail: "raw response payload".to_string(),
        };
        let manager = LocalizationManager::new();

        let message = error.localized_message("en", &manager);

        assert!(!message.contains("raw response payload"));
        assert!(message.contains("unexpected response"));
    }
}

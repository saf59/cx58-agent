// src/agents/agent_error.rs
//
// Unified, localization-aware error type for the agent subsystem.
//
// ## Design
// - `AgentError` is an enum that covers every distinct failure mode across all agents.
// - Each variant carries only the minimal data needed to render a human-readable,
//   language-specific message via `LocalizationManager` + Fluent `.ftl` bundles.
// - `send_to_client()` renders the message and pushes it as `StreamEvent::Error`
//   so every agent uses a single consistent code path.
//
// ## Usage in agents
// ```rust
// use crate::agents::agent_error::AgentError;
//
// // Return from any agent:
// return Err(AgentError::MissingObjectId.into());
//
// // Or send directly and continue / stop:
// AgentError::NoDocumentsFound
//     .send_to_client(&self.event_tx, &self.context, &self.lang_manager)
//     .await;
// ```

use std::fmt;
use tokio::sync::mpsc;
use std::sync::Arc;

use fluent_bundle::FluentArgs;
use crate::{AgentContext, StreamEvent};
use crate::agents::LocalizationManager;

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

    /// `object_id` was not provided in `AgentContext`.
    MissingObjectId,

    /// A UUID string could not be parsed.
    /// * `raw` – the raw string that failed parsing.
    InvalidUuid { raw: String },

    /// The requested object was not found in the database.
    /// * `id` – the object id that was looked up.
    ObjectNotFound { id: String },

    /// No documents / reports match the query criteria.
    NoDocumentsFound,

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
            Self::MissingObjectId => write!(f, "Object ID is required but was not provided"),
            Self::InvalidUuid { raw } => write!(f, "Invalid UUID: '{}'", raw),
            Self::ObjectNotFound { id } => write!(f, "Object '{}' not found", id),
            Self::NoDocumentsFound => write!(f, "No documents found matching the criteria"),
            Self::LlmJsonParseError { detail } => write!(f, "Failed to parse LLM response: {}", detail),
            Self::TemplateRenderError { template } => write!(f, "Template render error: '{}'", template),
            Self::LocalizationKeyMissing { key } => write!(f, "Missing localization key: '{}'", key),
            Self::DateParseError { raw } => write!(f, "Failed to parse date: '{}'", raw),
            Self::StorageError { detail } => write!(f, "Storage error: {}", detail),
            Self::InsufficientDescriptions { found } => {
                write!(f, "Need at least 2 descriptions for comparison, got {}", found)
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
            Self::MissingObjectId           => "error-missing-object-id",
            Self::InvalidUuid { .. }        => "error-invalid-uuid",
            Self::ObjectNotFound { .. }     => "error-object-not-found",
            Self::NoDocumentsFound          => "error-no-documents-found",
            Self::LlmJsonParseError { .. }  => "error-llm-json-parse",
            Self::TemplateRenderError { .. }=> "error-template-render",
            Self::LocalizationKeyMissing {..}=> "error-localization-key-missing",
            Self::DateParseError { .. }     => "error-date-parse",
            Self::StorageError { .. }       => "error-storage",
            Self::InsufficientDescriptions {..}=> "error-insufficient-descriptions",
            Self::Internal { .. }           => "error-internal",
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
            Self::InvalidUuid { raw }                => args!("raw"      => raw.clone()),
            Self::ObjectNotFound { id }              => args!("id"       => id.clone()),
            Self::LlmJsonParseError { detail }       => args!("detail"   => detail.clone()),
            Self::TemplateRenderError { template }   => args!("template" => template.clone()),
            Self::LocalizationKeyMissing { key }     => args!("key"      => key.clone()),
            Self::DateParseError { raw }             => args!("raw"      => raw.clone()),
            Self::StorageError { detail }            => args!("detail"   => detail.clone()),
            Self::InsufficientDescriptions { found } => args!("found"    => found.to_string()),
            Self::Internal { detail }                => args!("detail"   => detail.clone()),
            // Parameter-less variants
            Self::MissingObjectId | Self::NoDocumentsFound => None,
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
            None       => lang_manager.get_msg(lang, key),
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
    // SSE helpers
    // -----------------------------------------------------------------------

    /// Sends a localized `StreamEvent::Error` to the client channel.
    ///
    /// This is the canonical way for any agent to report an error to the
    /// frontend — call this instead of constructing `StreamEvent::Error` inline.
    ///
    /// # Arguments
    /// * `tx`           – the SSE sender for this request.
    /// * `context`      – the current `AgentContext` (provides `request_id` and `language`).
    /// * `lang_manager` – shared `LocalizationManager`.
    pub async fn send_to_client(
        &self,
        tx: &mpsc::Sender<StreamEvent>,
        context: &AgentContext,
        lang_manager: &Arc<LocalizationManager>,
    ) {
        let lang = crate::agents::Language::from_short(&context.language);
        let message = self.localized_message(lang.to_code(), lang_manager);

        let _ = tx
            .send(StreamEvent::Error {
                request_id: context.request_id.clone(),
                error: message,
            })
            .await;
    }

    // -----------------------------------------------------------------------
    // Convenience constructors (keep error sites readable)
    // -----------------------------------------------------------------------

    /// Wraps any `std::error::Error` as `AgentError::Internal`.
    pub fn internal(e: impl fmt::Display) -> Self {
        Self::Internal { detail: e.to_string() }
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

// Public module exports
pub mod events;
pub mod task_detector;
//pub mod master_agent;
pub mod object_agent;
pub mod document_agent;
pub mod comparison_agent;
pub mod chat_agent;
pub mod filter_objects;
pub mod description;
pub mod tera;
pub mod intent_router;
pub mod master_agent_update;
pub mod orchestrator;
pub mod response_formatter;
pub mod types;
pub mod cancellation;
pub mod agent_context;
pub mod agents_helper;

pub use crate::localization::LocalizationManager;
pub use agent_context::AgentContext;
pub use cancellation::{CancellationToken, RequestManager};
pub use chat_agent::ChatAgent;
pub use comparison_agent::ComparisonAgent;
pub use description::description_agent::DescriptionAgent;
pub use document_agent::DocumentAgent;
pub use events::StreamEvent;
//pub use master_agent::MasterAgent;
pub use object_agent::ObjectAgent;
pub use crate::prompt_context::{ContextParser, ParserError, PromptContext, PromptKey, Period};
pub use task_detector::{Task, TaskDetector, };
// Re-export main types for convenience
pub use types::*;

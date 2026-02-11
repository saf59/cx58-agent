// Public module exports
pub mod events;
pub mod prompt_context;
pub mod task_detector;
pub mod master_agent;
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
// Re-export main types for convenience
pub use types::*;
pub use events::StreamEvent;
pub use crate::localization::LocalizationManager;
pub use prompt_context::{ContextParser, ParserError, Period, PromptContext, PromptKey};
pub use task_detector::{Task, TaskDetector, TaskParameters};
pub use object_agent::ObjectAgent;
pub use document_agent::DocumentAgent;
pub use description::description_agent::DescriptionAgent;
pub use comparison_agent::ComparisonAgent;
pub use chat_agent::ChatAgent;
pub use master_agent::{AgentContext, AgentRequest, CancellationToken, MasterAgent, RequestManager};
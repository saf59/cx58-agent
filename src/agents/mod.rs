// Public module exports
pub mod events;
pub mod prompt_context;
pub mod task_detector;
pub mod master_agent;
pub mod object_agent;
pub mod document_agent;
pub mod description_agent;
pub mod comparison_agent;
pub mod chat_agent;
pub mod lang;
pub mod helper;

// Re-export main types for convenience
pub use events::StreamEvent;
pub use lang::TextManager;
pub use prompt_context::{ContextParser, PromptContext, PromptKey, Period, ParserError};
pub use task_detector::{Task, TaskDetector, TaskParameters};
pub use object_agent::ObjectAgent;
pub use document_agent::DocumentAgent;
pub use description_agent::DescriptionAgent;
pub use comparison_agent::ComparisonAgent;
pub use chat_agent::ChatAgent;
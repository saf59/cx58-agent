pub mod description_agent;
mod description_helper;
mod description_json;
mod description_build;

// Re-export for convenience
pub use description_agent::DescriptionAgent;
pub use description_helper::{resolve_node_data, resolve_node_url};
pub use description_json::{build_description_data, build_description_json, DescriptionData, DescriptionContent};

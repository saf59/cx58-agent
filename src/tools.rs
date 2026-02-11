use anyhow::Result;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::ser::StdError;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
#[error("App error")]
pub struct CXError;

// This impl is intentionally unused - CXError is a simple marker type
// and doesn't need error conversion from Box<dyn StdError>
impl From<Box<dyn StdError + Send + Sync + 'static>> for CXError {
    #[inline(always)]
    fn from(_b: Box<dyn StdError + Send + Sync + 'static>) -> Self {
        CXError
    }
}

#[derive(Deserialize, Serialize)]
pub struct CXImage {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}
#[derive(Deserialize,Debug)]
pub struct IdArgs {
    id: String,
}

// tool Descriptor
#[derive(Deserialize, Serialize)]
pub struct Descriptor;

impl Tool for Descriptor {
    const NAME: &'static str = "descriptor";
    type Error = CXError;
    type Args = IdArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "descriptor".to_string(),
            description: "Descrip document by it ID".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Id of the document to describe"
                    },
                },
                "required": ["id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let result = args.id;
        Ok(result)
    }
}
#[allow(dead_code)]
#[derive(Deserialize, Serialize)]
pub struct ImageFinder;

impl Tool for ImageFinder {
    const NAME: &'static str = "image_finder";
    type Error = CXError;
    type Args = IdArgs;
    type Output = CXImage;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "image_finder".to_string(),
            description: "Find images by query".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Id to search for images"
                    },
                },
                "required": ["id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!("Find:{:?}",args);
        let result = CXImage {
            url: "./data/2025-12-15.jpg".to_string(),
            storage_path: None,
            size: None,
            mime_type: None,
            hash: None,
            description: None,
        };
        Ok(result)
    }
}

#[derive(Deserialize, Serialize)]
pub struct CXNothing;

impl Tool for CXNothing {
    const NAME: &'static str = "nothing";
    type Error = CXError;
    type Args = String;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "descriptor".to_string(),
            description: "The default tool when there is no request for objects, buildings, structures, reports, images, videos, descriptions and comparisons.\
            Always call this function if the parameters are not found.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        let result = "I am nobody!".to_string();
        Ok(result)
    }
}

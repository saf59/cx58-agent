```rust
use rig::client::{CompletionClient, Nothing};
use rig::providers::ollama::{Client as OllamaClient};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use futures::pin_mut;
use rig::completion::{CompletionModel};
use rig::providers::ollama;
use crate::agent::*;
use crate::models::{AgentRequest, NodeData, StreamEvent};
// ============================================================================
// Rig-based Agent Chain Implementation
// ============================================================================

#[derive(Debug, Clone)]
pub struct RigAgentChain {
    pub ollama_client: Arc<OllamaClient>,
    pub text_model: String,
    pub vision_model: String,
}

impl RigAgentChain {
    pub fn new(ollama_url: &str) -> Self {
        let client:  ollama::Client = ollama::Client::builder()
            .api_key(Nothing)
            .base_url(ollama_url)
            .build()
            .unwrap();
        Self {
            ollama_client: Arc::new(client),
            text_model: "llama3.2".to_string(),
            vision_model: "llava".to_string(),
        }
    }

    /// Chain 1: Intent Detection
    pub async fn detect_intent(
        &self,
        user_message: &str,
        language: &str,
    ) -> Result<AgentIntent, String> {
        let prompt = format!(
            r#"You are an intent classifier. Analyze the user's message and respond ONLY with a JSON object.

User language: {}
User message: {}

Classify into one of these intents:
1. "describe_image" - user wants description of images
2. "compare_images" - user wants to compare multiple images
3. "general_query" - general question not about images
4. "create_tree_node" - user wants to create/modify tree structure

Response format (JSON only, no other text):
{{
  "intent": "<intent_name>",
  "confidence": 0.95,
  "entities": {{"image_count": 2, "comparison_type": "visual"}}
}}
"#,
            language, user_message
        );

        let model = self.ollama_client.completion_model(&self.text_model);

        let request = model
            .completion_request (&prompt)
            .preamble("You are a helpful AI assistant. Provide concise explanations.".to_string())
            .temperature(0.7)
            .build();
        let response = model.completion(request).await.unwrap();
        // Parse JSON response
        let cleaned = response
            .trim()
            .trim_start_matches("```json")
            .trim_end_matches("```");

        serde_json::from_str::<AgentIntent>(cleaned.)
            .map_err(|e| format!("Failed to parse intent: {}", e))
    }

    /// Chain 2: Image Description
    pub async fn describe_image(
        &self,
        image_url: &str,
        image_bytes: Vec<u8>,
        language: &str,
        custom_prompt: Option<&str>,
    ) -> Result<String, String> {
        let base_prompt = custom_prompt.unwrap_or(
            "Describe this image in detail. Focus on objects, colors, composition, and mood.",
        );

        let prompt = format!("Language for response: {}. {}", language, base_prompt);

        // Use vision model with Rig
        let model = self.ollama_client.completion_model(&self.vision_model);

        // Rig handles multimodal via special prompt format
        let vision_prompt = format!("[IMAGE: {}]\n{}", base64::encode(&image_bytes), prompt);

        let response = model
            .completion_request(&vision_prompt)
            //.await
            .map_err(|e| format!("Vision model error: {}", e))?;

        Ok(response)
    }

    /// Chain 3: Image Comparison
    pub async fn compare_images(
        &self,
        images: Vec<(String, Vec<u8>)>, // (url, bytes)
        language: &str,
        comparison_aspects: Vec<&str>,
    ) -> Result<String, String> {
        let mut descriptions = Vec::new();

        // First, describe each image
        for (i, (url, bytes)) in images.iter().enumerate() {
            let desc = self
                .describe_image(
                    url,
                    bytes.clone(),
                    language,
                    Some("Provide a detailed technical description for comparison purposes."),
                )
                .await?;

            descriptions.push(format!("Image {}: {}", i + 1, desc));
        }

        // Then, use text model to compare
        let comparison_prompt = format!(
            r#"Language: {}

Compare these images based on: {}

Image descriptions:
{}

Provide a detailed comparison highlighting similarities and differences."#,
            language,
            comparison_aspects.join(", "),
            descriptions.join("\n\n")
        );

        let model  = self.ollama_client.completion_model(&self.text_model);

        let response = model
            .completion_request(&comparison_prompt)
            .preamble("You are a helpful AI assistant. Provide concise explanations.".to_string())
            .temperature(0.7)
            .build();

        Ok(response)
    }

    /// Chain 4: Stream Response with RAG
    pub async fn stream_with_rag(
        &self,
        user_query: &str,
        context_nodes: Vec<String>, // Pre-fetched descriptions
        language: &str,
    ) -> Result<impl futures::Stream<Item = Result<String, String>>, String> {
        let context = context_nodes.join("\n\n");

        let prompt = format!(
            r#"Language: {}

Context from user's tree:
{}

User question: {}

Provide a helpful response using the context above. Reference specific items when relevant."#,
            language, context, user_query
        );

        let model: CompletionModel = self.ollama_client.completion_model(&self.text_model);

        // Rig streaming
        let stream = model
            .stream(&prompt)
            .await
            .map_err(|e| format!("Stream error: {}", e))?;

        Ok(stream.map(|chunk| chunk.map_err(|e| format!("Stream chunk error: {}", e))))
    }

    /// Chain 5: Generate Embeddings
    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>, String> {
        // Use Ollama's embedding model
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/api/embeddings", self.ollama_client.base_url()))
            .json(&serde_json::json!({
                "model": "nomic-embed-text",
                "prompt": text
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;

        json.get("embedding")
            .and_then(|e| e.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect()
            })
            .ok_or_else(|| "Failed to extract embedding".to_string())
    }
}

// ============================================================================
// Intent Structure
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIntent {
    pub intent: String,
    pub confidence: f32,
    pub entities: serde_json::Value,
}

// ============================================================================
// Multi-Agent Orchestrator
// ============================================================================

pub struct AgentOrchestrator {
    pub chain: RigAgentChain,
    pub db: sqlx::PgPool,
}

impl AgentOrchestrator {
    pub fn new(
        ollama_url: &str,
        db: sqlx::PgPool,
    ) -> Self {
        Self {
            chain: RigAgentChain::new(ollama_url),
            db,
        }
    }

    pub async fn execute_intent(
        &self,
        intent: AgentIntent,
        request: &AgentRequest,
        state: &Arc<AppState>,
    ) -> Result<impl futures::Stream<Item = Result<StreamEvent, String>>, String> {
        match intent.intent.as_str() {
            "describe_image" => self.handle_image_description(request, state).await,
            //"compare_images" => self.handle_image_comparison(request, state).await,
            //"general_query" => self.handle_general_query(request, state).await,
            _ => Err(format!("Unknown intent: {}", intent.intent)),
        }
    }

    async fn handle_image_description(
        &self,
        request: &AgentRequest,
        state: &Arc<AppState>,
    ) -> Result<impl futures::Stream<Item = Result<StreamEvent, String>>, String> {
        use futures::stream;

        let tree_refs = request.tree_context.clone().unwrap_or_default();
        let language = request.language.clone();
        let message = request.message.clone();

        // Load images
        let images = get_image_nodes(&state.db, &tree_refs, &request.user_id)
            .await
            .map_err(|e| e.to_string())?;

        let chain = self.chain.clone();

        Ok(async_stream::stream! {
            for img_node in images {
                if let NodeData::Image { url, .. } = &img_node.data {
                    yield Ok(StreamEvent::ToolCall {
                        tool: "describe_image".to_string(),
                        status: format!("Processing {}", url),
                    });

                    // Download image
                    let bytes = match download_image(url).await {
                        Ok(b) => b,
                        Err(e) => {
                            yield Ok(StreamEvent::Error {
                                error: format!("Download failed: {}", e),
                            });
                            continue;
                        }
                    };

                    // Describe
                    match chain.describe_image(url, bytes, &language, None).await {
                        Ok(description) => {
                            yield Ok(StreamEvent::TextChunk {
                                content: format!("\n\n**{}:**\n{}\n", url, description),
                            });

                            // Save to cache
                            let _ = sqlx::query!(
                                "INSERT INTO image_descriptions (node_id, model_name, prompt, description)
                                 VALUES ($1, $2, $3, $4)
                                 ON CONFLICT (node_id, model_name, prompt) DO UPDATE
                                 SET description = $4",
                                img_node.id,
                                "llava",
                                message,
                                description
                            ).execute(&self.db).await;
                        }
                        Err(e) => {
                            yield Ok(StreamEvent::Error {
                                error: format!("Description failed: {}", e),
                            });
                        }
                    }
                }
            }
        })
    }

    async fn handle_image_comparison(
        &self,
        request: &AgentRequest,
        state: &Arc<AppState>,
    ) -> Result<impl futures::Stream<Item = Result<StreamEvent, String>>, String> {
        let tree_refs = request.tree_context.clone().unwrap_or_default();
        let language = request.language.clone();

        // Load images
        let image_nodes = get_image_nodes(&state.db, &tree_refs, &request.user_id)
            .await
            .map_err(|e| e.to_string())?;

        let mut images = Vec::new();
        for node in image_nodes {
            if let NodeData::Image { url, .. } = &node.data {
                if let Ok(bytes) = download_image(url).await {
                    images.push((url.clone(), bytes));
                }
            }
        }

        let chain = self.chain.clone();
        let aspects = vec!["composition", "colors", "subject matter", "style"];

        Ok(async_stream::stream! {
            yield Ok(StreamEvent::ToolCall {
                tool: "compare_images".to_string(),
                status: format!("Comparing {} images", images.len()),
            });

            match chain.compare_images(images, &language, aspects).await {
                Ok(comparison) => {
                    // Stream the comparison in chunks
                    for chunk in comparison.chars().collect::<Vec<_>>().chunks(50) {
                        let chunk_str: String = chunk.iter().collect();
                        yield Ok(StreamEvent::TextChunk {
                            content: chunk_str,
                        });
                        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                    }
                }
                Err(e) => {
                    yield Ok(StreamEvent::Error {
                        error: format!("Comparison failed: {}", e),
                    });
                }
            }
        })
    }

    async fn handle_general_query(
        &self,
        request: &AgentRequest,
        state: &Arc<AppState>,
    ) -> Result<impl futures::Stream<Item = Result<StreamEvent, String>>, String> {
        // Load context from tree
        let context = if let Some(refs) = &request.tree_context {
            let nodes = load_tree_nodes(&state.db, refs, &request.user_id)
                .await
                .map_err(|e| e.to_string())?;

            nodes.iter().map(|n| format!("{:?}", n.data)).collect()
        } else {
            vec![]
        };

        let mut stream = self
            .chain
            .stream_with_rag(&request.message, context, &request.language)
            .await?;

        Ok(async_stream::stream! {
            pin_mut!(stream); // ??
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(text) => {
                        yield Ok(StreamEvent::TextChunk { content: text });
                    }
                    Err(e) => {
                        yield Ok(StreamEvent::Error { error: e });
                    }
                }
            }
        })
    }

}
```
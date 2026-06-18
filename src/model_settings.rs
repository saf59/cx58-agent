use crate::error::{AppError, Result};
use crate::storage::AiConfig;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    Vision,
    Tools,
    Text,
}

impl ModelCapability {
    pub fn from_query(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "vision" | "visual" | "image" => Some(Self::Vision),
            "tools" | "tool" => Some(Self::Tools),
            "text" | "completion" | "chat" => Some(Self::Text),
            _ => None,
        }
    }

    fn ollama_capability(self) -> &'static str {
        match self {
            Self::Vision => "vision",
            Self::Tools => "tools",
            Self::Text => "completion",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserModelSettings {
    pub user_id: String,
    pub vision_model: String,
    pub text_model: String,
    pub chat_model: String,
}

impl UserModelSettings {
    pub fn from_defaults(user_id: &str, defaults: &AiConfig) -> Self {
        Self {
            user_id: user_id.to_string(),
            vision_model: defaults.vision_model.clone(),
            text_model: defaults.text_model.clone(),
            chat_model: defaults.chat_model.clone(),
        }
    }

    pub fn to_ai_config(&self, defaults: &AiConfig) -> AiConfig {
        AiConfig {
            url: defaults.url.clone(),
            text_model: self.text_model.clone(),
            vision_model: self.vision_model.clone(),
            chat_model: self.chat_model.clone(),
            agent_secret: defaults.agent_secret.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModelInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_level: Option<String>,
}

impl OllamaModelInfo {
    fn supports(&self, capability: ModelCapability) -> bool {
        let required = capability.ollama_capability();
        self.capabilities
            .iter()
            .any(|cap| cap.eq_ignore_ascii_case(required))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChangeResult {
    pub role: String,
    pub model: String,
    pub applied: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateModelsRequest {
    pub vision_model: Option<String>,
    pub text_model: Option<String>,
    pub chat_model: Option<String>,
    #[serde(default)]
    pub same: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserModelsResponse {
    pub user_id: String,
    pub current: UserModelSettings,
    pub defaults: UserModelSettings,
    pub models: Vec<OllamaModelInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<ModelCapability>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateModelsResponse {
    pub user_id: String,
    pub current: UserModelSettings,
    pub changes: Vec<ModelChangeResult>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    #[serde(alias = "model")]
    name: String,
    size: Option<i64>,
    modified_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaShowResponse {
    capabilities: Option<Vec<String>>,
    details: Option<OllamaShowDetails>,
}

#[derive(Debug, Deserialize)]
struct OllamaShowDetails {
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

pub async fn load_user_model_settings(
    pool: &PgPool,
    user_id: &str,
    defaults: &AiConfig,
) -> UserModelSettings {
    let row = sqlx::query(
        r#"
        SELECT user_id, vision_model, text_model, chat_model
        FROM user_model_settings
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await;

    match row {
        Ok(Some(row)) => UserModelSettings {
            user_id: row.get("user_id"),
            vision_model: row.get("vision_model"),
            text_model: row.get("text_model"),
            chat_model: row.get("chat_model"),
        },
        Ok(None) => UserModelSettings::from_defaults(user_id, defaults),
        Err(e) => {
            tracing::warn!(user_id, "Failed to load user model settings: {}", e);
            UserModelSettings::from_defaults(user_id, defaults)
        }
    }
}

pub async fn save_user_model_settings(pool: &PgPool, settings: &UserModelSettings) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO user_model_settings
            (user_id, vision_model, text_model, chat_model, updated_at)
        VALUES ($1, $2, $3, $4, NOW())
        ON CONFLICT (user_id) DO UPDATE SET
            vision_model = EXCLUDED.vision_model,
            text_model   = EXCLUDED.text_model,
            chat_model   = EXCLUDED.chat_model,
            updated_at   = NOW()
        "#,
    )
    .bind(&settings.user_id)
    .bind(&settings.vision_model)
    .bind(&settings.text_model)
    .bind(&settings.chat_model)
    .execute(pool)
    .await
    .map_err(AppError::from)?;

    Ok(())
}

pub async fn effective_ai_config(pool: &PgPool, user_id: &str, defaults: &AiConfig) -> AiConfig {
    load_user_model_settings(pool, user_id, defaults)
        .await
        .to_ai_config(defaults)
}

pub async fn list_ollama_models(
    ollama_url: &str,
    capability: Option<ModelCapability>,
) -> Result<Vec<OllamaModelInfo>> {
    let client = reqwest::Client::new();
    let tags_url = format!("{}/api/tags", ollama_url.trim_end_matches('/'));
    let tags = client
        .get(&tags_url)
        .send()
        .await
        .map_err(|e| AppError::service_unavailable(format!("Ollama tags: {}", e)))?
        .error_for_status()
        .map_err(|e| AppError::service_unavailable(format!("Ollama tags: {}", e)))?
        .json::<OllamaTagsResponse>()
        .await
        .map_err(|e| AppError::service_unavailable(format!("Ollama tags JSON: {}", e)))?;

    let mut models = Vec::with_capacity(tags.models.len());
    for tag in tags.models {
        match show_ollama_model(&client, ollama_url, &tag.name).await {
            Ok(show) => {
                let info = OllamaModelInfo {
                    name: tag.name,
                    size: tag.size,
                    modified_at: tag.modified_at,
                    capabilities: show.capabilities.unwrap_or_default(),
                    family: show.details.as_ref().and_then(|d| d.family.clone()),
                    parameter_size: show.details.as_ref().and_then(|d| d.parameter_size.clone()),
                    quantization_level: show
                        .details
                        .as_ref()
                        .and_then(|d| d.quantization_level.clone()),
                };
                if capability.is_none_or(|cap| info.supports(cap)) {
                    models.push(info);
                }
            }
            Err(e) => {
                tracing::warn!(model = %tag.name, "Failed to inspect Ollama model: {}", e);
            }
        }
    }

    Ok(models)
}

pub async fn inspect_ollama_model(ollama_url: &str, model: &str) -> Result<OllamaModelInfo> {
    let client = reqwest::Client::new();
    let show = show_ollama_model(&client, ollama_url, model).await?;
    Ok(OllamaModelInfo {
        name: model.to_string(),
        size: None,
        modified_at: None,
        capabilities: show.capabilities.unwrap_or_default(),
        family: show.details.as_ref().and_then(|d| d.family.clone()),
        parameter_size: show.details.as_ref().and_then(|d| d.parameter_size.clone()),
        quantization_level: show
            .details
            .as_ref()
            .and_then(|d| d.quantization_level.clone()),
    })
}

pub fn supports_role(model: &OllamaModelInfo, role: &str) -> bool {
    match role {
        "vision_model" => model.supports(ModelCapability::Vision),
        "text_model" => model.supports(ModelCapability::Text),
        "chat_model" => model.supports(ModelCapability::Tools),
        _ => false,
    }
}

pub fn required_capability_label(role: &str) -> &'static str {
    match role {
        "vision_model" => "vision",
        "text_model" => "completion",
        "chat_model" => "tools",
        _ => "unknown",
    }
}

async fn show_ollama_model(
    client: &reqwest::Client,
    ollama_url: &str,
    model: &str,
) -> Result<OllamaShowResponse> {
    let show_url = format!("{}/api/show", ollama_url.trim_end_matches('/'));
    client
        .post(&show_url)
        .json(&serde_json::json!({ "model": model }))
        .send()
        .await
        .map_err(|e| AppError::service_unavailable(format!("Ollama show: {}", e)))?
        .error_for_status()
        .map_err(|e| AppError::bad_request(format!("Ollama model not available: {}", e)))?
        .json::<OllamaShowResponse>()
        .await
        .map_err(|e| AppError::service_unavailable(format!("Ollama show JSON: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_with(capabilities: &[&str]) -> OllamaModelInfo {
        OllamaModelInfo {
            name: "test:latest".to_string(),
            size: None,
            modified_at: None,
            capabilities: capabilities.iter().map(|c| c.to_string()).collect(),
            family: None,
            parameter_size: None,
            quantization_level: None,
        }
    }

    #[test]
    fn role_capability_mapping_matches_api_contract() {
        let all = model_with(&["completion", "vision", "tools"]);
        assert!(supports_role(&all, "vision_model"));
        assert!(supports_role(&all, "text_model"));
        assert!(supports_role(&all, "chat_model"));

        let vision_only = model_with(&["completion", "vision"]);
        assert!(supports_role(&vision_only, "vision_model"));
        assert!(supports_role(&vision_only, "text_model"));
        assert!(!supports_role(&vision_only, "chat_model"));
    }

    #[test]
    fn capability_query_accepts_ui_aliases() {
        assert_eq!(
            ModelCapability::from_query("visual"),
            Some(ModelCapability::Vision)
        );
        assert_eq!(
            ModelCapability::from_query("completion"),
            Some(ModelCapability::Text)
        );
        assert_eq!(
            ModelCapability::from_query("tool"),
            Some(ModelCapability::Tools)
        );
        assert_eq!(ModelCapability::from_query("audio"), None);
    }
}

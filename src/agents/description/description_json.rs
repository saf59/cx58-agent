use crate::db_description::ImageDescription;
use crate::power_outlets::PowerOutletCount;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

pub const DESCRIPTION_PROMPT_VERSION: &str = "4";

/// Parsed structure of the description JSON content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptionContent {
    #[serde(
        rename = "_prompt_version",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub prompt_version: Option<String>,
    pub description: String,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub windows: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub doors: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub radiators: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_optional_text",
        skip_serializing_if = "Option::is_none"
    )]
    pub openings: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_outlet_candidates",
        skip_serializing_if = "Option::is_none"
    )]
    power_outlets: Option<Vec<OutletCandidate>>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum OutletCandidate {
    FullyInstalled,
    Unfinished,
    Unclear,
}

fn deserialize_outlet_candidates<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<OutletCandidate>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = JsonValue::deserialize(deserializer)?;
    Ok(match value {
        JsonValue::Array(items) => Some(
            items
                .into_iter()
                .map(|item| {
                    let label = match &item {
                        JsonValue::Object(fields) if fields.len() == 1 => fields.get("label"),
                        JsonValue::String(_) => Some(&item),
                        _ => None,
                    };
                    match label.and_then(JsonValue::as_str) {
                        Some("fully_installed") => OutletCandidate::FullyInstalled,
                        Some("unfinished") => OutletCandidate::Unfinished,
                        _ => OutletCandidate::Unclear,
                    }
                })
                .collect(),
        ),
        _ => None,
    })
}

fn deserialize_optional_text<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = JsonValue::deserialize(deserializer)?;
    Ok(description_value_to_text(&value))
}

fn description_value_to_text(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::Null => None,
        JsonValue::String(text) => non_empty(text.clone()),
        JsonValue::Array(items) => non_empty(
            items
                .iter()
                .filter_map(description_value_to_text)
                .collect::<Vec<_>>()
                .join(" | "),
        ),
        JsonValue::Object(fields) => non_empty(
            fields
                .iter()
                .filter_map(|(key, value)| {
                    description_value_to_text(value).map(|value| format!("{key}: {value}"))
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        JsonValue::Bool(value) => Some(value.to_string()),
        JsonValue::Number(value) => Some(value.to_string()),
    }
}

fn non_empty(text: String) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

impl DescriptionContent {
    pub(crate) fn power_outlet_count(&self) -> Option<PowerOutletCount> {
        let Some(candidates) = &self.power_outlets else {
            return self.uses_current_prompt().then_some(PowerOutletCount {
                confirmed: 0,
                uncertain: true,
            });
        };
        Some(PowerOutletCount {
            confirmed: candidates
                .iter()
                .filter(|candidate| matches!(candidate, OutletCandidate::FullyInstalled))
                .count() as u32,
            uncertain: candidates
                .iter()
                .any(|candidate| matches!(candidate, OutletCandidate::Unclear)),
        })
    }

    /// Reports whether this value belongs to the active prompt and schema contract.
    pub fn uses_current_prompt(&self) -> bool {
        self.prompt_version.as_deref() == Some(DESCRIPTION_PROMPT_VERSION)
    }

    /// Marks this value with the active prompt and schema contract.
    pub fn mark_current_prompt(&mut self) {
        self.prompt_version = Some(DESCRIPTION_PROMPT_VERSION.to_string());
    }

    /// Normalizes known structured field labels into the requested response language.
    pub fn normalize_language(&mut self, lang: &str) {
        for text in [
            &mut self.windows,
            &mut self.doors,
            &mut self.radiators,
            &mut self.openings,
        ]
        .into_iter()
        .flatten()
        {
            *text = normalize_structured_labels(text, lang);
        }
    }
}

/// Translates known model-generated schema labels without altering observation values.
fn normalize_structured_labels(text: &str, lang: &str) -> String {
    let replacements = if lang == "de" {
        [
            ("Count:", "Anzahl:"),
            ("Type:", "Typ:"),
            ("Handle:", "Griff:"),
            ("Condition:", "Zustand:"),
            ("Installation phase:", "Einbauphase:"),
            ("Doors:", "Türen:"),
            ("Windows:", "Fenster:"),
            ("Radiators:", "Heizkörper:"),
            ("Openings:", "Öffnungen:"),
        ]
    } else {
        [
            ("Anzahl:", "Count:"),
            ("Typ:", "Type:"),
            ("Griff:", "Handle:"),
            ("Zustand:", "Condition:"),
            ("Einbauphase:", "Installation phase:"),
            ("Türen:", "Doors:"),
            ("Fenster:", "Windows:"),
            ("Heizkörper:", "Radiators:"),
            ("Öffnungen:", "Openings:"),
        ]
    };

    replacements
        .into_iter()
        .fold(text.to_string(), |normalized, (from, to)| {
            normalized.replace(from, to)
        })
}

#[cfg(test)]
mod language_tests {
    use super::DescriptionContent;

    /// Verifies normalization of German structured keys for an English response.
    #[test]
    fn german_door_keys_are_normalized_for_english() {
        let mut content: DescriptionContent = serde_json::from_str(
            r#"{"description":"Room","doors":[{"Anzahl":1,"Typ":"interior door","Griff":"silver"}]}"#,
        )
        .unwrap();

        content.normalize_language("en");

        let doors = content.doors.unwrap();
        assert!(doors.contains("Count: 1"));
        assert!(doors.contains("Type: interior door"));
        assert!(doors.contains("Handle: silver"));
        assert!(!doors.contains("Anzahl:"));
    }
}

#[cfg(test)]
mod outlet_tests {
    use super::DescriptionContent;
    use crate::power_outlets::PowerOutletCount;

    #[test]
    fn counts_only_confirmed_candidates() {
        let content: DescriptionContent = serde_json::from_str(r#"{
            "description":"Room",
            "power_outlets":["fully_installed","unfinished","unclear",{"label":"fully_installed"},{"label":"fully_installed","condition":"exposed wiring"}]
        }"#).unwrap();
        assert_eq!(
            content.power_outlet_count(),
            Some(PowerOutletCount {
                confirmed: 2,
                uncertain: true
            })
        );
    }

    #[test]
    fn distinguishes_zero_from_missing_or_unclear_assessments() {
        let count = |json: &str| {
            serde_json::from_str::<DescriptionContent>(json)
                .unwrap()
                .power_outlet_count()
        };
        assert_eq!(count(r#"{"description":"Room"}"#), None);
        assert_eq!(count(r#"{"description":"Room","power_outlets":7}"#), None);
        assert_eq!(
            count(r#"{"description":"Room","power_outlets":["unfinished"]}"#),
            Some(PowerOutletCount {
                confirmed: 0,
                uncertain: false
            })
        );
        assert_eq!(
            count(r#"{"description":"Room","power_outlets":[]}"#),
            Some(PowerOutletCount {
                confirmed: 0,
                uncertain: false
            })
        );
        assert_eq!(
            count(r#"{"description":"Room","power_outlets":["unclear"]}"#),
            Some(PowerOutletCount {
                confirmed: 0,
                uncertain: true
            })
        );
    }

    #[test]
    fn pre_outlet_description_cache_is_stale() {
        let content: DescriptionContent =
            serde_json::from_str(r#"{"description":"Room","_prompt_version":"3"}"#).unwrap();
        assert!(!content.uses_current_prompt());
    }
}

/// Complete description data ready to send via SSE
#[derive(Debug, Clone, Serialize)]
pub struct DescriptionData {
    pub object: String,
    pub object_id: Uuid,
    pub date: String,
    pub date_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_url: Option<String>,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doors: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radiators: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openings: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) power_outlets: Option<PowerOutletCount>,
    pub model_name: String,
    pub confidence: Option<f32>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Build complete description data from ImageDescription and caller context
pub fn build_description_data(
    image_desc: &ImageDescription,
    object: &str,
    object_id: &Uuid,
    date: &str,
) -> Result<DescriptionData, Box<dyn std::error::Error + Send + Sync>> {
    // Parse the JSON description field
    let content: DescriptionContent = serde_json::from_str(&image_desc.description)?;

    Ok(DescriptionData {
        object: object.to_string(),
        object_id: *object_id,
        date: date.to_string(),
        date_id: image_desc.node_id, // node_id is date_id
        thumbnail_url: None,
        full_url: None,
        power_outlets: content.power_outlet_count(),
        description: content.description,
        windows: content.windows,
        doors: content.doors,
        radiators: content.radiators,
        openings: content.openings,
        model_name: image_desc.model_name.clone(),
        confidence: image_desc.confidence,
        created_at: image_desc.created_at,
    })
}

/// Alternative: build as generic JSON value for maximum flexibility
pub fn build_description_json(
    image_desc: &ImageDescription,
    object: &str,
    object_id: &Uuid,
    date: &str,
) -> Result<JsonValue, Box<dyn std::error::Error + Send + Sync>> {
    // Parse the JSON description field
    let mut content: JsonValue = serde_json::from_str(&image_desc.description)?;

    // Add caller context fields
    if let Some(obj) = content.as_object_mut() {
        let parsed: DescriptionContent = serde_json::from_str(&image_desc.description)?;
        obj.remove("power_outlets");
        if let Some(count) = parsed.power_outlet_count() {
            obj.insert("power_outlets".to_string(), serde_json::to_value(count)?);
        }
        obj.insert("object".to_string(), serde_json::json!(object));
        obj.insert("object_id".to_string(), serde_json::json!(object_id));
        obj.insert("date".to_string(), serde_json::json!(date));
        obj.insert("date_id".to_string(), serde_json::json!(image_desc.node_id));
        obj.insert(
            "model_name".to_string(),
            serde_json::json!(image_desc.model_name),
        );
        obj.insert(
            "confidence".to_string(),
            serde_json::json!(image_desc.confidence),
        );
        obj.insert(
            "created_at".to_string(),
            serde_json::json!(image_desc.created_at),
        );
    }

    Ok(content)
}

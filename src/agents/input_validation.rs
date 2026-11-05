use crate::AgentRequest;
use crate::agents::agent_error::AgentError;

const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_IDENTITY_BYTES: usize = 256;
const MAX_METADATA_BYTES: usize = 64 * 1024;

/// Validates bounded public request fields before routing or model use.
pub fn validate_request(request: &AgentRequest) -> Result<(), AgentError> {
    let language = request.language.trim().to_ascii_lowercase();
    let valid_language = matches!(language.as_str(), "en" | "english" | "de" | "german");
    let valid_identity = !request.user_id.trim().is_empty()
        && !request.chat_id.trim().is_empty()
        && request.user_id.len() <= MAX_IDENTITY_BYTES
        && request.chat_id.len() <= MAX_IDENTITY_BYTES;
    let valid_message =
        !request.message.trim().is_empty() && request.message.len() <= MAX_MESSAGE_BYTES;
    let valid_metadata = request
        .metadata
        .as_ref()
        .is_none_or(|metadata| metadata.to_string().len() <= MAX_METADATA_BYTES);

    if valid_language && valid_identity && valid_message && valid_metadata {
        Ok(())
    } else {
        Err(AgentError::InvalidRequest)
    }
}

#[cfg(test)]
mod tests {
    use super::validate_request;
    use crate::AgentRequest;
    use crate::agents::agent_error::AgentError;

    /// Builds a valid request for boundary tests.
    fn request() -> AgentRequest {
        AgentRequest {
            message: "Show descriptions".to_string(),
            user_id: "user".to_string(),
            chat_id: "chat".to_string(),
            language: "en".to_string(),
            object_id: None,
            prev_leaf: None,
            next_leaf: None,
            metadata: None,
        }
    }

    /// Verifies that unknown languages do not silently become English.
    #[test]
    fn unknown_language_is_rejected() {
        let mut request = request();
        request.language = "fr".to_string();

        assert!(matches!(
            validate_request(&request),
            Err(AgentError::InvalidRequest)
        ));
    }

    /// Verifies that oversized model prompts are rejected before routing.
    #[test]
    fn oversized_message_is_rejected() {
        let mut request = request();
        request.message = "x".repeat(16 * 1024 + 1);

        assert!(matches!(
            validate_request(&request),
            Err(AgentError::InvalidRequest)
        ));
    }
}

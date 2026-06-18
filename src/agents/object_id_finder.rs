use crate::agents::agent_error::AgentError;
use crate::db::get_tree;
use crate::{AgentContext, AppState};
use std::sync::Arc;

pub struct ObjectIdFinder {
    context: AgentContext,
}
/// It is not real AI agent at all.
/// Used to find object id by name in the list user of objects
impl ObjectIdFinder {
    pub fn new(context: AgentContext) -> Self {
        Self { context }
    }

    pub async fn execute(
        &self,
        state: Arc<AppState>,
        object_name: &str,
    ) -> Result<String, AgentError> {
        let tree = get_tree(&state.db, &self.context.user_id, true).await?;
        let object_name_key = ObjectNameKey::new(object_name);

        let branch = tree.iter().find(|it| {
            it.own
                && it.node_type == crate::db::NodeType::Branch
                && it
                    .name
                    .as_deref()
                    .map(|name| ObjectNameKey::new(name).matches(&object_name_key))
                    .unwrap_or(false)
        });

        if branch.is_none() {
            tracing::warn!("ObjectIdFinder: object with name {} not found", object_name);
            return Err(AgentError::ObjectNotFound {
                id: object_name.to_string(),
            });
        }
        let uuid = branch.unwrap().id;
        Ok(uuid.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObjectNameKey {
    lowercase: String,
    ascii_folded: String,
    german_transliterated: String,
}

impl ObjectNameKey {
    fn new(value: &str) -> Self {
        let lowercase = value.to_lowercase();
        Self {
            ascii_folded: fold_german_umlauts(&lowercase, false),
            german_transliterated: fold_german_umlauts(&lowercase, true),
            lowercase,
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.lowercase == other.lowercase
            || self.ascii_folded == other.ascii_folded
            || self.german_transliterated == other.german_transliterated
    }
}

fn fold_german_umlauts(value: &str, transliterate: bool) -> String {
    let mut folded = String::with_capacity(value.len());

    for ch in value.chars() {
        match ch {
            'ä' if transliterate => folded.push_str("ae"),
            'ö' if transliterate => folded.push_str("oe"),
            'ü' if transliterate => folded.push_str("ue"),
            'ä' => folded.push('a'),
            'ö' => folded.push('o'),
            'ü' => folded.push('u'),
            'ß' => folded.push_str("ss"),
            _ => folded.push(ch),
        }
    }

    folded
}

#[cfg(test)]
mod tests {
    use super::ObjectNameKey;

    #[test]
    fn object_name_key_matches_german_umlaut_forms() {
        let canonical = ObjectNameKey::new("EG - Küche");

        assert!(canonical.matches(&ObjectNameKey::new("EG - Küche")));
        assert!(canonical.matches(&ObjectNameKey::new("eg - kuche")));
        assert!(canonical.matches(&ObjectNameKey::new("EG - Kueche")));
    }
}

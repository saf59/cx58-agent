use std::sync::Arc;
use tera::{Context, Tera};
use anyhow::Result;

pub struct TemplateManager {
    lang_manager: Arc<crate::localization::LocalizationManager>,
}

impl TemplateManager {
    pub fn new(lang_manager: Arc<crate::localization::LocalizationManager>) -> Self {
        Self { lang_manager }
    }

    pub fn render(&self, lang: &str, msg_id: &str, context: Context) -> Result<String> {
        let template = self.lang_manager.get_msg(lang, msg_id);
        let text = Tera::one_off(&template, &context, false)?;
        Ok(text)
    }
}
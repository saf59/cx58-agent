use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use std::collections::HashMap;
use unic_langid::LanguageIdentifier;

pub struct TextManager {
    bundles: HashMap<String, FluentBundle<FluentResource>>,
}
impl Default for TextManager {
    fn default() -> Self {
        Self::new()
    }
}
impl TextManager {
    pub fn new() -> Self {
        let mut bundles = HashMap::new();

        // languages
        let locales = vec![
            ("en", include_str!("../../locales/en.ftl")),
            ("de", include_str!("../../locales/de.ftl")),
        ];

        for (lang_code, content) in locales {
            let res = FluentResource::try_new(content.to_string()).expect("Error parsing FTL file");

            let lang_id: LanguageIdentifier = lang_code.parse().expect("Wrong language ID");
            let mut bundle = FluentBundle::new(vec![lang_id]);
            bundle
                .add_resource(res)
                .expect("Failed to add resource to bundle");

            bundles.insert(lang_code.to_string(), bundle);
        }

        Self { bundles }
    }

    pub fn get_msg(&self, lang: &str, msg_id: &str) -> String {
        self.get_msg_with_args(lang, msg_id, FluentArgs::new())
    }
    pub fn split_msg(&self, lang: &str, msg_id: &str) -> Vec<String> {
        self.get_msg_with_args(lang, msg_id, FluentArgs::new())
            .split_whitespace().map(String::from).collect()
    }
    pub fn get_msg1(&self, lang: &str, msg_id: &str, param1: &str) -> String {
        let mut args = FluentArgs::new();
        args.set("p1", param1);
        self.get_msg_with_args(lang, msg_id, args)
    }
    pub fn get_msg2(&self, lang: &str, msg_id: &str, param1: &str, param2: &str) -> String {
        let mut args = FluentArgs::new();
        args.set("p1", param1);
        args.set("p2", param2);
        self.get_msg_with_args(lang, msg_id, args)
    }
    pub fn get_msg3(
        &self,
        lang: &str,
        msg_id: &str,
        param1: &str,
        param2: &str,
        param3: &str,
    ) -> String {
        let mut args = FluentArgs::new();
        args.set("p1", param1);
        args.set("p2", param2);
        args.set("p3", param3);
        self.get_msg_with_args(lang, msg_id, args)
    }
    /// Builds a prompt string for a specific language and parameters
    pub fn get_msg_with_args(&self, lang: &str, msg_id: &str, args: FluentArgs) -> String {
        let bundle = self
            .bundles
            .get(lang)
            .or_else(|| self.bundles.get("en")) // Fallback to English
            .expect("Language not found");

        let msg = bundle
            .get_message(msg_id)
            .unwrap_or_else(|| panic!("Message '{}' not found in FTL", msg_id));
        //.expect(&format!("Message '{}' not found in FTL", msg_id));

        let pattern = msg.value().expect("Message value is empty");
        let mut errors = vec![];

        bundle
            .format_pattern(pattern, Some(&args), &mut errors)
            .to_string()
    }
}

// src/localization/mod.rs

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

const PROMPT_FILES: &[(&str, &str)] = &[
    ("intent-router-system-prompt", "intent_router_system.txt"),
    ("orchestrator-system-prompt", "orchestrator_system.txt"),
    ("comparison-system-prompt", "comparison_system.txt"),
    ("formatter-system-prompt", "formatter_system.txt"),
    ("description-system-prompt", "descriptor_system.txt"),
    ("chat-system-prompt", "chat-system-prompt.txt"),
];

/// Internal request sent to localization worker thread
enum Request {
    Message {
        lang: String,
        msg_id: String,
        args: Vec<(String, String)>,
        resp: mpsc::Sender<String>,
    },
    Prompt {
        lang: String,
        prompt_id: String,
        resp: mpsc::Sender<Result<String>>,
    },
}

/// Thread-safe frontend. Fluent never leaves worker thread.
pub struct LocalizationManager {
    sender: mpsc::Sender<Request>,
}

impl Default for LocalizationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalizationManager {

    /// Creates manager and spawns dedicated worker thread.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Request>();

        thread::spawn(move || {
            let mut bundles: HashMap<String, FluentBundle<FluentResource>> =
                HashMap::new();
            let mut prompts: HashMap<String, HashMap<String, String>> =
                HashMap::new();

            // Load languages
            load_language(&mut bundles, "en", include_str!("../locales/en/messages.ftl"));
            load_language(&mut bundles, "de", include_str!("../locales/de/messages.ftl"));

            // Load prompts
            load_prompts(&mut prompts, "en");
            load_prompts(&mut prompts, "de");

            // Worker loop
            while let Ok(req) = rx.recv() {
                match req {
                    Request::Message { lang, msg_id, args, resp } => {
                        let result = format_message(&bundles, &lang, &msg_id, args);
                        let _ = resp.send(result);
                    }
                    Request::Prompt { lang, prompt_id, resp } => {
                        let result = get_prompt_internal(&prompts, &lang, &prompt_id);
                        let _ = resp.send(result);
                    }
                }
            }
        });

        Self { sender: tx }
    }

    /// Returns localized message without parameters.
    pub fn get_msg(&self, lang: &str, msg_id: &str) -> String {
        self.send_message(lang, msg_id, vec![])
    }

    /// Splits localized message into words.
    pub fn split_msg(&self, lang: &str, msg_id: &str) -> Vec<String> {
        self.get_msg(lang, msg_id)
            .split_whitespace()
            .map(String::from)
            .collect()
    }
    /// Convenience wrapper for the common case of a single variable substitution.
    ///
    /// Equivalent to calling `get_msg_with_args` with a one-entry `FluentArgs`.
    ///
    /// # Example
    /// ```
    /// // messages.ftl:  context-not-set = not set
    /// // messages.ftl:  progress-executing-worker = Executing { $worker }...
    /// let msg = lang_manager.get_msg_with_arg("en", "progress-executing-worker", "worker", "DescribeReport");
    /// ```
    pub fn get_msg_with_arg(
        &self,
        lang: &str,
        msg_id: &str,
        key: &str,
        value: &str,
    ) -> String {
        let mut args  = FluentArgs::new();
        args.set(key, value);
        self.get_msg_with_args(lang, msg_id, args)
    }
    /// Formats message with provided FluentArgs.
    pub fn get_msg_with_args(
        &self,
        lang: &str,
        msg_id: &str,
        args: FluentArgs,
    ) -> String {
        let converted: Vec<(String, String)> =
            args.iter().map(|(k, v)| (k.to_string(), Self::fluent_value_to_string(v))).collect();

        self.send_message(lang, msg_id, converted)
    }

    /// Returns prompt for specified language.
    ///
    /// Returns `Err` instead of panicking when the worker thread has stopped.
    pub fn get_prompt(&self, lang: &str, prompt_id: &str) -> Result<String> {
        let (resp_tx, resp_rx) = mpsc::channel();

        if self
            .sender
            .send(Request::Prompt {
                lang: lang.to_string(),
                prompt_id: prompt_id.to_string(),
                resp: resp_tx,
            })
            .is_err()
        {
            tracing::error!(
                "LocalizationManager: worker thread stopped, cannot resolve prompt='{}'",
                prompt_id
            );
            return Err(anyhow::anyhow!(
                "Localization worker stopped while fetching prompt '{}'",
                prompt_id
            ));
        }

        resp_rx.recv().unwrap_or_else(|_| {
            Err(anyhow::anyhow!("Prompt worker channel closed"))
        })
    }

    /// Internal helper for sending message request.
    ///
    /// Returns a safe fallback string instead of panicking when the worker
    /// thread has stopped (e.g. due to a startup error in load_language).
    fn send_message(
        &self,
        lang: &str,
        msg_id: &str,
        args: Vec<(String, String)>,
    ) -> String {
        let (resp_tx, resp_rx) = mpsc::channel();

        if self
            .sender
            .send(Request::Message {
                lang: lang.to_string(),
                msg_id: msg_id.to_string(),
                args,
                resp: resp_tx,
            })
            .is_err()
        {
            // Worker thread is gone — log and return a visible but safe placeholder.
            tracing::error!(
                "LocalizationManager: worker thread stopped, cannot resolve msg='{}'",
                msg_id
            );
            return format!("[{}]", msg_id);
        }

        resp_rx.recv().unwrap_or_else(|_| format!("[{}]", msg_id))
    }
    fn fluent_value_to_string(v: &fluent_bundle::FluentValue) -> String {
        match v {
            fluent_bundle::FluentValue::String(s) => s.to_string(),
            fluent_bundle::FluentValue::Number(n) => n.value.to_string(),
            _ => String::new(),
        }
    }

}

/// Loads FTL bundle inside worker thread.
///
/// Logs errors instead of panicking so the worker thread stays alive
/// even if one language file has a syntax error.
fn load_language(
    bundles: &mut HashMap<String, FluentBundle<FluentResource>>,
    lang: &str,
    content: &str,
) {
    let lang_id: LanguageIdentifier = match lang.parse() {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("LocalizationManager: invalid lang id '{}': {}", lang, e);
            return;
        }
    };

    let resource = match FluentResource::try_new(content.to_string()) {
        Ok(r) => r,
        Err((r, errors)) => {
            // try_new returns the partially-parsed resource alongside errors;
            // log them but continue with whatever was parsed successfully.
            for e in &errors {
                tracing::error!(
                    "LocalizationManager: FTL parse error in lang '{}': {:?}",
                    lang, e
                );
            }
            r
        }
    };

    let mut bundle = FluentBundle::new(vec![lang_id]);

    if let Err(errors) = bundle.add_resource(resource) {
        for e in &errors {
            tracing::error!(
                "LocalizationManager: duplicate message in lang '{}': {:?}",
                lang, e
            );
        }
        // Still insert the bundle — duplicate messages are non-fatal.
    }

    bundles.insert(lang.to_string(), bundle);
}

/// Loads prompt files inside worker thread.
fn load_prompts(
    prompts: &mut HashMap<String, HashMap<String, String>>,
    lang: &str,
) {
    let mut lang_prompts = HashMap::new();

    for (prompt_id, filename) in PROMPT_FILES {
        if let Ok(content) = load_prompt_file(lang, filename) {
            lang_prompts.insert(prompt_id.to_string(), content);
        }
    }

    prompts.insert(lang.to_string(), lang_prompts);
}

/// Loads prompt file via include_str mapping.
fn load_prompt_file(lang: &str, filename: &str) -> Result<String> {
    let content = match (lang, filename) {
        ("en", "intent_router_system.txt") => include_str!("../locales/en/prompts/intent_router_system.txt"),
        ("en", "orchestrator_system.txt") => include_str!("../locales/en/prompts/orchestrator_system.txt"),
        ("en", "comparison_system.txt") => include_str!("../locales/en/prompts/comparison_system.txt"),
        ("en", "formatter_system.txt") => include_str!("../locales/en/prompts/formatter_system.txt"),
        ("en", "descriptor_system.txt") => include_str!("../locales/en/prompts/descriptor_system.txt"),
        ("en", "chat-system-prompt.txt") => include_str!("../locales/en/prompts/chat-system-prompt.txt"),

        ("de", "intent_router_system.txt") => include_str!("../locales/de/prompts/intent_router_system.txt"),
        ("de", "orchestrator_system.txt") => include_str!("../locales/de/prompts/orchestrator_system.txt"),
        ("de", "comparison_system.txt") => include_str!("../locales/de/prompts/comparison_system.txt"),
        ("de", "formatter_system.txt") => include_str!("../locales/de/prompts/formatter_system.txt"),
        ("de", "descriptor_system.txt") => include_str!("../locales/de/prompts/descriptor_system.txt"),
        ("de", "chat-system-prompt.txt") => include_str!("../locales/de/prompts/chat-system-prompt.txt"),

        _ => return Err(anyhow::anyhow!("Unknown prompt")),
    };

    Ok(content.to_string())
}

/// Formats message inside worker thread.
pub fn format_message(
    bundles: &HashMap<String, FluentBundle<FluentResource>>,
    lang: &str,
    msg_id: &str,
    args: Vec<(String, String)>,
) -> String {
    let bundle = match bundles.get(lang).or_else(|| bundles.get("en")) {
        Some(b) => b,
        None => {
            tracing::error!(
                "LocalizationManager: no bundle for lang '{}' and no 'en' fallback",
                lang
            );
            return format!("[{}]", msg_id);
        }
    };

    let msg = match bundle.get_message(msg_id) {
        Some(m) => m,
        None => return format!("Missing message: {}", msg_id),
    };

    let pattern = match msg.value() {
        Some(p) => p,
        None => return format!("Empty message: {}", msg_id),
    };

    let mut errors = vec![];

    let mut fluent_args = FluentArgs::new();
    for (k, v) in args {
        fluent_args.set(k, v);
    }

    bundle
        .format_pattern(pattern, Some(&fluent_args), &mut errors)
        .to_string()
}

/// Retrieves prompt inside worker thread.
fn get_prompt_internal(
    prompts: &HashMap<String, HashMap<String, String>>,
    lang: &str,
    prompt_id: &str,
) -> Result<String> {
    prompts
        .get(lang)
        .or_else(|| prompts.get("en"))
        .and_then(|map| map.get(prompt_id))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Prompt not found"))
}

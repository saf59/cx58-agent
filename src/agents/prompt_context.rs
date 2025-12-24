use aho_corasick::{AhoCorasick, Match, MatchKind};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use strum_macros::{EnumIter, IntoStaticStr};
use thiserror::Error;
use crate::agents::TextManager;

#[derive(Error, Debug)]
pub enum ParserError {
    #[error("Failed to build Aho-Corasick automaton: {0}")]
    AhoCorasickBuild(String),

    #[error("Failed to parse amount number: {0}")]
    AmountParsing(#[from] std::num::ParseIntError),

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Invalid pattern configuration for key: {0}")]
    InvalidPattern(String),
}

#[derive(Debug, EnumIter, IntoStaticStr, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Period {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

#[derive(Debug, EnumIter, IntoStaticStr, Copy, Clone, PartialEq, Eq, Hash)]
pub enum PromptKey {
    Object,
    Document,
    Description,
    Comparison,
    Last,
    New,
    All,
    Period,
    Amount,
}

#[derive(Debug, Default, PartialEq)]
pub struct PromptContext {
    pub keys: Vec<PromptKey>,
    pub period: Option<Period>,
    pub amount: Option<usize>,
}

impl PromptContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn keys(&self) -> &[PromptKey] {
        &self.keys
    }

    pub fn period(&self) -> Option<Period> {
        self.period
    }

    pub fn amount(&self) -> Option<usize> {
        self.amount
    }

    pub fn has_key(&self, key: PromptKey) -> bool {
        self.keys.contains(&key)
    }

    fn add_key(&mut self, key: PromptKey) {
        if !self.keys.contains(&key) {
            self.keys.push(key);
        }
    }

    fn set_amount(&mut self, num: usize) {
        self.amount = Some(num);
    }

    fn set_period(&mut self, period: Period) {
        self.period = Some(period);
    }
}

pub struct ContextParser {}

impl ContextParser {
    pub fn new() -> Self {
        Self {}
    }

    /// Extracts context from prompt
    pub fn parse(&mut self, lang: &str, prompt: &str) -> Result<PromptContext, ParserError> {
        let mut context = PromptContext::new();
        let text_manager = TextManager::new();

        for key in PromptKey::iter() {
            match key {
                PromptKey::Amount => {
                    Self::parse_amount(prompt, &mut context, lang, &text_manager)?;
                }
                PromptKey::Period => {
                    Self::parse_period(prompt, &mut context, lang, &text_manager)?;
                }
                _ => {
                    Self::parse_generic_key(key, prompt, &mut context, lang, &text_manager)?;
                }
            }
        }

        Ok(context)
    }

    fn parse_amount(
        prompt: &str,
        context: &mut PromptContext,
        lang: &str,
        text_manager: &TextManager,
    ) -> Result<(), ParserError> {
        // amount_num must be present
        let num_patterns = text_manager.split_msg(lang, "amount_num");

        // Parse numeric values from patterns
        let nums: Vec<usize> = num_patterns
            .iter()
            .map(|s| s.parse::<usize>())
            .collect::<Result<Vec<usize>, _>>()?;

        let pattern_refs: Vec<&str> = num_patterns.iter().map(|s| s.as_str()).collect();

        // Find numeric patterns
        if let Some(found) = aho_parse(&pattern_refs, prompt)? {
            let num = nums[found.pattern().as_usize()];
            context.set_amount(num);
            return Ok(());
        }

        // Text patterns
        let text_patterns = text_manager.split_msg(lang, "amount_text");
        let pattern_refs: Vec<&str> = text_patterns.iter().map(|s| s.as_str()).collect();

        if let Some(found) = aho_parse(&pattern_refs, prompt)? {
            let index = found.pattern().as_usize();
            // Use the same numbers from amount_num by index
            let num = nums.get(index).copied().unwrap_or(index + 1);
            context.set_amount(num);
        }

        Ok(())
    }

    fn parse_period(
        prompt: &str,
        context: &mut PromptContext,
        lang: &str,
        text_manager: &TextManager,
    ) -> Result<(), ParserError> {
        let patterns = text_manager.split_msg(lang, "period-words");
        let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();

        if let Some(found) = aho_parse(&pattern_refs, prompt)? {
            let periods: Vec<Period> = Period::iter().collect();
            let index = found.pattern().as_usize();

            if index < periods.len() {
                context.set_period(periods[index]);
            }
        }

        Ok(())
    }

    fn parse_generic_key(
        key: PromptKey,
        prompt: &str,
        context: &mut PromptContext,
        lang: &str,
        text_manager: &TextManager,
    ) -> Result<(), ParserError> {
        let key_str: &'static str = key.into();
        let key_lower = format!("{}-words", key_str.to_lowercase());
        let patterns = text_manager.split_msg(lang, &key_lower);

        let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();

        if aho_parse(&pattern_refs, prompt)?.is_some() {
            context.add_key(key);
        }

        Ok(())
    }

}

impl Default for ContextParser {
    fn default() -> Self {
        Self::new()
    }
}

fn aho_parse(patterns: &[&str], haystack: &str) -> Result<Option<Match>, ParserError> {
    if patterns.is_empty() {
        return Ok(None);
    }

    let ac = AhoCorasick::builder()
        .match_kind(MatchKind::LeftmostFirst)
        .build(patterns)
        .map_err(|e| ParserError::AhoCorasickBuild(e.to_string()))?;

    Ok(ac.find(haystack))
}

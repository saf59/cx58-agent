use crate::agents::{ParserError, Period, PromptContext, PromptKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Task {
    Object { parameters: TaskParameters },
    Document { parameters: TaskParameters },
    Description { parameters: TaskParameters },
    Comparison { parameters: TaskParameters },
    Chat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskParameters {
    pub last: bool,
    pub all: bool,
    pub period: Option<Period>,
    pub amount: Option<usize>,
}

pub struct TaskDetector;

impl Default for TaskDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskDetector {
    pub fn new() -> Self {
        Self
    }

    pub fn detect_task(
        &self,
        prompt_context: &PromptContext,
        _prompt: &str,
    ) -> Result<Task, ParserError> {
        let parameters = self.build_parameters(prompt_context);

        // Priority order: Comparison > Description > Document > Object > Chat

        if prompt_context.keys.contains(&PromptKey::Comparison) {
            return Ok(Task::Comparison { parameters });
        }

        if prompt_context.keys.contains(&PromptKey::Description) {
            return Ok(Task::Description { parameters });
        }

        if prompt_context.keys.contains(&PromptKey::Document) {
            return Ok(Task::Document { parameters });
        }

        if prompt_context.keys.contains(&PromptKey::Object) {
            return Ok(Task::Object { parameters });
        }

        // Default to Chat if no specific task detected
        Ok(Task::Chat)
    }

    fn build_parameters(&self, context: &PromptContext) -> TaskParameters {
        TaskParameters {
            last: context.keys.contains(&PromptKey::Last),
            all: context.keys.contains(&PromptKey::All),
            period: context.period,
            amount: context.amount,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{ContextParser, Period};

    #[test]
    fn test_detect_object_task() {
        let mut parser = ContextParser::new();
        let context = parser.parse("en", "show last object").unwrap();

        let detector = TaskDetector::new();
        let task = detector.detect_task(&context, "show last object").unwrap();

        match task {
            Task::Object { parameters } => {
                assert!(parameters.last);
            }
            _ => panic!("Expected Object task"),
        }
    }

    #[test]
    fn test_detect_document_task() {
        let mut parser = ContextParser::new();
        let context = parser
            .parse("en", "get all documents for this month")
            .unwrap();

        let detector = TaskDetector::new();
        let task = detector
            .detect_task(&context, "get all documents for this month")
            .unwrap();

        match task {
            Task::Document { parameters } => {
                assert!(parameters.all);
                assert_eq!(parameters.period, Some(Period::Month));
            }
            _ => panic!("Expected Document task"),
        }
    }

    #[test]
    fn test_detect_comparison_task() {
        let mut parser = ContextParser::new();
        let context = parser.parse("en", "compare objects").unwrap();

        let detector = TaskDetector::new();
        let task = detector.detect_task(&context, "compare objects").unwrap();

        match task {
            Task::Comparison { .. } => {}
            _ => panic!("Expected Comparison task"),
        }
    }

    #[test]
    fn test_detect_chat_task() {
        let mut parser = ContextParser::new();
        let context = parser.parse("en", "hello how are you").unwrap();

        let detector = TaskDetector::new();
        let task = detector.detect_task(&context, "hello how are you").unwrap();

        match task {
            Task::Chat => {}
            _ => panic!("Expected Chat task"),
        }
    }
}

# comment no Tabs, please

describe-yourself = You are a helpful assistant.
  User message: {$p1}
  Say your name, then answer!

which-task-for-you = Which of these tasks are you suitable for?

three-qwestions = I need your help with three types of tasks!
  1. Understanding what's in the image.
  2. Working with tools.
  3. Thinking.

object-words = build construct object create make
document-words = picture image video report document file
description-words = describe modification alteration
comparison-words = compar differ detect update change
last-words = last previous recent
new-words = new latest
all-words = all every entire complete
period-words = day week month quarter year
amount_num = 1 2 3 4 5 6 7 8 9 10
amount_text = one two three four five six seven eight nine ten

# -------------------------------

# Progress Messages
progress-analyzing = Analyzing your request...
progress-context-validation = Validating context...
progress-formatting = Formatting response...

# Context Request Messages
context-request-object-id = Which building or site would you like to work with?
context-request-current-report = Which photo report would you like to analyze?
context-request-previous-report = Which previous report would you like to compare with?
context-request-clarification = I'm not sure what you mean. Could you please clarify?

# UI Messages
status-not-set = NOT SET
status-set = '{$value}'
no-conversation-history = No previous conversation
no-worker-results = No workers executed yet
worker-result-summary = {$worker_type}: {$status} ({$execution_time}ms)

# Error Messages
error-serialization = Serialization error: {$error}
error-agent = Agent error: {$error}
error-classification = Failed to parse classification result: {$error}
error-classification-fallback = Failed to parse classification result: {$error}
error-unknown-decision = Unknown decision type: {$decision_type}
error-missing-field = Missing {$field} field
error-unknown-worker = Unknown worker type
error-unknown-context-field = Unknown context field
error-unknown-decision-type = Unknown decision type: {$decision_type}
error-empty-report-id = report_id cannot be empty

# Orchestrator Messages
orchestrator-cannot-process = Cannot process this request

# Response Formatter Messages
error-comparison-parse = Failed to parse comparison: {$error}

# General Messages
analyzing-query = Analyzing query
fetching-data = Fetching data
processing-results = Processing results

context-not-set = not_set
progress-executing-worker = Executing { $worker }...
progress-worker-finding-object   = Looking up the object in the project tree...
progress-worker-finding-reports  = Finding matching reports...
progress-worker-loading-tree     = Loading the object hierarchy...
progress-worker-loading-reports  = Loading the report list...
progress-worker-describing-report = Analyzing the report...
progress-worker-comparing-reports = Comparing the two reports...
progress-worker-searching-knowledge = Searching the project knowledge base...
# ------------------------------------------------------------
progress-downloading-image = Downloading '{ $report_type }' image...
progress-processing-image = Processing '{ $report_type }' image...
progress-generating-description = Generating '{ $report_type }' description...
progress-description-parse-warning = Description parse failed for '{ $report_type }', saved raw
progress-generate-err = Failed to generate description for '{ $report_type }'
processing = Processing...
unknown = Unknown

# ------------------------------------------------------------
# Info / status messages
# ------------------------------------------------------------

info-no-documents-found = No documents found matching the criteria.
info-out-of-scope = This query is outside the scope of the construction monitoring system.

# ------------------------------------------------------------
# AgentError messages
# Each key maps 1-to-1 to an AgentError variant via AgentError::fluent_key().
# Named variables (in curly braces) correspond to AgentError::fluent_args().
# ------------------------------------------------------------
error-cancelled = The request was cancelled.

error-missing-object-id =
    Object ID is required but was not provided.
    Please select an object and try again.

error-invalid-uuid =
    The value "{ $raw }" is not a valid identifier.
    Please try again or contact support.

error-object-not-found =
    Object "{ $id }" was not found.
    Please check the object ID and try again.

error-no-documents-found =
    No documents were found matching your criteria.
    Try a different time period or object.

error-llm-json-parse =
    The AI model returned an unexpected response and it could not be processed.
    Details: { $detail }

error-template-render =
    Failed to render template "{ $template }".
    Please contact support if this problem persists.

error-localization-key-missing =
    Missing translation key: { $key }.
    Please report this to support.

error-date-parse =
    The date "{ $raw }" could not be parsed.
    Please use the format DD.MM.YYYY HH:MM:SS.

error-storage =
    A storage operation failed: { $detail }.
    Please try again later.

error-insufficient-descriptions =
    A comparison requires at least 2 report descriptions, but only { $found } were available.

error-internal =
    An internal error occurred: { $detail }
    Please try again or contact support.

# Context request — object selection
context-request-select-object = Please select an object from the tree to continue.
context-request-select-object-hint = Open the object tree and tap the item you want to work with.

# Context request — current report selection (DescribeReport, no reports in context)
context-request-select-report = Please select a report to describe.
context-request-select-report-hint = Choose a report from the list to view its description.

# Context request — previous report selection (CompareReports, both IDs absent)
context-request-select-previous-report = Please select two reports to compare.
context-request-select-previous-report-hint = Select an earlier and a later report from the list.

# Context request — second report missing (CompareReports, previous_report_id absent)
context-request-select-second-report = A second report is needed to run a comparison.
context-request-select-second-report-hint = Please select another report to compare with the current one.

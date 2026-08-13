// @agent-context: Deterministic per-turn tool-step admission policy.
//
// The policy sits in front of dispatch. It prevents repeated model tool loops,
// reuses the already-bounded model projection for equivalent calls, and stops
// searching a source after two zero-result responses.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};

const ZERO_RESULT_EXHAUSTION_LIMIT: u32 = 2;
const STEP_LIMIT_REASON: &str = "Tool step limit reached for this turn.";
const SAME_TOOL_LIMIT_REASON: &str = "Per-tool call limit reached for this turn.";
const SEARCH_EXHAUSTED_REASON: &str = "Search source exhausted after two zero-result responses.";
pub const DESTRUCTIVE_INTENT_REFUSAL: &str = "I can’t help delete, remove, overwrite, or otherwise destroy skills or coding-agent data through chat. Skill Deck’s agent is read-only. Use an explicit, confirmed app control or manage the files yourself after making a backup.";

const DESTRUCTIVE_VERBS: &[&str] = &[
    "delete",
    "deletes",
    "deleted",
    "deleting",
    "destroy",
    "destroys",
    "destroyed",
    "destroying",
    "erase",
    "erases",
    "erased",
    "erasing",
    "overwrite",
    "overwrites",
    "overwritten",
    "overwriting",
    "purge",
    "purges",
    "purged",
    "purging",
    "remove",
    "removes",
    "removed",
    "removing",
    "uninstall",
    "uninstalls",
    "uninstalled",
    "uninstalling",
    "wipe",
    "wipes",
    "wiped",
    "wiping",
];

const PROTECTED_TARGETS: &[&str] = &[
    "agent",
    "agents",
    "command",
    "commands",
    "config",
    "configs",
    "configuration",
    "data",
    "directory",
    "directories",
    "file",
    "files",
    "folder",
    "folders",
    "history",
    "hook",
    "hooks",
    "prompt",
    "prompts",
    "rule",
    "rules",
    "session",
    "sessions",
    "skill",
    "skills",
    "workflow",
    "workflows",
];

const UI_STATE_TARGETS: &[&str] = &[
    "card",
    "cards",
    "favorite",
    "favorites",
    "favourite",
    "favourites",
    "filter",
    "filters",
    "list",
    "lists",
    "query",
    "queries",
    "result",
    "results",
    "search",
    "selection",
    "selections",
    "tag",
    "tags",
    "view",
    "views",
];

/// App-level admission policy for chat input.
///
/// Skill Deck's agent is a discovery surface, not a filesystem executor. A
/// request for destructive advice is refused before provider startup, scanning,
/// or tool routing. Explicit negative statements ("do not delete my skills")
/// and requests that only alter UI state ("remove the active skills filter")
/// are not classified as destructive requests.
pub fn destructive_intent_refusal(user_text: &str) -> Option<&'static str> {
    let tokens = normalized_words(user_text);
    let has_target = tokens
        .iter()
        .any(|token| PROTECTED_TARGETS.contains(&token.as_str()));
    if !has_target {
        return None;
    }

    let destructive = tokens.iter().enumerate().any(|(index, token)| {
        DESTRUCTIVE_VERBS.contains(&token.as_str())
            && !verb_is_negated(&tokens, index)
            && !verb_targets_ui_state(&tokens, index)
    });
    destructive.then_some(DESTRUCTIVE_INTENT_REFUSAL)
}

fn normalized_words(text: &str) -> Vec<String> {
    text.chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn verb_is_negated(tokens: &[String], verb_index: usize) -> bool {
    let start = verb_index.saturating_sub(3);
    tokens[start..verb_index].iter().any(|token| {
        matches!(
            token.as_str(),
            "don" | "dont" | "never" | "no" | "not" | "without"
        )
    })
}

/// Distinguish non-destructive collection/view changes from artifact deletion.
///
/// Two ordinary UI constructions are recognized:
/// - the UI noun is the object ("remove the active skills filter");
/// - an item is removed *from* a UI collection ("remove skill from favorites").
///
/// A concrete filesystem noun anywhere in the phrase keeps the request on the
/// refusal path, even when a UI word is also present.
fn verb_targets_ui_state(tokens: &[String], verb_index: usize) -> bool {
    const FILESYSTEM_TARGETS: &[&str] = &[
        "data",
        "directory",
        "directories",
        "file",
        "files",
        "folder",
        "folders",
        "history",
        "session",
        "sessions",
    ];

    if tokens
        .iter()
        .any(|token| FILESYSTEM_TARGETS.contains(&token.as_str()))
    {
        return false;
    }

    let object_window_end = (verb_index + 6).min(tokens.len());
    let object_window = &tokens[verb_index + 1..object_window_end];
    let ui_position = object_window
        .iter()
        .position(|token| UI_STATE_TARGETS.contains(&token.as_str()));

    let Some(ui_position) = ui_position else {
        return false;
    };

    let before_ui = &object_window[..ui_position];
    before_ui.is_empty()
        || before_ui.iter().any(|token| token == "from")
        || before_ui.iter().all(|token| {
            PROTECTED_TARGETS.contains(&token.as_str())
                || matches!(
                    token.as_str(),
                    "a" | "active"
                        | "all"
                        | "an"
                        | "any"
                        | "current"
                        | "my"
                        | "selected"
                        | "the"
                        | "this"
                        | "these"
                )
        })
}

/// Stable identity for a normalized tool call.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallFingerprint(String);

impl ToolCallFingerprint {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ToolCallFingerprint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Decision returned before a tool is dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepDecision {
    Execute {
        fingerprint: ToolCallFingerprint,
    },
    Reuse {
        fingerprint: ToolCallFingerprint,
        bounded_projection: String,
    },
    Veto {
        fingerprint: ToolCallFingerprint,
        reason: &'static str,
    },
}

impl StepDecision {
    pub fn fingerprint(&self) -> &ToolCallFingerprint {
        match self {
            Self::Execute { fingerprint }
            | Self::Reuse { fingerprint, .. }
            | Self::Veto { fingerprint, .. } => fingerprint,
        }
    }
}

#[derive(Debug, Clone)]
struct PriorResult {
    bounded_projection: String,
    zero_results: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SearchSource {
    Local,
    Marketplace,
    Web,
}

/// Stateful policy for one model turn.
///
/// Call `evaluate` before dispatch and `record_result` after a successful
/// dispatch has been projected into its bounded model-facing representation.
#[derive(Debug)]
pub struct StepPolicy {
    max_tool_steps: u32,
    max_same_tool_calls: u32,
    attempted_steps: u32,
    calls_by_tool: HashMap<String, u32>,
    zero_results_by_source: HashMap<SearchSource, u32>,
    tools_by_fingerprint: HashMap<ToolCallFingerprint, String>,
    prior_results: HashMap<ToolCallFingerprint, PriorResult>,
}

impl StepPolicy {
    pub fn new(max_tool_steps: u32, max_same_tool_calls: u32) -> Self {
        Self {
            max_tool_steps,
            max_same_tool_calls,
            attempted_steps: 0,
            calls_by_tool: HashMap::new(),
            zero_results_by_source: HashMap::new(),
            tools_by_fingerprint: HashMap::new(),
            prior_results: HashMap::new(),
        }
    }

    /// Evaluate a call without dispatching it.
    ///
    /// Execute and reuse decisions consume a step and a per-tool call slot.
    /// Vetoes do not consume additional budget. Counting reuse attempts is
    /// intentional: otherwise a model can loop forever on a cached response.
    pub fn evaluate(&mut self, tool_name: &str, args: &Value) -> StepDecision {
        let normalized_tool = normalize_tool_name(tool_name);
        let fingerprint = tool_call_fingerprint(&normalized_tool, args);

        if self.attempted_steps >= self.max_tool_steps {
            return StepDecision::Veto {
                fingerprint,
                reason: STEP_LIMIT_REASON,
            };
        }

        let prior_tool_calls = self
            .calls_by_tool
            .get(&normalized_tool)
            .copied()
            .unwrap_or_default();
        if prior_tool_calls >= self.max_same_tool_calls {
            return StepDecision::Veto {
                fingerprint,
                reason: SAME_TOOL_LIMIT_REASON,
            };
        }

        let search_source = search_source(&normalized_tool);
        if search_source.is_some_and(|source| self.search_source_exhausted(source)) {
            return StepDecision::Veto {
                fingerprint,
                reason: SEARCH_EXHAUSTED_REASON,
            };
        }

        self.attempted_steps = self.attempted_steps.saturating_add(1);
        self.calls_by_tool
            .insert(normalized_tool.clone(), prior_tool_calls.saturating_add(1));
        self.tools_by_fingerprint
            .insert(fingerprint.clone(), normalized_tool);

        if let Some(prior) = self.prior_results.get(&fingerprint).cloned() {
            return StepDecision::Reuse {
                fingerprint,
                bounded_projection: prior.bounded_projection,
            };
        }

        StepDecision::Execute { fingerprint }
    }

    /// Retain the bounded projection for reuse by an equivalent later call.
    ///
    /// Recording the same fingerprint again replaces its projection without
    /// double-counting its zero-result response.
    pub fn record_result(
        &mut self,
        fingerprint: &ToolCallFingerprint,
        bounded_projection: String,
        zero_results: bool,
    ) {
        let already_counted_zero = self
            .prior_results
            .get(fingerprint)
            .is_some_and(|prior| prior.zero_results);

        if zero_results && !already_counted_zero {
            if let Some(tool_name) = self.tool_name_for(fingerprint) {
                if let Some(source) = search_source(&tool_name) {
                    self.observe_zero_results(source);
                }
            }
        }

        self.prior_results.insert(
            fingerprint.clone(),
            PriorResult {
                bounded_projection,
                zero_results,
            },
        );
    }

    pub fn attempted_steps(&self) -> u32 {
        self.attempted_steps
    }

    pub fn calls_for(&self, tool_name: &str) -> u32 {
        self.calls_by_tool
            .get(&normalize_tool_name(tool_name))
            .copied()
            .unwrap_or_default()
    }

    fn tool_name_for(&self, fingerprint: &ToolCallFingerprint) -> Option<String> {
        self.tools_by_fingerprint.get(fingerprint).cloned()
    }

    fn search_source_exhausted(&self, source: SearchSource) -> bool {
        self.zero_results_by_source
            .get(&source)
            .copied()
            .unwrap_or_default()
            >= ZERO_RESULT_EXHAUSTION_LIMIT
    }

    fn observe_zero_results(&mut self, source: SearchSource) {
        let count = self.zero_results_by_source.entry(source).or_default();
        *count = count.saturating_add(1);
    }
}

/// Produce a deterministic fingerprint from a tool name and normalized JSON.
pub fn tool_call_fingerprint(tool_name: &str, args: &Value) -> ToolCallFingerprint {
    let normalized_tool = normalize_tool_name(tool_name);
    let normalized_args = normalized_tool_args(&normalized_tool, args);
    let serialized = serde_json::to_string(&normalized_args).unwrap_or_else(|_| "null".to_string());
    let digest = Sha256::digest(format!("{normalized_tool}\n{serialized}").as_bytes());
    ToolCallFingerprint(format!("sha256:{digest:x}"))
}

/// Canonicalize JSON recursively for argument-order independent comparison.
///
/// Search query terms are lowercased, punctuation/whitespace is collapsed,
/// repeated terms are removed, and term order is sorted. Explicit
/// `search_skills` defaults are omitted so they match absent defaults.
pub fn normalized_tool_args(tool_name: &str, args: &Value) -> Value {
    let normalized_tool = normalize_tool_name(tool_name);
    let mut normalized = normalize_json(args);

    if is_search_tool(&normalized_tool) {
        if let Some(object) = normalized.as_object_mut() {
            if let Some(query_value) = object.get_mut("query") {
                if let Some(query) = query_value.as_str() {
                    *query_value = Value::String(normalize_search_query(query));
                }
            }
        }
    }

    match normalized_tool.as_str() {
        "search_skills" => omit_search_skills_defaults(&mut normalized),
        "search_marketplace" => omit_search_marketplace_defaults(&mut normalized),
        "web_search" => omit_web_search_defaults(&mut normalized),
        _ => {}
    }

    normalized
}

/// Normalize a search query for semantic duplicate detection.
pub fn normalize_search_query(query: &str) -> String {
    let mut current = String::new();
    let mut terms = BTreeSet::new();

    for character in query.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            current.push(character);
        } else if !current.is_empty() {
            terms.insert(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        terms.insert(current);
    }

    terms.into_iter().collect::<Vec<_>>().join(" ")
}

fn normalize_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut normalized = Map::new();
            for key in keys {
                let child = &object[key];
                if !child.is_null() {
                    normalized.insert(key.clone(), normalize_json(child));
                }
            }
            Value::Object(normalized)
        }
        Value::Array(values) => Value::Array(values.iter().map(normalize_json).collect()),
        other => other.clone(),
    }
}

fn omit_search_skills_defaults(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let fields_are_default = object.get("fields").is_some_and(|fields| {
        fields
            .as_array()
            .is_some_and(|fields| fields.len() == 1 && fields[0].as_str() == Some("all"))
    });
    if fields_are_default {
        object.remove("fields");
    }
}

fn omit_search_marketplace_defaults(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    if object
        .get("provider")
        .and_then(Value::as_str)
        .is_some_and(|provider| provider.eq_ignore_ascii_case("all"))
    {
        object.remove("provider");
    }
    if object.get("limit").and_then(Value::as_u64) == Some(10) {
        object.remove("limit");
    }
    if object
        .get("mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("parallel"))
    {
        object.remove("mode");
    }
}

fn omit_web_search_defaults(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if object.get("limit").and_then(Value::as_u64) == Some(6) {
        object.remove("limit");
    }
}

fn normalize_tool_name(tool_name: &str) -> String {
    tool_name.trim().to_ascii_lowercase()
}

fn is_search_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "search_skills" | "search_marketplace" | "web_search"
    )
}

fn search_source(tool_name: &str) -> Option<SearchSource> {
    match tool_name {
        "search_skills" => Some(SearchSource::Local),
        "search_marketplace" => Some(SearchSource::Marketplace),
        "web_search" => Some(SearchSource::Web),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn execute(policy: &mut StepPolicy, tool: &str, args: Value) -> ToolCallFingerprint {
        match policy.evaluate(tool, &args) {
            StepDecision::Execute { fingerprint } => fingerprint,
            decision => panic!("expected execute, got {decision:?}"),
        }
    }

    #[test]
    fn screenshot_repeat_pattern_cannot_continue() {
        let mut policy = StepPolicy::new(6, 2);

        let local = execute(&mut policy, "search_skills", json!({"query": "rust"}));
        policy.record_result(&local, r#"{"items":[]}"#.into(), true);
        assert!(matches!(
            policy.evaluate("search_skills", &json!({"query": "RUST!"})),
            StepDecision::Reuse { .. }
        ));
        assert!(matches!(
            policy.evaluate("search_skills", &json!({"query": "rust async"})),
            StepDecision::Veto {
                reason: SAME_TOOL_LIMIT_REASON,
                ..
            }
        ));

        let marketplace = execute(
            &mut policy,
            "search_marketplace",
            json!({"query": "rust", "provider": "all"}),
        );
        policy.record_result(&marketplace, r#"{"items":[]}"#.into(), true);
        assert!(matches!(
            policy.evaluate(
                "search_marketplace",
                &json!({"provider": "all", "query": "rust rust"})
            ),
            StepDecision::Reuse { .. }
        ));
        assert!(matches!(
            policy.evaluate(
                "search_marketplace",
                &json!({"query": "rust testing", "provider": "all"})
            ),
            StepDecision::Veto {
                reason: SAME_TOOL_LIMIT_REASON,
                ..
            }
        ));
    }

    #[test]
    fn argument_order_is_equivalent() {
        let first = tool_call_fingerprint(
            "search_marketplace",
            &json!({"query": "rust", "provider": "all", "limit": 5}),
        );
        let second = tool_call_fingerprint(
            "search_marketplace",
            &json!({"limit": 5, "provider": "all", "query": "rust"}),
        );
        assert_eq!(first, second);
    }

    #[test]
    fn normalized_queries_and_explicit_search_skills_defaults_are_equivalent() {
        let first = tool_call_fingerprint(
            "search_skills",
            &json!({"query": " Rust, testing rust! ", "fields": ["all"]}),
        );
        let second = tool_call_fingerprint("search_skills", &json!({"query": "TESTING   rust"}));
        assert_eq!(first, second);
    }

    #[test]
    fn explicit_marketplace_and_web_search_defaults_equal_absent_defaults() {
        let marketplace_absent =
            tool_call_fingerprint("search_marketplace", &json!({"query": "rust"}));
        let marketplace_explicit = tool_call_fingerprint(
            "search_marketplace",
            &json!({
                "query": "rust",
                "provider": "ALL",
                "limit": 10,
                "mode": "PARALLEL"
            }),
        );
        assert_eq!(marketplace_absent, marketplace_explicit);

        let web_absent = tool_call_fingerprint("web_search", &json!({"query": "rust"}));
        let web_explicit =
            tool_call_fingerprint("web_search", &json!({"query": "rust", "limit": 6}));
        assert_eq!(web_absent, web_explicit);
    }

    #[test]
    fn two_zero_result_responses_exhaust_a_search_source() {
        let mut policy = StepPolicy::new(10, 10);

        let first = execute(&mut policy, "search_skills", json!({"query": "alpha"}));
        policy.record_result(&first, "first".into(), true);
        let second = execute(&mut policy, "search_skills", json!({"query": "beta"}));
        policy.record_result(&second, "second".into(), true);

        assert!(matches!(
            policy.evaluate("search_skills", &json!({"query": "gamma"})),
            StepDecision::Veto {
                reason: SEARCH_EXHAUSTED_REASON,
                ..
            }
        ));
        assert_eq!(policy.attempted_steps(), 2);
    }

    #[test]
    fn cached_zero_result_reuse_does_not_exhaust_source() {
        let mut policy = StepPolicy::new(10, 10);
        let first = execute(&mut policy, "search_skills", json!({"query": "alpha"}));
        policy.record_result(&first, "first".into(), true);

        assert!(matches!(
            policy.evaluate("search_skills", &json!({"query": "ALPHA!"})),
            StepDecision::Reuse { .. }
        ));

        let second = execute(&mut policy, "search_skills", json!({"query": "beta"}));
        policy.record_result(&second, "second".into(), true);
        assert!(matches!(
            policy.evaluate("search_skills", &json!({"query": "gamma"})),
            StepDecision::Veto {
                reason: SEARCH_EXHAUSTED_REASON,
                ..
            }
        ));
    }

    #[test]
    fn capped_steps_veto_without_consuming_more_budget() {
        let mut policy = StepPolicy::new(2, 10);
        execute(&mut policy, "get_skill_stats", json!({}));
        execute(
            &mut policy,
            "get_skill_detail",
            json!({"skill_id": "artifact:one"}),
        );

        assert!(matches!(
            policy.evaluate("list_skills", &json!({})),
            StepDecision::Veto {
                reason: STEP_LIMIT_REASON,
                ..
            }
        ));
        assert_eq!(policy.attempted_steps(), 2);
    }

    #[test]
    fn duplicate_call_reuses_the_prior_bounded_projection() {
        let mut policy = StepPolicy::new(6, 2);
        let fingerprint = execute(
            &mut policy,
            "search_marketplace",
            json!({"query": "Prompt, Engineering"}),
        );
        policy.record_result(&fingerprint, r#"{"bounded":true}"#.into(), false);

        match policy.evaluate(
            "search_marketplace",
            &json!({"query": "engineering prompt prompt"}),
        ) {
            StepDecision::Reuse {
                fingerprint: repeated,
                bounded_projection,
            } => {
                assert_eq!(repeated, fingerprint);
                assert_eq!(bounded_projection, r#"{"bounded":true}"#);
            }
            decision => panic!("expected reuse, got {decision:?}"),
        }
    }

    #[test]
    fn zero_limits_and_non_object_args_are_handled_without_panicking() {
        let mut blocked = StepPolicy::new(0, 0);
        assert!(matches!(
            blocked.evaluate("web_search", &json!(["rust", null])),
            StepDecision::Veto {
                reason: STEP_LIMIT_REASON,
                ..
            }
        ));
        assert_eq!(blocked.attempted_steps(), 0);

        let first = tool_call_fingerprint("web_search", &json!(["rust", null]));
        let second = tool_call_fingerprint("web_search", &json!(["rust", null]));
        assert_eq!(first, second);
    }

    #[test]
    fn jan_delete_advice_case_is_refused_before_tool_policy() {
        let refusal = destructive_intent_refusal(
            "Find the right skill and tell me how to delete every skill file from my agents.",
        );
        assert_eq!(refusal, Some(DESTRUCTIVE_INTENT_REFUSAL));
    }

    #[test]
    fn destructive_intent_covers_common_agent_artifacts() {
        for prompt in [
            "How do I remove this hook?",
            "Wipe the agent history.",
            "Overwrite all configuration files.",
            "Uninstall my coding-agent workflows.",
            "Purge these command folders.",
        ] {
            assert!(
                destructive_intent_refusal(prompt).is_some(),
                "must refuse: {prompt}"
            );
        }
    }

    #[test]
    fn negative_or_nondestructive_intent_is_not_refused() {
        for prompt in [
            "Do not delete my skills.",
            "Search skills without removing any files.",
            "Explain what a hook does.",
            "Remove duplicate words from this search query.",
            "Remove the active skills filter.",
            "Remove this skill from favorites.",
            "Remove selected skills from this list.",
            "Delete the skill card from this view.",
        ] {
            assert!(
                destructive_intent_refusal(prompt).is_none(),
                "must allow: {prompt}"
            );
        }
    }

    #[test]
    fn ui_words_do_not_hide_real_artifact_deletion() {
        for prompt in [
            "Remove the skill file from my favorites folder.",
            "Delete session history from this list.",
            "Uninstall this hook after opening the filtered view.",
        ] {
            assert!(
                destructive_intent_refusal(prompt).is_some(),
                "must refuse: {prompt}"
            );
        }
    }
}

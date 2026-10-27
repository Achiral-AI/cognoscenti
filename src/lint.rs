//! Dataset lint (Plan 25 prerequisite fix 3): catches an authoring mistake
//! before it reaches a scored run, not after.

use crate::stream::Stream;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// Exactly the two word lists the old `organic-memory` scorer
/// (`lifecycle_signal`, removed in Phase 0) rewarded or punished. A dataset
/// whose event content contains them can be gamed the same way that scorer
/// gamed `organizational_memory.jsonl`: the answer is read off the data
/// instead of found by retrieval.
pub const ANSWER_REVEALING_MARKERS: &[&str] = &[
    "validated",
    "current",
    "reinforced",
    "approved",
    "reused",
    "high-signal",
    "latest",
    "stale",
    "superseded",
    "deprecated",
    "outdated",
    "low-signal",
    "ignore",
    "historical only",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintIssue {
    pub item_id: String,
    pub message: String,
}

impl LintIssue {
    fn new(item_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            item_id: item_id.into(),
            message: message.into(),
        }
    }
}

/// Loads and lints a stream dataset file, printing every issue found.
/// Returns the issues so the caller decides how to fail (an empty `Vec`
/// means the dataset is clean).
pub fn lint_stream_file(path: &str) -> Result<Vec<LintIssue>> {
    let stream = Stream::load(path)?;
    let issues = lint(&stream);

    if issues.is_empty() {
        println!("Dataset lint: clean ({path})");
    } else {
        println!("Dataset lint: {} issue(s) in {path}", issues.len());
        for issue in &issues {
            println!("  {}: {}", issue.item_id, issue.message);
        }
    }

    Ok(issues)
}

pub fn lint(stream: &Stream) -> Vec<LintIssue> {
    let mut issues = Vec::new();
    check_unique_ids(stream, &mut issues);
    check_gold_references_exist(stream, &mut issues);
    check_no_forward_dated_gold(stream, &mut issues);
    check_supersedes(stream, &mut issues);
    check_chronological_order(stream, &mut issues);
    check_answer_revealing_markers(stream, &mut issues);
    issues
}

fn check_unique_ids(stream: &Stream, issues: &mut Vec<LintIssue>) {
    let mut seen_events = HashSet::new();
    for event in stream.events() {
        if !seen_events.insert(event.id.as_str()) {
            issues.push(LintIssue::new(&event.id, "duplicate event id"));
        }
    }

    let mut seen_questions = HashSet::new();
    for question in stream.questions() {
        if !seen_questions.insert(question.id.as_str()) {
            issues.push(LintIssue::new(&question.id, "duplicate question id"));
        }
    }
}

fn check_gold_references_exist(stream: &Stream, issues: &mut Vec<LintIssue>) {
    let event_ids: HashSet<&str> = stream.events().map(|event| event.id.as_str()).collect();
    for question in stream.questions() {
        for gold_id in &question.gold_event_ids {
            if !event_ids.contains(gold_id.as_str()) {
                issues.push(LintIssue::new(
                    &question.id,
                    format!("gold_event_ids references unknown event {gold_id}"),
                ));
            }
        }
    }
}

fn check_no_forward_dated_gold(stream: &Stream, issues: &mut Vec<LintIssue>) {
    let timestamps: HashMap<&str, _> = stream
        .events()
        .map(|event| (event.id.as_str(), event.timestamp))
        .collect();

    for question in stream.questions() {
        for gold_id in &question.gold_event_ids {
            if let Some(event_timestamp) = timestamps.get(gold_id.as_str()) {
                if *event_timestamp > question.asked_at {
                    issues.push(LintIssue::new(
                        &question.id,
                        format!(
                            "gold event {gold_id} is timestamped after the question that cites it \
                             - the question could not have known it yet"
                        ),
                    ));
                }
            }
        }
    }
}

fn check_supersedes(stream: &Stream, issues: &mut Vec<LintIssue>) {
    let events: HashMap<&str, _> = stream
        .events()
        .map(|event| (event.id.as_str(), event))
        .collect();

    for event in stream.events() {
        let Some(target_id) = &event.supersedes else {
            continue;
        };
        match events.get(target_id.as_str()) {
            None => issues.push(LintIssue::new(
                &event.id,
                format!("supersedes unknown event {target_id}"),
            )),
            Some(target) if target.timestamp >= event.timestamp => issues.push(LintIssue::new(
                &event.id,
                format!("supersedes {target_id}, which is not timestamped earlier"),
            )),
            Some(_) => {}
        }
    }
}

fn check_chronological_order(stream: &Stream, issues: &mut Vec<LintIssue>) {
    let mut previous: Option<(&str, chrono::DateTime<chrono::Utc>)> = None;
    for item in &stream.items {
        let timestamp = item.timestamp();
        if let Some((previous_id, previous_timestamp)) = previous {
            if timestamp < previous_timestamp {
                issues.push(LintIssue::new(
                    item.id(),
                    format!(
                        "out of chronological order: comes after {previous_id} in the file \
                         but is timestamped earlier"
                    ),
                ));
            }
        }
        previous = Some((item.id(), timestamp));
    }
}

fn check_answer_revealing_markers(stream: &Stream, issues: &mut Vec<LintIssue>) {
    for event in stream.events() {
        let lowered = event.content.to_ascii_lowercase();
        for marker in ANSWER_REVEALING_MARKERS {
            if lowered.contains(marker) {
                issues.push(LintIssue::new(
                    &event.id,
                    format!("content contains the answer-revealing marker word \"{marker}\""),
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, content: &str, ts: &str) -> String {
        format!(
            r#"{{"kind":"event","id":"{id}","content":"{content}","timestamp":"{ts}","actor":{{"id":"a","kind":"user"}},"scope":{{"kind":"org","id":"acme"}},"source":"slack"}}"#
        )
    }

    fn question(id: &str, gold: &[&str], ts: &str) -> String {
        let gold_json = gold
            .iter()
            .map(|g| format!("\"{g}\""))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"kind":"question","id":"{id}","query":"q","asked_at":"{ts}","asked_by":{{"id":"a","kind":"user"}},"gold_event_ids":[{gold_json}]}}"#
        )
    }

    #[test]
    fn a_clean_stream_has_no_issues() {
        let stream = Stream::from_jsonl(&format!(
            "{}\n{}\n{}",
            event(
                "e1",
                "Picked Postgres for the ledger",
                "2026-01-01T00:00:00Z"
            ),
            event("e2", "Switched to Cockroach", "2026-02-01T00:00:00Z"),
            question("q1", &["e2"], "2026-03-01T00:00:00Z"),
        ))
        .unwrap();

        assert_eq!(lint(&stream), Vec::new());
    }

    #[test]
    fn flags_duplicate_event_ids() {
        let stream = Stream::from_jsonl(&format!(
            "{}\n{}",
            event("e1", "first", "2026-01-01T00:00:00Z"),
            event("e1", "second", "2026-01-02T00:00:00Z"),
        ))
        .unwrap();

        let issues = lint(&stream);
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("duplicate event id")));
    }

    #[test]
    fn flags_a_gold_reference_to_a_missing_event() {
        let stream =
            Stream::from_jsonl(&question("q1", &["nonexistent"], "2026-01-01T00:00:00Z")).unwrap();

        let issues = lint(&stream);
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("references unknown event")));
    }

    #[test]
    fn flags_gold_that_the_question_could_not_have_known_yet() {
        let stream = Stream::from_jsonl(&format!(
            "{}\n{}",
            question("q1", &["e1"], "2026-01-01T00:00:00Z"),
            event("e1", "comes later in time", "2026-06-01T00:00:00Z"),
        ))
        .unwrap();

        let issues = lint(&stream);
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("could not have known it yet")));
    }

    #[test]
    fn flags_a_supersedes_reference_to_a_missing_event() {
        let json = r#"{"kind":"event","id":"e2","content":"x","timestamp":"2026-01-01T00:00:00Z","actor":{"id":"a","kind":"user"},"scope":{"kind":"org","id":"acme"},"source":"slack","supersedes":"ghost"}"#;
        let stream = Stream::from_jsonl(json).unwrap();

        let issues = lint(&stream);
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("supersedes unknown event")));
    }

    #[test]
    fn flags_a_supersedes_that_does_not_move_forward_in_time() {
        let e2 = r#"{"kind":"event","id":"e2","content":"x","timestamp":"2026-01-01T00:00:00Z","actor":{"id":"a","kind":"user"},"scope":{"kind":"org","id":"acme"},"source":"slack","supersedes":"e1"}"#;
        let stream = Stream::from_jsonl(&format!(
            "{}\n{}",
            event("e1", "first", "2026-02-01T00:00:00Z"),
            e2,
        ))
        .unwrap();

        let issues = lint(&stream);
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("not timestamped earlier")));
    }

    #[test]
    fn flags_a_stream_out_of_chronological_order() {
        let stream = Stream::from_jsonl(&format!(
            "{}\n{}",
            event("e1", "later in the file", "2026-02-01T00:00:00Z"),
            event("e2", "but earlier in time", "2026-01-01T00:00:00Z"),
        ))
        .unwrap();

        let issues = lint(&stream);
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("out of chronological order")));
    }

    #[test]
    fn flags_answer_revealing_marker_words() {
        let stream = Stream::from_jsonl(&event(
            "e1",
            "This fact is Validated and current",
            "2026-01-01T00:00:00Z",
        ))
        .unwrap();

        let issues = lint(&stream);
        assert!(issues
            .iter()
            .any(|issue| issue.message.contains("validated")));
        assert!(issues.iter().any(|issue| issue.message.contains("current")));
    }
}

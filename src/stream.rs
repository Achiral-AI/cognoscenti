//! The stream data model (Plan 25 prerequisite fix 3): a timestamped
//! sequence of events and questions, with gold answers tied to source event
//! IDs (not memory text) and optional validity windows for when a gold
//! answer is actually true. Events and questions interleave; fact changes
//! are events that mark an earlier event superseded, not a separate type.
//!
//! This module only defines and loads the schema. Nothing replays a stream
//! against a system yet - that is the runner, in a later phase.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Actor {
    pub id: String,
    pub kind: ActorKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Org,
    User,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scope {
    pub kind: ScopeKind,
    pub id: String,
}

/// One unit of recorded content. A fact change is an event whose
/// `supersedes` names the event it replaces - there is no separate
/// fact-change type, so superseding never needs a second, duplicate record
/// of the same fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    pub id: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub actor: Actor,
    pub scope: Scope,
    pub source: String,
    #[serde(default)]
    pub supersedes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidityWindow {
    pub from: DateTime<Utc>,
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
}

/// A question asked at a point in simulated time. Gold is tied to source
/// event IDs so a system that rewrites what it stores can still be scored
/// on whether it found the right evidence (Plan 25, Scoring).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamQuestion {
    pub id: String,
    pub query: String,
    pub asked_at: DateTime<Utc>,
    pub asked_by: Actor,
    pub gold_event_ids: Vec<String>,
    #[serde(default)]
    pub gold_answer: Option<String>,
    #[serde(default)]
    pub validity: Option<ValidityWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamItem {
    Event(StreamEvent),
    Question(StreamQuestion),
}

impl StreamItem {
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            StreamItem::Event(event) => event.timestamp,
            StreamItem::Question(question) => question.asked_at,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            StreamItem::Event(event) => &event.id,
            StreamItem::Question(question) => &question.id,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Stream {
    /// Preserves the authored (file) order; this is the stream's canonical
    /// chronological order, not auto-sorted on load. Replicate/ordering runs
    /// shuffle an explicit copy of this later - they never redefine it.
    pub items: Vec<StreamItem>,
}

impl Stream {
    pub fn load(path: &str) -> Result<Self> {
        let input =
            fs::read_to_string(path).with_context(|| format!("failed to read stream: {path}"))?;
        Self::from_jsonl(&input).with_context(|| format!("invalid stream file: {path}"))
    }

    /// One JSON object per non-blank, non-`#`-comment line, each tagged
    /// `"kind": "event" | "question"`.
    pub fn from_jsonl(input: &str) -> Result<Self> {
        let mut items = Vec::new();
        for (idx, line) in input.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let item: StreamItem = serde_json::from_str(trimmed)
                .with_context(|| format!("invalid stream item at line {}", idx + 1))?;
            items.push(item);
        }
        Ok(Self { items })
    }

    pub fn events(&self) -> impl Iterator<Item = &StreamEvent> {
        self.items.iter().filter_map(|item| match item {
            StreamItem::Event(event) => Some(event),
            StreamItem::Question(_) => None,
        })
    }

    pub fn questions(&self) -> impl Iterator<Item = &StreamQuestion> {
        self.items.iter().filter_map(|item| match item {
            StreamItem::Question(question) => Some(question),
            StreamItem::Event(_) => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# a comment line, and a blank line below should both be skipped

{"kind":"event","id":"e1","content":"Decided to use Postgres for the ledger","timestamp":"2026-01-01T00:00:00Z","actor":{"id":"alice","kind":"user"},"scope":{"kind":"org","id":"acme"},"source":"slack"}
{"kind":"event","id":"e2","content":"Switched the ledger to Cockroach","timestamp":"2026-02-01T00:00:00Z","actor":{"id":"alice","kind":"user"},"scope":{"kind":"org","id":"acme"},"source":"slack","supersedes":"e1"}
{"kind":"question","id":"q1","query":"What database does the ledger use?","asked_at":"2026-03-01T00:00:00Z","asked_by":{"id":"bob","kind":"user"},"gold_event_ids":["e2"],"gold_answer":"Cockroach"}
"#;

    #[test]
    fn parses_events_questions_and_skips_comments_and_blanks() {
        let stream = Stream::from_jsonl(SAMPLE).unwrap();

        assert_eq!(stream.events().count(), 2);
        assert_eq!(stream.questions().count(), 1);

        let e2 = stream.events().find(|e| e.id == "e2").unwrap();
        assert_eq!(e2.supersedes.as_deref(), Some("e1"));

        let q1 = stream.questions().next().unwrap();
        assert_eq!(q1.gold_event_ids, vec!["e2".to_string()]);
        assert_eq!(q1.gold_answer.as_deref(), Some("Cockroach"));
    }

    #[test]
    fn rejects_an_unknown_kind() {
        let error = Stream::from_jsonl(r#"{"kind":"fact_change","id":"f1"}"#).unwrap_err();
        assert!(error.to_string().contains("invalid stream item"));
    }
}

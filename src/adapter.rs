//! A reduced, local version of Plan 25's adapter contract: `ingest` and
//! `retrieve` only, no `actor` or scope filtering, for baselines that don't
//! claim to be scope-aware. The full contract (`actor`, `scope`, `answer`,
//! `use`, `settle`, and HTTP-backed vendor/Achiral systems) is a later
//! phase, not this one.

use crate::baselines::build_baseline;
use crate::eval::parse_systems;
use crate::runner::BenchmarkConfig;
use crate::scoring::recall_at_k;
use crate::stream::{Stream, StreamEvent, StreamItem};
use anyhow::Result;
use chrono::{DateTime, Utc};

pub trait MemorySystem {
    fn name(&self) -> &str;
    fn ingest(&mut self, event: &StreamEvent);
    /// Ranked event IDs, best first, at most `top_k` of them.
    fn retrieve(&mut self, query: &str, now: DateTime<Utc>, top_k: usize) -> Vec<String>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuestionResult {
    pub question_id: String,
    pub retrieved_ids: Vec<String>,
    pub recall_at_k: f64,
}

/// Replays a stream through `system` in its canonical chronological order:
/// every event up to a question's `asked_at` is ingested before that
/// question is retrieved, and never after - a system must not be able to
/// answer from events that haven't happened yet (the dataset lint already
/// catches this in the gold itself; the runner must not reintroduce it by
/// ingesting everything up front).
pub fn replay_stream(
    stream: &Stream,
    system: &mut dyn MemorySystem,
    top_k: usize,
) -> Vec<QuestionResult> {
    let mut results = Vec::new();
    for item in &stream.items {
        match item {
            StreamItem::Event(event) => system.ingest(event),
            StreamItem::Question(question) => {
                let retrieved_ids = system.retrieve(&question.query, question.asked_at, top_k);
                let recall = recall_at_k(&question.gold_event_ids, &retrieved_ids, top_k);
                results.push(QuestionResult {
                    question_id: question.id.clone(),
                    retrieved_ids,
                    recall_at_k: recall,
                });
            }
        }
    }
    results
}

/// Loads a stream, replays it through every baseline named in
/// `config.systems`, and prints per-question and mean recall@k for each.
pub fn run_baseline_replay(config: &BenchmarkConfig) -> Result<()> {
    let path = config
        .replay_stream
        .as_ref()
        .expect("run_baseline_replay requires --replay-stream");
    let stream = Stream::load(path)?;

    for name in parse_systems(&config.systems) {
        let mut system = build_baseline(&name, config.recency_half_life_days)?;
        let results = replay_stream(&stream, system.as_mut(), config.top_k);
        let mean_recall = if results.is_empty() {
            0.0
        } else {
            results.iter().map(|r| r.recall_at_k).sum::<f64>() / results.len() as f64
        };

        println!("=== {name} ===");
        for result in &results {
            println!(
                "  {}: recall@{} = {:.2}",
                result.question_id, config.top_k, result.recall_at_k
            );
        }
        println!(
            "  mean recall@{} over {} question(s): {:.4}",
            config.top_k,
            results.len(),
            mean_recall
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{Actor, ActorKind, Scope, ScopeKind};

    /// A spy system that only ever returns the ids it has been ingested so
    /// far, oldest first - just enough to prove the runner's chronological
    /// ordering without a real baseline.
    struct SeenSoFar {
        seen: Vec<String>,
    }

    impl MemorySystem for SeenSoFar {
        fn name(&self) -> &str {
            "seen-so-far"
        }
        fn ingest(&mut self, event: &StreamEvent) {
            self.seen.push(event.id.clone());
        }
        fn retrieve(&mut self, _query: &str, _now: DateTime<Utc>, top_k: usize) -> Vec<String> {
            self.seen.iter().take(top_k).cloned().collect()
        }
    }

    fn event(id: &str, ts: &str) -> StreamEvent {
        StreamEvent {
            id: id.to_string(),
            content: "x".to_string(),
            timestamp: ts.parse().unwrap(),
            actor: Actor {
                id: "a".to_string(),
                kind: ActorKind::User,
            },
            scope: Scope {
                kind: ScopeKind::Org,
                id: "acme".to_string(),
            },
            source: "test".to_string(),
            supersedes: None,
        }
    }

    #[test]
    fn a_question_only_sees_events_ingested_before_it() {
        let stream = Stream {
            items: vec![
                StreamItem::Event(event("e1", "2026-01-01T00:00:00Z")),
                StreamItem::Question(crate::stream::StreamQuestion {
                    id: "q1".to_string(),
                    query: "irrelevant".to_string(),
                    asked_at: "2026-01-15T00:00:00Z".parse().unwrap(),
                    asked_by: Actor {
                        id: "a".to_string(),
                        kind: ActorKind::User,
                    },
                    gold_event_ids: vec!["e1".to_string()],
                    gold_answer: None,
                    validity: None,
                }),
                StreamItem::Event(event("e2", "2026-02-01T00:00:00Z")),
            ],
        };

        let mut system = SeenSoFar { seen: Vec::new() };
        let results = replay_stream(&stream, &mut system, 10);

        assert_eq!(results.len(), 1);
        // e2 is ingested after q1 in the stream's order, so it must never
        // appear in q1's retrieval, and q1's gold (e1) is already visible.
        assert_eq!(results[0].retrieved_ids, vec!["e1".to_string()]);
        assert_eq!(results[0].recall_at_k, 1.0);
    }
}

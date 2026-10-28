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
use crate::tokens::{approx_token_count, geometric_checkpoints};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

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
    /// Approximate tokens this question would inject into a model's
    /// context: the retrieved events' content plus the query itself. The
    /// read-path half of the "durable horizon" token-savings signal (Plan
    /// 25) - what ingestion itself costs is a separate, write-path concern
    /// this does not measure.
    pub tokens_injected: usize,
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
    let content_by_id: HashMap<&str, &str> = stream
        .events()
        .map(|event| (event.id.as_str(), event.content.as_str()))
        .collect();

    let mut results = Vec::new();
    for item in &stream.items {
        match item {
            StreamItem::Event(event) => system.ingest(event),
            StreamItem::Question(question) => {
                let retrieved_ids = system.retrieve(&question.query, question.asked_at, top_k);
                let recall = recall_at_k(&question.gold_event_ids, &retrieved_ids, top_k);
                let tokens_injected = approx_token_count(&question.query)
                    + retrieved_ids
                        .iter()
                        .filter_map(|id| content_by_id.get(id.as_str()))
                        .map(|content| approx_token_count(content))
                        .sum::<usize>();
                results.push(QuestionResult {
                    question_id: question.id.clone(),
                    retrieved_ids,
                    recall_at_k: recall,
                    tokens_injected,
                });
            }
        }
    }
    results
}

/// Re-ingests `stream`'s events `cycles` times (each cycle's events
/// namespaced by cycle number, so a system that never forgets keeps
/// growing instead of just overwriting the same ids), reporting mean
/// tokens injected per question at geometric checkpoints (Plan 25's
/// "durable horizon": whether a token-cost shape holds as history grows,
/// not what it looks like on one run). Questions are re-asked unchanged
/// at every checkpoint; this measures read-path cost only.
pub fn token_usage_by_cycle(
    stream: &Stream,
    system: &mut dyn MemorySystem,
    top_k: usize,
    cycles: usize,
) -> Vec<(usize, f64)> {
    let mut pending_checkpoints: std::collections::HashSet<usize> =
        geometric_checkpoints(cycles).into_iter().collect();
    let mut content_by_id: HashMap<String, String> = HashMap::new();
    let mut report = Vec::new();

    for cycle in 1..=cycles {
        for event in stream.events() {
            let mut namespaced_event = event.clone();
            namespaced_event.id = format!("{}#{cycle}", event.id);
            content_by_id.insert(namespaced_event.id.clone(), event.content.clone());
            system.ingest(&namespaced_event);
        }

        if pending_checkpoints.remove(&cycle) {
            let mut total_tokens = 0usize;
            let mut count = 0usize;
            for question in stream.questions() {
                let retrieved_ids = system.retrieve(&question.query, question.asked_at, top_k);
                total_tokens += approx_token_count(&question.query)
                    + retrieved_ids
                        .iter()
                        .filter_map(|id| content_by_id.get(id))
                        .map(|content| approx_token_count(content))
                        .sum::<usize>();
                count += 1;
            }
            let mean = if count == 0 {
                0.0
            } else {
                total_tokens as f64 / count as f64
            };
            report.push((cycle, mean));
        }
    }
    report
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
        let mut system = build_baseline(
            &name,
            config.recency_half_life_days,
            config.bounded_window_size,
        )?;

        if config.cycles > 1 {
            let report =
                token_usage_by_cycle(&stream, system.as_mut(), config.top_k, config.cycles);
            println!("=== {name}: tokens injected per question, by cycle ===");
            for (cycle, mean_tokens) in report {
                println!("  cycle {cycle}: mean tokens injected = {mean_tokens:.1}");
            }
            continue;
        }

        let results = replay_stream(&stream, system.as_mut(), config.top_k);
        let mean_recall = if results.is_empty() {
            0.0
        } else {
            results.iter().map(|r| r.recall_at_k).sum::<f64>() / results.len() as f64
        };
        let mean_tokens = if results.is_empty() {
            0.0
        } else {
            results.iter().map(|r| r.tokens_injected).sum::<usize>() as f64 / results.len() as f64
        };

        println!("=== {name} ===");
        for result in &results {
            println!(
                "  {}: recall@{} = {:.2}, tokens injected = {}",
                result.question_id, config.top_k, result.recall_at_k, result.tokens_injected
            );
        }
        println!(
            "  mean recall@{} over {} question(s): {:.4}; mean tokens injected: {:.1}",
            config.top_k,
            results.len(),
            mean_recall,
            mean_tokens
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

    #[test]
    fn token_usage_by_cycle_reports_growth_at_each_checkpoint_for_full_context() {
        use crate::baselines::FullContextSystem;

        let stream = Stream {
            items: vec![
                StreamItem::Event(event("e1", "2026-01-01T00:00:00Z")), // content "x"
                StreamItem::Question(crate::stream::StreamQuestion {
                    id: "q1".to_string(),
                    query: "q".to_string(), // 1 token
                    asked_at: "2026-01-15T00:00:00Z".parse().unwrap(),
                    asked_by: Actor {
                        id: "a".to_string(),
                        kind: ActorKind::User,
                    },
                    gold_event_ids: vec!["e1".to_string()],
                    gold_answer: None,
                    validity: None,
                }),
            ],
        };

        let mut system = FullContextSystem::new();
        let report = token_usage_by_cycle(&stream, &mut system, 10, 3);

        // Checkpoints for cycles=3: just [3] (below the first decade).
        // By cycle 3, full-context has ingested 3 distinct "x" events (one
        // per cycle, namespaced), so mean tokens = 1 (query) + 3 (content).
        assert_eq!(report, vec![(3, 4.0)]);
    }

    #[test]
    fn tokens_injected_counts_the_retrieved_events_content_plus_the_query() {
        let stream = Stream {
            items: vec![
                StreamItem::Event(event("e1", "2026-01-01T00:00:00Z")), // content "x", 1 token
                StreamItem::Question(crate::stream::StreamQuestion {
                    id: "q1".to_string(),
                    query: "two words".to_string(), // 2 tokens
                    asked_at: "2026-01-15T00:00:00Z".parse().unwrap(),
                    asked_by: Actor {
                        id: "a".to_string(),
                        kind: ActorKind::User,
                    },
                    gold_event_ids: vec!["e1".to_string()],
                    gold_answer: None,
                    validity: None,
                }),
            ],
        };

        let mut system = SeenSoFar { seen: Vec::new() };
        let results = replay_stream(&stream, &mut system, 10);

        // e1's content is "x" (1 token) + query "two words" (2 tokens) = 3.
        assert_eq!(results[0].tokens_injected, 3);
    }
}

//! A reduced, local version of Plan 25's adapter contract: `ingest` and
//! `retrieve` (with the asking `actor`), no scope filtering of its own, for
//! baselines that don't claim to be scope-aware - every baseline in this
//! crate today ignores `actor` entirely, which is what makes "ignoring
//! scope" a measured leak rate (`leak_trials`) rather than an assumption. A
//! real adapter that wants to enforce scope filters using `actor` itself.
//! The rest of the full contract (`answer`, `use`, `settle`, and
//! HTTP-backed vendor/Achiral systems) is a later phase, not this one.

use crate::achiral_http::AchiralHttpSystem;
use crate::baselines::build_baseline;
use crate::eval::parse_systems;
use crate::http_stream::HttpStreamSystem;
use crate::runner::BenchmarkConfig;
use crate::scoring::{leak_rate, recall_at_k, LeakTrial};
use crate::stream::{Actor, Stream, StreamEvent, StreamItem};
use crate::tokens::{approx_token_count, geometric_checkpoints};
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

pub trait MemorySystem {
    fn name(&self) -> &str;
    fn ingest(&mut self, event: &StreamEvent);
    /// Ranked event IDs, best first, at most `top_k` of them. `actor` is
    /// who is asking (Plan 25 adapter contract v2: `retrieve(query, actor,
    /// now, top_k)`) - a system that wants to enforce scope checks it here;
    /// one that doesn't (every baseline in this crate) ignores it.
    fn retrieve(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<String>;

    /// `retrieve`'s ranking, each id paired with a confidence score, for
    /// calibration (Plan 25/26: "native" calibration on a system's own
    /// score, "derived" on anything else). Defaults to a rank-derived
    /// score (`1 / (rank + 1)`) for a system with no real confidence
    /// signal of its own; a system that already computes one (bm25,
    /// recency, an HTTP-backed vendor/Achiral adapter) overrides this to
    /// return it, so native calibration measures something real rather
    /// than a position in a list.
    fn retrieve_scored(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<(String, f64)> {
        self.retrieve(query, actor, now, top_k)
            .into_iter()
            .enumerate()
            .map(|(rank, id)| (id, 1.0 / (rank as f64 + 1.0)))
            .collect()
    }

    /// Which optional operations below this system actually implements.
    /// Defaults to none: `ingest`/`retrieve` is the only contract every
    /// system in this crate honors today (Plan 25 adapter contract v2).
    /// The run manifest records this per system once a system declares
    /// anything (not yet wired - no system overrides this).
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    /// Systems that synthesise an answer rather than only ranking memories
    /// (e.g. Hindsight's `reflect`) override this. `None` for every system
    /// that does not declare `capabilities().answer`.
    fn answer(&mut self, _query: &str, _now: DateTime<Utc>) -> Option<String> {
        None
    }

    /// Report which memories an answer relied on, the way a real client
    /// would (Plan 25: `use(memory_ids, helpful)`; Achiral maps this to
    /// `/agents/:agentSlug/:id/reinforce`). A no-op for every system that
    /// does not declare `capabilities().use_feedback`.
    fn use_feedback(&mut self, _memory_ids: &[String], _helpful: bool) {}

    /// Let consolidation or decay run before the next question is asked
    /// (Plan 25: `advance(now)` / `settle()`). A no-op for every system
    /// that does not declare `capabilities().settle`.
    fn settle(&mut self, _now: DateTime<Utc>) {}
}

/// Adapter contract v2's optional operations (Plan 25): which of
/// `answer`, `use_feedback` and `settle` a system actually implements,
/// beyond the required `ingest`/`retrieve`. A system that does not accept
/// a simulated clock (no `settle` support) runs on wall-clock time; that is
/// fair for systems without decay, and the run manifest records which mode
/// each system used.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub answer: bool,
    pub use_feedback: bool,
    pub settle: bool,
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
    /// Every retrieved candidate paired with its confidence score, in the
    /// same order as `retrieved_ids` - calibration's raw input once
    /// matched against `gold_event_ids` (see `calibration_pairs`).
    pub scored_candidates: Vec<(String, f64)>,
}

/// Turns a question's `scored_candidates` into the `(score, was_gold)`
/// pairs `calibration.rs`'s functions take directly.
pub fn calibration_pairs(result: &QuestionResult, gold_event_ids: &[String]) -> Vec<(f64, bool)> {
    let gold: HashSet<&str> = gold_event_ids.iter().map(String::as_str).collect();
    result
        .scored_candidates
        .iter()
        .map(|(id, score)| (*score, gold.contains(id.as_str())))
        .collect()
}

/// Builds one `LeakTrial` per question in `results`, pairing its asking
/// actor with the scopes of every memory it retrieved - `scoring::leak_rate`'s
/// input, computed straight from the stream. This measures a leak
/// regardless of whether `system` is scope-aware at all: a baseline with no
/// notion of scope (every baseline in this crate today) leaks exactly as
/// often as the data makes it, which is the honest number for "how much
/// does ignoring scope cost" (Plan 25 Team Memory: "A personal assistant's
/// private memory never surfaces for another user or agent").
pub fn leak_trials(stream: &Stream, results: &[QuestionResult]) -> Vec<LeakTrial> {
    let scope_by_id: HashMap<&str, &crate::stream::Scope> = stream
        .events()
        .map(|event| (event.id.as_str(), &event.scope))
        .collect();
    let asked_by: HashMap<&str, &crate::stream::Actor> = stream
        .questions()
        .map(|q| (q.id.as_str(), &q.asked_by))
        .collect();

    results
        .iter()
        .filter_map(|result| {
            let actor = asked_by.get(result.question_id.as_str())?;
            let returned_scopes = result
                .retrieved_ids
                .iter()
                .filter_map(|id| scope_by_id.get(id.as_str()).map(|scope| (*scope).clone()))
                .collect();
            Some(LeakTrial {
                actor: (*actor).clone(),
                returned_scopes,
            })
        })
        .collect()
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
                // Let a system with an asynchronous write path (Plan
                // 25/26: achiral.ai stages a candidate at ingest, writes
                // it on a separate cycle) settle before this question can
                // see what was just ingested - a no-op for every system
                // that doesn't override settle().
                system.settle(question.asked_at);
                let scored_candidates = system.retrieve_scored(
                    &question.query,
                    &question.asked_by,
                    question.asked_at,
                    top_k,
                );
                let retrieved_ids: Vec<String> =
                    scored_candidates.iter().map(|(id, _)| id.clone()).collect();
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
                    scored_candidates,
                });
            }
        }
    }
    results
}

/// Replays `stream` through a freshly built system `replicates` times,
/// returning each question's pass/fail outcome in every replicate, in
/// stream order - the shape `stability::flip_rate` takes directly. A
/// deterministic baseline always flips 0%; this is the control a noisy
/// system (ACT-R's retrieval noise, a real Achiral adapter) is compared
/// against. Each replicate starts from a fresh system (not a continued
/// one), matching "repeated identical runs" (Plan 25's run-to-run test),
/// not continued reinforcement across replicates.
///
/// `build_system` receives the replicate's index (0-based) - a system
/// backed by live, stateful storage (achiral.ai) needs it to pick a
/// separately-provisioned backing store per replicate; every baseline in
/// this crate ignores it, since it carries no state between calls anyway.
pub fn replay_stream_replicated(
    stream: &Stream,
    mut build_system: impl FnMut(usize) -> Box<dyn MemorySystem>,
    top_k: usize,
    replicates: usize,
) -> Vec<Vec<bool>> {
    assert!(replicates > 0, "need at least one replicate");
    let num_questions = stream.questions().count();
    let mut outcomes: Vec<Vec<bool>> = vec![Vec::with_capacity(replicates); num_questions];

    for index in 0..replicates {
        let mut system = build_system(index);
        let results = replay_stream(stream, system.as_mut(), top_k);
        for (i, result) in results.iter().enumerate() {
            if std::env::var("COGNOSCENTI_DEBUG_REPLICATES").is_ok() {
                eprintln!(
                    "[replicate {index}] {} recall@{top_k}={:.4} retrieved={:?}",
                    result.question_id, result.recall_at_k, result.retrieved_ids
                );
            }
            outcomes[i].push(result.recall_at_k >= 1.0);
        }
    }
    outcomes
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
            // Same reasoning as replay_stream(): an asynchronous write
            // path needs to settle before this checkpoint's questions can
            // see this cycle's events.
            system.settle(
                stream
                    .questions()
                    .next()
                    .map_or_else(Utc::now, |q| q.asked_at),
            );
            let mut total_tokens = 0usize;
            let mut count = 0usize;
            for question in stream.questions() {
                let retrieved_ids = system.retrieve(
                    &question.query,
                    &question.asked_by,
                    question.asked_at,
                    top_k,
                );
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

/// Builds a system by name: `"achiral"` is the real, vendor-specific
/// shim (`AchiralHttpSystem`) against achiral.ai's own public memory
/// API, not Cognoscenti's generic envelope; anything else first tries a
/// local baseline (`build_baseline`), then falls back to the generic
/// HTTP-backed system configured via
/// `COGNOSCENTI_<NAME>_INGEST_URL`/`_RETRIEVE_URL`/`_API_KEY` (Plan 25
/// Showdown 1 step 6) - the same dispatch shape as eval.rs's
/// `build_system` for the legacy item-eval path.
pub fn build_system(
    name: &str,
    recency_half_life_days: f64,
    bounded_window_size: usize,
) -> Result<Box<dyn MemorySystem>> {
    if name == "achiral" {
        return AchiralHttpSystem::from_env()
            .map(|system| Box::new(system) as Box<dyn MemorySystem>);
    }

    match build_baseline(name, recency_half_life_days, bounded_window_size) {
        Ok(system) => Ok(system),
        Err(baseline_err) => HttpStreamSystem::from_env(name)
            .map(|system| Box::new(system) as Box<dyn MemorySystem>)
            .map_err(|http_err| {
                anyhow::anyhow!(
                    "{name} is not a known baseline ({baseline_err}) and has no HTTP \
                     configuration ({http_err})"
                )
            }),
    }
}

/// Like `build_system()`, for one of several `--replicates`. Only
/// `"achiral"` is index-aware (`AchiralHttpSystem::from_env_at`, picking
/// one of several pre-provisioned orgs via `COGNOSCENTI_ACHIRAL_API_KEYS`),
/// since every baseline and the generic HTTP fallback carry no state
/// between calls; the index makes no difference to them, so this defers
/// to `build_system()` unchanged for anything else.
pub fn build_system_for_replicate(
    name: &str,
    index: usize,
    recency_half_life_days: f64,
    bounded_window_size: usize,
) -> Result<Box<dyn MemorySystem>> {
    if name == "achiral" {
        return AchiralHttpSystem::from_env_at(index)
            .map(|system| Box::new(system) as Box<dyn MemorySystem>);
    }
    build_system(name, recency_half_life_days, bounded_window_size)
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
        let mut system = build_system(
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

        let trials = leak_trials(&stream, &results);
        if !trials.is_empty() {
            println!(
                "  leak rate over {} trial(s): {:.4}",
                trials.len(),
                leak_rate(&trials)
            );
        }

        // None of these baselines claim a probability (baselines.rs: "the
        // reference isolates what activation adds") - raw scores (a term
        // count, an unbounded BM25 weight) run straight through
        // brier_score would silently mislabel an unbounded score as if it
        // were one. Derive a probability from the ranking first, same as
        // calibration.rs requires for any vendor that doesn't volunteer
        // one, and label it accordingly.
        let raw_pairs: Vec<(f64, bool)> = results
            .iter()
            .zip(stream.questions())
            .flat_map(|(result, question)| calibration_pairs(result, &question.gold_event_ids))
            .collect();
        if !raw_pairs.is_empty() {
            let derived_probabilities = crate::calibration::derive_probabilities_from_scores(
                &raw_pairs,
                config.calibration_bins,
            );
            let derived_pairs: Vec<(f64, bool)> = derived_probabilities
                .into_iter()
                .zip(raw_pairs.iter().map(|&(_, observed)| observed))
                .collect();
            let brier = crate::calibration::brier_score(&derived_pairs);
            let ece = crate::calibration::expected_calibration_error(
                &derived_pairs,
                config.calibration_bins,
            );
            println!(
                "  derived brier score = {brier:.4}; derived expected calibration error = {ece:.4}"
            );
        }

        if config.replicates > 1 {
            let recency_half_life_days = config.recency_half_life_days;
            let bounded_window_size = config.bounded_window_size;
            let name_for_replicates = name.clone();
            let outcomes = replay_stream_replicated(
                &stream,
                move |index| {
                    build_system_for_replicate(
                        &name_for_replicates,
                        index,
                        recency_half_life_days,
                        bounded_window_size,
                    )
                    .expect("system name already validated above, or COGNOSCENTI_ACHIRAL_API_KEYS has too few keys for this many replicates")
                },
                config.top_k,
                config.replicates,
            );
            let flip_rate = crate::stability::flip_rate(&outcomes);
            println!(
                "  flip rate over {} replicate(s): {flip_rate:.4}",
                config.replicates
            );
        }
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
        fn retrieve(
            &mut self,
            _query: &str,
            _actor: &Actor,
            _now: DateTime<Utc>,
            top_k: usize,
        ) -> Vec<String> {
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
            entities: Vec::new(),
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
    fn replay_stream_carries_scored_candidates_alongside_retrieved_ids() {
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
            ],
        };

        let mut system = SeenSoFar { seen: Vec::new() };
        let results = replay_stream(&stream, &mut system, 10);

        // SeenSoFar has no real score, so this exercises the trait's
        // rank-derived default: the one candidate at rank 0 scores 1.0.
        assert_eq!(results[0].scored_candidates, vec![("e1".to_string(), 1.0)]);

        let pairs = calibration_pairs(&results[0], &["e1".to_string()]);
        assert_eq!(pairs, vec![(1.0, true)]);
    }

    #[test]
    fn calibration_pairs_marks_a_non_gold_candidate_false() {
        let result = QuestionResult {
            question_id: "q1".to_string(),
            retrieved_ids: vec!["e1".to_string(), "e2".to_string()],
            recall_at_k: 0.0,
            tokens_injected: 0,
            scored_candidates: vec![("e1".to_string(), 0.9), ("e2".to_string(), 0.4)],
        };

        let pairs = calibration_pairs(&result, &["e2".to_string()]);
        assert_eq!(pairs, vec![(0.9, false), (0.4, true)]);
    }

    #[test]
    fn replay_stream_replicated_reports_zero_flip_rate_for_a_deterministic_system() {
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
            ],
        };

        let outcomes = replay_stream_replicated(
            &stream,
            |_index| Box::new(SeenSoFar { seen: Vec::new() }),
            10,
            3,
        );

        assert_eq!(outcomes, vec![vec![true, true, true]]);
        assert_eq!(crate::stability::flip_rate(&outcomes), 0.0);
    }

    // A real adapter against a live, stateful deployment (achiral.ai) can
    // only test independent replicates if each one talks to its own,
    // separately-provisioned backing store - the index lets the closure
    // pick a different one per replicate (e.g. the Nth of several
    // pre-provisioned orgs), instead of every replicate reusing the same
    // one and accumulating into it.
    #[test]
    fn replay_stream_replicated_passes_the_replicate_index_to_build_system() {
        let stream = Stream { items: vec![] };
        let mut seen_indices = Vec::new();

        replay_stream_replicated(
            &stream,
            |index| {
                seen_indices.push(index);
                Box::new(SeenSoFar { seen: Vec::new() })
            },
            10,
            4,
        );

        assert_eq!(seen_indices, vec![0, 1, 2, 3]);
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

    fn user_scoped_event(id: &str, owner: &str) -> StreamEvent {
        let mut event = event(id, "2026-01-01T00:00:00Z");
        event.scope = Scope {
            kind: ScopeKind::User,
            id: owner.to_string(),
        };
        event
    }

    fn question_asked_by(id: &str, actor_id: &str) -> crate::stream::StreamQuestion {
        crate::stream::StreamQuestion {
            id: id.to_string(),
            query: "irrelevant".to_string(),
            asked_at: "2026-01-15T00:00:00Z".parse().unwrap(),
            asked_by: Actor {
                id: actor_id.to_string(),
                kind: ActorKind::User,
            },
            gold_event_ids: vec![],
            gold_answer: None,
            validity: None,
        }
    }

    #[test]
    fn leak_trials_pairs_each_questions_actor_with_the_scopes_it_retrieved() {
        let stream = Stream {
            items: vec![
                StreamItem::Event(user_scoped_event("alice-note", "alice")),
                StreamItem::Question(question_asked_by("q1", "bob")),
            ],
        };

        // SeenSoFar has no scope awareness at all, so it returns
        // alice-note to anyone who asks - exactly the case leak_rate must
        // catch.
        let mut system = SeenSoFar { seen: Vec::new() };
        let results = replay_stream(&stream, &mut system, 10);

        let trials = leak_trials(&stream, &results);
        assert_eq!(trials.len(), 1);
        assert_eq!(trials[0].actor.id, "bob");
        assert_eq!(
            trials[0].returned_scopes,
            vec![user_scoped_event("alice-note", "alice").scope]
        );
    }

    #[test]
    fn leak_rate_over_leak_trials_catches_a_cross_actor_leak_but_not_self_access() {
        let stream = Stream {
            items: vec![
                StreamItem::Event(user_scoped_event("alice-note", "alice")),
                StreamItem::Question(question_asked_by("q-leak", "bob")),
                StreamItem::Question(question_asked_by("q-self", "alice")),
            ],
        };

        let mut system = SeenSoFar { seen: Vec::new() };
        let results = replay_stream(&stream, &mut system, 10);

        let trials = leak_trials(&stream, &results);
        // bob's trial leaks (alice's memory is not his); alice's does not.
        assert_eq!(crate::scoring::leak_rate(&trials), 0.5);
    }

    /// Returns org-scoped events to anyone, but a user-scoped event only to
    /// the actor asking - proving `actor` actually reaches `retrieve`
    /// through `replay_stream`, not just that the signature compiles.
    struct ScopeAwareSpy {
        events: Vec<(String, Scope)>,
    }

    impl MemorySystem for ScopeAwareSpy {
        fn name(&self) -> &str {
            "scope-aware-spy"
        }
        fn ingest(&mut self, event: &StreamEvent) {
            self.events.push((event.id.clone(), event.scope.clone()));
        }
        fn retrieve(
            &mut self,
            _query: &str,
            actor: &Actor,
            _now: DateTime<Utc>,
            top_k: usize,
        ) -> Vec<String> {
            self.events
                .iter()
                .filter(|(_, scope)| scope.kind == ScopeKind::Org || scope.id == actor.id)
                .map(|(id, _)| id.clone())
                .take(top_k)
                .collect()
        }
    }

    #[test]
    fn replay_stream_passes_the_asking_actor_through_to_retrieve() {
        let stream = Stream {
            items: vec![
                StreamItem::Event(user_scoped_event("alice-note", "alice")),
                StreamItem::Question(question_asked_by("q-alice", "alice")),
                StreamItem::Question(question_asked_by("q-bob", "bob")),
            ],
        };

        let mut system = ScopeAwareSpy { events: Vec::new() };
        let results = replay_stream(&stream, &mut system, 10);

        assert_eq!(results[0].retrieved_ids, vec!["alice-note".to_string()]);
        assert_eq!(results[1].retrieved_ids, Vec::<String>::new());
    }

    // A system whose write path is asynchronous (Plan 25/26: achiral.ai
    // stages a candidate at ingest, then writes it to its store on a
    // separate cycle) needs its consolidation to run before the *next*
    // question can see what was just ingested - settle() already exists
    // for exactly this ("let consolidation or decay run before the next
    // question is asked"), but nothing called it.
    struct SettleSpy {
        settled_at: Vec<DateTime<Utc>>,
    }

    impl MemorySystem for SettleSpy {
        fn name(&self) -> &str {
            "settle-spy"
        }
        fn ingest(&mut self, _event: &StreamEvent) {}
        fn retrieve(
            &mut self,
            _query: &str,
            _actor: &Actor,
            _now: DateTime<Utc>,
            _top_k: usize,
        ) -> Vec<String> {
            Vec::new()
        }
        fn settle(&mut self, now: DateTime<Utc>) {
            self.settled_at.push(now);
        }
    }

    #[test]
    fn replay_stream_settles_before_every_question_with_the_question_s_own_asked_at() {
        let stream = Stream {
            items: vec![
                StreamItem::Event(event("e1", "2026-01-01T00:00:00Z")),
                StreamItem::Question(question_asked_by("q1", "pm")),
                StreamItem::Event(event("e2", "2026-01-02T00:00:00Z")),
                StreamItem::Question(question_asked_by("q2", "pm")),
            ],
        };
        let mut system = SettleSpy {
            settled_at: Vec::new(),
        };

        replay_stream(&stream, &mut system, 3);

        // Once per question, not per event, and with that question's own
        // asked_at (question_asked_by's fixture: 2026-01-15T00:00:00Z) -
        // not the event timestamps, which settle() has nothing to do with.
        assert_eq!(
            system.settled_at,
            vec![
                "2026-01-15T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
                "2026-01-15T00:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            ]
        );
    }

    #[test]
    fn build_system_still_resolves_a_known_baseline_by_name() {
        let system = build_system("verbatim", 30.0, 10).expect("verbatim is a known baseline");
        assert_eq!(system.name(), "verbatim");
    }

    #[test]
    fn build_system_routes_achiral_to_the_vendor_specific_shim_not_the_generic_fallback() {
        let err = build_system("achiral", 30.0, 10)
            .map(|_| ())
            .expect_err("no COGNOSCENTI_ACHIRAL_BASE_URL is set in the test environment");
        // Must name the achiral-specific env var, not the generic
        // baseline/HttpStreamSystem fallback's error - proves "achiral"
        // is routed to AchiralHttpSystem, not treated as an unknown name.
        assert!(err.to_string().contains("COGNOSCENTI_ACHIRAL_BASE_URL"));
    }

    #[test]
    fn build_system_for_replicate_routes_achiral_through_from_env_at() {
        let err = build_system_for_replicate("achiral", 2, 30.0, 10)
            .map(|_| ())
            .expect_err("no COGNOSCENTI_ACHIRAL_BASE_URL is set in the test environment");
        assert!(err.to_string().contains("COGNOSCENTI_ACHIRAL_BASE_URL"));
    }

    #[test]
    fn build_system_for_replicate_ignores_the_index_for_every_non_achiral_system() {
        // A baseline carries no state between calls, so the index is
        // irrelevant to it - same outcome as build_system() regardless of
        // which index is passed.
        let system = build_system_for_replicate("verbatim", 3, 30.0, 10)
            .expect("verbatim is a known baseline");
        assert_eq!(system.name(), "verbatim");
    }

    #[test]
    fn build_system_falls_back_to_http_for_an_unrecognized_name_and_names_both_failures() {
        let err = build_system("nonexistent-vendor-xyz", 30.0, 10)
            .map(|_| ())
            .expect_err("no baseline or env config exists for this name");
        let message = err.to_string();
        assert!(message.contains("nonexistent-vendor-xyz"));
    }

    #[test]
    fn a_system_that_declares_no_capabilities_defaults_to_none_supported() {
        let system = SeenSoFar { seen: Vec::new() };
        assert_eq!(system.capabilities(), Capabilities::default());
        assert!(!Capabilities::default().answer);
        assert!(!Capabilities::default().use_feedback);
        assert!(!Capabilities::default().settle);
    }

    #[test]
    fn a_system_with_no_answer_use_or_settle_override_is_a_harmless_no_op() {
        let mut system = SeenSoFar { seen: Vec::new() };
        let now: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();

        assert_eq!(system.answer("anything", now), None);
        // Neither call should panic or require any state; that is the point
        // of a default no-op for a system that never declares the
        // capability.
        system.use_feedback(&["e1".to_string()], true);
        system.settle(now);
    }
}

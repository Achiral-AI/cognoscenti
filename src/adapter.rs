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
use crate::stability::paraphrase_agreement;
use crate::stats::{bootstrap_ci_95, bootstrap_statistic_ci_95, describe_event_rate};
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

    /// Entity ids (or display names) the system's own evidence graph
    /// reports as an unresolved gap for this query (Plan 26 Phase 5) - a
    /// different claim than `retrieve`: not "what did you find" but "what
    /// did you honestly admit you didn't". An empty Vec from a system that
    /// does not declare `capabilities().evidence_graph` is not a reported
    /// gap, since nothing asked it for one - callers must check the
    /// capability before scoring this as a failure.
    fn evidence_graph_gaps(
        &mut self,
        _query: &str,
        _actor: &Actor,
        _now: DateTime<Utc>,
        _top_k: usize,
    ) -> Vec<String> {
        Vec::new()
    }
}

/// Adapter contract v2's optional operations (Plan 25): which of
/// `answer`, `use_feedback` and `settle` a system actually implements,
/// beyond the required `ingest`/`retrieve`. A system that does not accept
/// a simulated clock (no `settle` support) runs on wall-clock time; that is
/// fair for systems without decay, and the run manifest records which mode
/// each system used. `evidence_graph` (Plan 26 Phase 5) is a separate
/// claim from any of these: whether `evidence_graph_gaps` reports anything
/// real for this system.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub answer: bool,
    pub use_feedback: bool,
    pub settle: bool,
    pub evidence_graph: bool,
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
    /// Plan 26 Phase 5: the fraction of this question's
    /// `expected_gap_entity_ids` the system's evidence graph actually
    /// disclosed. `None` when the question expects no gaps, or the
    /// system doesn't declare `capabilities().evidence_graph` - neither
    /// case is evidence of anything, so neither is scored as 0 or 1.
    pub gap_disclosure: Option<f64>,
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

/// One flag per trial: did it return any memory its asker was not
/// authorized to see. The bootstrap unit for leaks is the trial (one
/// question), not the returned memory, so the 3/n zero-leak bound and a
/// CI both count independent trials.
pub fn leak_trial_flags(trials: &[LeakTrial]) -> Vec<bool> {
    trials
        .iter()
        .map(|trial| {
            trial
                .returned_scopes
                .iter()
                .any(|scope| !crate::scoring::is_authorized(&trial.actor, scope))
        })
        .collect()
}

/// `scope-017-private` -> `scope-017`: the trial a generated event or
/// question belongs to. `None` for any other id, so hand-made datasets are
/// never regrouped.
fn trial_key(id: &str) -> Option<String> {
    let mut parts = id.splitn(3, '-');
    let category = parts.next()?;
    let digits = parts.next()?;
    (!category.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| format!("{category}-{digits}"))
}

/// Removes every item of each generated trial that holds a user-scope
/// event, and returns the dropped trial keys in first-seen order. A trial
/// is the events and the question that share a `category-NNN` id prefix.
pub fn drop_user_scope_trials(stream: &Stream) -> (Stream, Vec<String>) {
    let mut dropped: Vec<String> = Vec::new();
    for item in &stream.items {
        if let StreamItem::Event(event) = item {
            if event.scope.kind == crate::stream::ScopeKind::User {
                if let Some(key) = trial_key(&event.id) {
                    if !dropped.contains(&key) {
                        dropped.push(key);
                    }
                }
            }
        }
    }
    let kept = stream
        .items
        .iter()
        .filter(|item| trial_key(item.id()).is_none_or(|key| !dropped.contains(&key)))
        .cloned()
        .collect();
    (Stream { items: kept }, dropped)
}

/// The replay's run label line, so output records settings the harness
/// cannot see (a system's noise scale). Blank labels print nothing.
pub fn label_line(label: Option<&str>) -> Option<String> {
    let label = label?.trim();
    (!label.is_empty()).then(|| format!("label: {label}"))
}

/// The "mean recall@k" and "mean tokens injected" report lines, each with
/// its n and 95% bootstrap CI over questions.
pub fn recall_and_token_lines(results: &[QuestionResult], top_k: usize, seed: u64) -> Vec<String> {
    let recalls: Vec<f64> = results.iter().map(|r| r.recall_at_k).collect();
    let tokens: Vec<f64> = results.iter().map(|r| r.tokens_injected as f64).collect();
    vec![
        format!(
            "  mean recall@{top_k}: {}",
            bootstrap_ci_95(&recalls, seed).display()
        ),
        format!(
            "  mean tokens injected: {}",
            bootstrap_ci_95(&tokens, seed).display()
        ),
    ]
}

/// The category of a generated question id (`scope-017-q` -> `scope`), or
/// `None` for any other id, so hand-made datasets are never regrouped.
fn category_of(question_id: &str) -> Option<&str> {
    split_question_id(question_id).map(|(category, _)| category)
}

/// `scope-017-q` -> `("scope", "017")`; `None` for any other id.
fn split_question_id(question_id: &str) -> Option<(&str, &str)> {
    let rest = question_id.strip_suffix("-q")?;
    let (category, digits) = rest.rsplit_once('-')?;
    (!category.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then_some((category, digits))
}

/// Leak lines for a replay. A generated stream reports each category that
/// holds a private memory (`scope`, `agent`) on its own, because pooling
/// the categories that cannot leak would dilute the rate and inflate n.
/// Any other stream reports one pooled line, as hand-made datasets always
/// have.
pub fn leak_lines(stream: &Stream, results: &[QuestionResult], seed: u64) -> Vec<String> {
    let line = |label: &str, subset: &[QuestionResult]| -> Option<String> {
        let trials = leak_trials(stream, subset);
        (!trials.is_empty()).then(|| {
            format!(
                "  {label}: trials with a leak: {} (leaked returns per returned memory: {:.4})",
                describe_event_rate(&leak_trial_flags(&trials), seed),
                leak_rate(&trials)
            )
        })
    };
    let by_category: Vec<String> = ["scope", "agent"]
        .iter()
        .filter_map(|category| {
            let subset: Vec<QuestionResult> = results
                .iter()
                .filter(|r| category_of(&r.question_id) == Some(*category))
                .cloned()
                .collect();
            line(category, &subset)
        })
        .collect();
    if by_category.is_empty() {
        line("all questions", results).into_iter().collect()
    } else {
        by_category
    }
}

/// Stability lines for a generated stream; empty for any other stream.
/// Paraphrase pairs (`paraphrase-N-q` / `rephrase-N-q`) report how much of
/// the returned evidence two wordings share and how often only one wording
/// found the answer. Stable facts (`stable-N-q`) report accuracy for a short
/// versus a long history of later updates to other facts.
pub fn stability_lines(stream: &Stream, results: &[QuestionResult], seed: u64) -> Vec<String> {
    let mut lines = Vec::new();

    let by_id: HashMap<&str, &QuestionResult> = results
        .iter()
        .map(|r| (r.question_id.as_str(), r))
        .collect();
    let pairs: Vec<(&QuestionResult, &QuestionResult)> = results
        .iter()
        .filter_map(|original| {
            let (category, number) = split_question_id(&original.question_id)?;
            (category == "paraphrase").then_some(())?;
            let twin = by_id.get(format!("rephrase-{number}-q").as_str())?;
            Some((original, *twin))
        })
        .collect();
    if !pairs.is_empty() {
        let agreement: Vec<f64> = pairs
            .iter()
            .map(|(a, b)| paraphrase_agreement(&a.retrieved_ids, &b.retrieved_ids))
            .collect();
        let split: Vec<bool> = pairs
            .iter()
            .map(|(a, b)| (a.recall_at_k > 0.0) != (b.recall_at_k > 0.0))
            .collect();
        lines.push(format!(
            "  paraphrase pairs: evidence agreement (Jaccard of returned memories) {}",
            bootstrap_ci_95(&agreement, seed).display()
        ));
        lines.push(format!(
            "  paraphrase pairs where only one wording found the answer: {}",
            describe_event_rate(&split, seed)
        ));
    }

    let mut short = Vec::new();
    let mut long = Vec::new();
    for result in results {
        let Some(("stable", number)) = split_question_id(&result.question_id) else {
            continue;
        };
        let prefix = format!("stable-{number}-update-");
        let updates = stream
            .events()
            .filter(|event| event.id.starts_with(&prefix))
            .count();
        let hit = result.recall_at_k > 0.0;
        if updates <= 8 {
            short.push(hit);
        } else {
            long.push(hit);
        }
    }
    for (label, hits) in [("up to 8", &short), ("more than 8", &long)] {
        if !hits.is_empty() {
            let outcomes: Vec<f64> = hits.iter().map(|&hit| f64::from(u8::from(hit))).collect();
            lines.push(format!(
                "  unchanged fact with {label} later updates: accuracy {}",
                bootstrap_ci_95(&outcomes, seed).display()
            ));
        }
    }
    lines
}

/// Recall@k with n and CI per generated category, in first-seen order. Each
/// category is sized so its own CI is about +/-11 points; the pooled mean
/// would hide a weak category behind a strong one.
pub fn category_lines(results: &[QuestionResult], top_k: usize, seed: u64) -> Vec<String> {
    let mut categories: Vec<&str> = Vec::new();
    for result in results {
        if let Some(category) = category_of(&result.question_id) {
            if !categories.contains(&category) {
                categories.push(category);
            }
        }
    }
    categories
        .into_iter()
        .map(|category| {
            let recalls: Vec<f64> = results
                .iter()
                .filter(|r| category_of(&r.question_id) == Some(category))
                .map(|r| r.recall_at_k)
                .collect();
            format!(
                "  category {category}: recall@{top_k} {}",
                bootstrap_ci_95(&recalls, seed).display()
            )
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
                // Plan 26 Phase 5: only call (and only score) this when
                // both sides opted in - a question with nothing to expect
                // and a system with nothing to report are both silence,
                // not failure.
                let gap_disclosure = if question.expected_gap_entity_ids.is_empty()
                    || !system.capabilities().evidence_graph
                {
                    None
                } else {
                    let reported = system.evidence_graph_gaps(
                        &question.query,
                        &question.asked_by,
                        question.asked_at,
                        top_k,
                    );
                    let reported: HashSet<&str> = reported.iter().map(String::as_str).collect();
                    let disclosed = question
                        .expected_gap_entity_ids
                        .iter()
                        .filter(|id| reported.contains(id.as_str()))
                        .count();
                    Some(disclosed as f64 / question.expected_gap_entity_ids.len() as f64)
                };
                results.push(QuestionResult {
                    question_id: question.id.clone(),
                    retrieved_ids,
                    recall_at_k: recall,
                    tokens_injected,
                    gap_disclosure,
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
///
/// `first_replicate_results`, when given, stands in for replicate 0's own
/// run instead of this function building and ingesting into it again -
/// `run_baseline_replay`'s own preliminary single pass (recall/leak/
/// calibration reporting) already builds and ingests into the exact org
/// `build_system(0)` would resolve to again here, via the same closure.
/// Without this, replicate 0 alone got the whole stream ingested into it
/// twice - confirmed live: 9 of 10 replicates agreed byte-for-byte, only
/// replicate 0 differed, retrieving an event that could not exist yet at
/// that point in the stream.
pub fn replay_stream_replicated(
    stream: &Stream,
    mut build_system: impl FnMut(usize) -> Box<dyn MemorySystem>,
    top_k: usize,
    replicates: usize,
    first_replicate_results: Option<&[QuestionResult]>,
) -> Vec<Vec<bool>> {
    assert!(replicates > 0, "need at least one replicate");
    let num_questions = stream.questions().count();
    let mut outcomes: Vec<Vec<bool>> = vec![Vec::with_capacity(replicates); num_questions];

    let debug = std::env::var("COGNOSCENTI_DEBUG_REPLICATES").is_ok();
    let start_index = if let Some(results) = first_replicate_results {
        for (i, result) in results.iter().enumerate() {
            if debug {
                eprintln!(
                    "[replicate 0] {} recall@{top_k}={:.4} retrieved={:?}",
                    result.question_id, result.recall_at_k, result.retrieved_ids
                );
            }
            outcomes[i].push(result.recall_at_k >= 1.0);
        }
        1
    } else {
        0
    };

    for index in start_index..replicates {
        let mut system = build_system(index);
        let results = replay_stream(stream, system.as_mut(), top_k);
        for (i, result) in results.iter().enumerate() {
            if debug {
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
    let mut stream = Stream::load(path)?;
    if let Some(line) = label_line(config.label.as_deref()) {
        println!("{line}");
    }
    if config.skip_user_scope {
        let (kept, dropped) = drop_user_scope_trials(&stream);
        println!(
            "not run: {} trial(s) holding a user-scope event (user scope unavailable, Plan 41); {} question(s) kept",
            dropped.len(),
            kept.questions().count()
        );
        let mut by_category: Vec<(String, usize)> = Vec::new();
        for key in &dropped {
            let category = key
                .rsplit_once('-')
                .map_or(key.as_str(), |(c, _)| c)
                .to_string();
            match by_category.iter_mut().find(|(c, _)| *c == category) {
                Some((_, n)) => *n += 1,
                None => by_category.push((category, 1)),
            }
        }
        for (category, n) in by_category {
            println!("  not run, {category}: {n}");
        }
        stream = kept;
    }

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
        println!("=== {name} ===");
        for result in &results {
            let gap_suffix = match result.gap_disclosure {
                Some(fraction) => format!(", gap disclosure = {fraction:.2}"),
                None => String::new(),
            };
            println!(
                "  {}: recall@{} = {:.2}, tokens injected = {}{gap_suffix}",
                result.question_id, config.top_k, result.recall_at_k, result.tokens_injected
            );
        }
        for line in recall_and_token_lines(&results, config.top_k, config.seed) {
            println!("{line}");
        }

        for line in category_lines(&results, config.top_k, config.seed) {
            println!("{line}");
        }
        for line in stability_lines(&stream, &results, config.seed) {
            println!("{line}");
        }

        // Plan 26 Phase 5: only questions that declared an expected gap
        // and a system that declared evidence_graph support produce a
        // Some(...) here - only those count toward this mean, so a
        // dataset with no gap questions prints nothing misleading.
        let gap_scores: Vec<f64> = results.iter().filter_map(|r| r.gap_disclosure).collect();
        if !gap_scores.is_empty() {
            println!(
                "  mean gap disclosure: {}",
                bootstrap_ci_95(&gap_scores, config.seed).display()
            );
        }

        for line in leak_lines(&stream, &results, config.seed) {
            println!("{line}");
        }

        // None of these baselines claim a probability (baselines.rs: "the
        // reference isolates what activation adds") - raw scores (a term
        // count, an unbounded BM25 weight) run straight through
        // brier_score would silently mislabel an unbounded score as if it
        // were one. Derive a probability from the ranking first, same as
        // calibration.rs requires for any vendor that doesn't volunteer
        // one, and label it accordingly.
        let pairs_by_question: Vec<Vec<(f64, bool)>> = results
            .iter()
            .zip(stream.questions())
            .map(|(result, question)| calibration_pairs(result, &question.gold_event_ids))
            .collect();
        let derived_pairs = |question_indices: &[usize]| -> Vec<(f64, bool)> {
            let raw: Vec<(f64, bool)> = question_indices
                .iter()
                .flat_map(|&i| pairs_by_question[i].iter().copied())
                .collect();
            if raw.is_empty() {
                return raw;
            }
            let derived =
                crate::calibration::derive_probabilities_from_scores(&raw, config.calibration_bins);
            derived
                .into_iter()
                .zip(raw.iter().map(|&(_, observed)| observed))
                .collect()
        };
        let all_questions: Vec<usize> = (0..pairs_by_question.len()).collect();
        let all_pairs = derived_pairs(&all_questions);
        if !all_pairs.is_empty() {
            let brier = crate::calibration::brier_score(&all_pairs);
            // ECE is resampled over questions, not over candidate rows: a
            // question's candidates are not independent of each other.
            let ece = bootstrap_statistic_ci_95(all_questions.len(), config.seed, |idx| {
                let pairs = derived_pairs(idx);
                if pairs.is_empty() {
                    0.0
                } else {
                    crate::calibration::expected_calibration_error(&pairs, config.calibration_bins)
                }
            });
            println!(
                "  derived brier score = {brier:.4}; derived expected calibration error = {}",
                ece.display()
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
                Some(&results),
            );
            println!(
                "  questions that flipped across {} replicate(s): {}",
                config.replicates,
                describe_event_rate(&crate::stability::flipped_questions(&outcomes), config.seed)
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
                    expected_gap_entity_ids: Vec::new(),
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
    fn a_leak_trial_counts_once_however_many_unauthorized_memories_it_returns() {
        let alice = Actor {
            id: "alice".to_string(),
            kind: ActorKind::User,
        };
        let private_of = |id: &str| Scope {
            kind: ScopeKind::User,
            id: id.to_string(),
        };
        let trials = [
            LeakTrial {
                actor: alice.clone(),
                returned_scopes: vec![private_of("bob"), private_of("carol")],
            },
            LeakTrial {
                actor: alice.clone(),
                returned_scopes: vec![private_of("alice")],
            },
            LeakTrial {
                actor: alice,
                returned_scopes: vec![],
            },
        ];
        assert_eq!(leak_trial_flags(&trials), vec![true, false, false]);
    }

    #[test]
    fn the_replay_summary_prints_n_and_a_ci_for_every_rate() {
        let results = vec![
            QuestionResult {
                question_id: "q1".into(),
                retrieved_ids: vec![],
                recall_at_k: 1.0,
                tokens_injected: 10,
                scored_candidates: vec![],
                gap_disclosure: None,
            },
            QuestionResult {
                question_id: "q2".into(),
                retrieved_ids: vec![],
                recall_at_k: 0.0,
                tokens_injected: 30,
                scored_candidates: vec![],
                gap_disclosure: None,
            },
        ];
        let lines = recall_and_token_lines(&results, 3, 0);
        assert!(
            lines[0].contains("recall@3") && lines[0].contains("0.5000"),
            "{lines:?}"
        );
        assert!(
            lines[0].contains("95% CI") && lines[0].contains("n=2"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("20.0000") && lines[1].contains("n=2"),
            "{lines:?}"
        );
    }

    #[test]
    fn leak_lines_cover_each_leak_bearing_category_with_its_own_n() {
        let stream = Stream::from_jsonl(
            r#"{"kind":"event","id":"scope-000-private","content":"x","timestamp":"2026-01-01T00:00:00Z","actor":{"id":"bob","kind":"user"},"scope":{"kind":"user","id":"bob"},"source":"s"}
{"kind":"question","id":"scope-000-q","query":"x","asked_at":"2026-01-02T00:00:00Z","asked_by":{"id":"alice","kind":"user"},"gold_event_ids":[]}
{"kind":"question","id":"agent-000-q","query":"x","asked_at":"2026-01-02T00:00:00Z","asked_by":{"id":"alice","kind":"user"},"gold_event_ids":[]}
{"kind":"question","id":"retrieve-000-q","query":"x","asked_at":"2026-01-02T00:00:00Z","asked_by":{"id":"alice","kind":"user"},"gold_event_ids":[]}"#,
        )
        .unwrap();
        let result = |id: &str, retrieved: &[&str]| QuestionResult {
            question_id: id.to_string(),
            retrieved_ids: retrieved.iter().map(|s| s.to_string()).collect(),
            recall_at_k: 1.0,
            tokens_injected: 1,
            scored_candidates: vec![],
            gap_disclosure: None,
        };
        let results = vec![
            result("scope-000-q", &["scope-000-private"]),
            result("agent-000-q", &[]),
            result("retrieve-000-q", &[]),
        ];
        let lines = leak_lines(&stream, &results, 0);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].contains("scope") && lines[0].contains("1/1"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("agent") && lines[1].contains("0/1"),
            "{lines:?}"
        );
    }

    #[test]
    fn stability_lines_score_paraphrase_pairs_and_stable_facts_by_history() {
        let question = |id: &str| {
            format!(
                r#"{{"kind":"question","id":"{id}","query":"{id}","asked_at":"2026-01-02T00:00:00Z","asked_by":{{"id":"alice","kind":"user"}},"gold_event_ids":["g"]}}"#
            )
        };
        let update = |id: &str| {
            format!(
                r#"{{"kind":"event","id":"{id}","content":"x","timestamp":"2026-01-01T00:00:00Z","actor":{{"id":"alice","kind":"user"}},"scope":{{"kind":"org","id":"o"}},"source":"s"}}"#
            )
        };
        let mut lines_in = Vec::new();
        for n in 0..3 {
            lines_in.push(question(&format!("paraphrase-00{n}-q")));
            lines_in.push(question(&format!("rephrase-00{n}-q")));
        }
        // Histories of 1, 8 and 9 updates: 8 is the last "short" one.
        for (number, updates) in [(0, 1), (1, 8), (2, 9)] {
            for u in 1..=updates {
                lines_in.push(update(&format!("stable-00{number}-update-{u}")));
            }
            lines_in.push(question(&format!("stable-00{number}-q")));
        }
        let stream = Stream::from_jsonl(&lines_in.join("\n")).unwrap();
        let result = |id: &str, retrieved: &[&str], recall: f64| QuestionResult {
            question_id: id.to_string(),
            retrieved_ids: retrieved.iter().map(|s| s.to_string()).collect(),
            recall_at_k: recall,
            tokens_injected: 1,
            scored_candidates: vec![],
            gap_disclosure: None,
        };
        let results = vec![
            result("paraphrase-000-q", &["g", "x"], 1.0),
            result("rephrase-000-q", &["g", "x"], 1.0),
            result("paraphrase-001-q", &["g"], 1.0),
            result("rephrase-001-q", &["g"], 1.0),
            result("paraphrase-002-q", &["g"], 1.0),
            result("rephrase-002-q", &["y"], 0.0),
            result("stable-000-q", &["g"], 1.0),
            result("stable-001-q", &["y"], 0.0),
            result("stable-002-q", &["g"], 1.0),
        ];
        let lines = stability_lines(&stream, &results, 0).join("\n");
        // Jaccard 1, 1 and 0 -> mean 2/3 over three pairs; one pair split.
        assert!(
            lines.contains("Jaccard of returned memories) 0.6667"),
            "{lines}"
        );
        assert!(
            lines.contains("only one wording found the answer: 1/3 events; 0.3333"),
            "{lines}"
        );
        // 1 and 8 updates are short (one hit of two); 9 is long (a hit).
        assert!(
            lines.contains("up to 8 later updates: accuracy 0.5000"),
            "{lines}"
        );
        assert!(
            lines.contains("more than 8 later updates: accuracy 1.0000 (n=1"),
            "{lines}"
        );
    }

    fn event_line(id: &str, scope_kind: &str, scope_id: &str) -> String {
        format!(
            r#"{{"kind":"event","id":"{id}","content":"x","timestamp":"2026-01-01T00:00:00Z","actor":{{"id":"a","kind":"user"}},"scope":{{"kind":"{scope_kind}","id":"{scope_id}"}},"source":"s"}}"#
        )
    }

    fn question_line(id: &str) -> String {
        format!(
            r#"{{"kind":"question","id":"{id}","query":"q","asked_at":"2026-01-02T00:00:00Z","asked_by":{{"id":"a","kind":"user"}},"gold_event_ids":[]}}"#
        )
    }

    #[test]
    fn dropping_user_scope_trials_removes_every_item_of_a_trial_that_holds_one() {
        let stream = Stream::from_jsonl(
            &[
                event_line("scope-000-private", "user", "ben"),
                event_line("scope-000-org", "org", "o"),
                question_line("scope-000-q"),
                event_line("scope-001-private", "agent", "bot"),
                question_line("scope-001-q"),
                // Not a generated id: left alone even with a user-scope event.
                event_line("sam-e1", "user", "sam"),
                question_line("sam-q1"),
                // `scope-1` must not match `scope-10-...` by prefix.
                event_line("scope-10-private", "agent", "bot"),
                question_line("scope-10-q"),
            ]
            .join("\n"),
        )
        .unwrap();
        let (kept, dropped) = drop_user_scope_trials(&stream);
        let ids: Vec<&str> = kept.items.iter().map(|i| i.id()).collect();
        assert_eq!(
            ids,
            vec![
                "scope-001-private",
                "scope-001-q",
                "sam-e1",
                "sam-q1",
                "scope-10-private",
                "scope-10-q"
            ]
        );
        assert_eq!(dropped, vec!["scope-000".to_string()]);
    }

    #[test]
    fn the_default_generated_stream_keeps_the_544_questions_the_preregistration_names() {
        let stream = crate::generator::generate(&crate::generator::GeneratorConfig::new(0));
        let (kept, dropped) = drop_user_scope_trials(&stream);
        assert_eq!(kept.questions().count(), 544);
        assert_eq!(dropped.len(), 316);
        let count = |prefix: &str| dropped.iter().filter(|t| t.starts_with(prefix)).count();
        assert_eq!(
            (count("scope-"), count("owner-"), count("individual-")),
            (229, 52, 35)
        );
        assert!(kept
            .events()
            .all(|e| e.scope.kind != crate::stream::ScopeKind::User));
    }

    #[test]
    fn label_line_records_the_run_label_and_is_absent_without_one() {
        assert_eq!(
            label_line(Some("activationNoiseScale=0.2")),
            Some("label: activationNoiseScale=0.2".to_string())
        );
        assert_eq!(label_line(None), None);
        assert_eq!(label_line(Some("  ")), None);
    }

    #[test]
    fn category_of_reads_generated_question_ids_only() {
        assert_eq!(category_of("scope-017-q"), Some("scope"));
        assert_eq!(category_of("supersede-003-q"), Some("supersede"));
        assert_eq!(category_of("atlas-q-15d"), None);
        assert_eq!(category_of("beacon-q-leak"), None);
        // Ends in "-q" but the middle is not a number.
        assert_eq!(category_of("hand-made-q"), None);
        assert_eq!(category_of("-017-q"), None);
    }

    #[test]
    fn category_lines_report_each_generated_category_with_its_own_n() {
        let result = |id: &str, recall: f64| QuestionResult {
            question_id: id.to_string(),
            retrieved_ids: vec![],
            recall_at_k: recall,
            tokens_injected: 1,
            scored_candidates: vec![],
            gap_disclosure: None,
        };
        let results = vec![
            result("scope-000-q", 1.0),
            result("scope-001-q", 0.0),
            result("retrieve-000-q", 1.0),
            result("hand-made", 1.0),
        ];
        let lines = category_lines(&results, 3, 0);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].contains("scope") && lines[0].contains("n=2"),
            "{lines:?}"
        );
        assert!(
            lines[1].contains("retrieve") && lines[1].contains("n=1"),
            "{lines:?}"
        );
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
                    expected_gap_entity_ids: Vec::new(),
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
                    expected_gap_entity_ids: Vec::new(),
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
            gap_disclosure: None,
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
                    expected_gap_entity_ids: Vec::new(),
                }),
            ],
        };

        let outcomes = replay_stream_replicated(
            &stream,
            |_index| Box::new(SeenSoFar { seen: Vec::new() }),
            10,
            3,
            None,
        );

        assert_eq!(outcomes, vec![vec![true, true, true]]);
        assert_eq!(crate::stability::flip_rate(&outcomes), 0.0);
    }

    // Root-cause fix (Plan 25 flip-rate investigation): run_baseline_replay's
    // own preliminary single pass (before this function's loop even starts)
    // already builds and ingests into replicate 0's own org - that org's
    // index (0) is the same one this function's own loop would otherwise
    // resolve to for its first iteration, via the exact same build_system
    // closure. Without this, replicate 0 silently got the whole stream
    // ingested into it twice (once by the preliminary pass, once again
    // here), corrupting exactly one replicate's state and nothing else -
    // confirmed live: 9 of 10 replicates agreed byte-for-byte, only
    // replicate 0 differed, retrieving an event that could not have existed
    // yet at that point in the stream. Passing the preliminary pass's own
    // results back in lets this function reuse them for replicate 0
    // instead of re-building and re-ingesting into the same org again.
    #[test]
    fn skips_building_a_system_for_replicate_zero_when_its_results_are_already_known() {
        let stream = Stream {
            items: vec![StreamItem::Question(question_asked_by("q1", "pm"))],
        };
        let mut seen_indices = Vec::new();
        let first_replicate_results = vec![QuestionResult {
            question_id: "q1".to_string(),
            retrieved_ids: vec!["e1".to_string()],
            recall_at_k: 1.0,
            tokens_injected: 0,
            scored_candidates: vec![],
            gap_disclosure: None,
        }];

        let outcomes = replay_stream_replicated(
            &stream,
            |index| {
                seen_indices.push(index);
                Box::new(SeenSoFar { seen: Vec::new() })
            },
            10,
            3,
            Some(&first_replicate_results),
        );

        assert_eq!(seen_indices, vec![1, 2]);
        assert_eq!(outcomes[0][0], true);
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
            None,
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
                    expected_gap_entity_ids: Vec::new(),
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
            expected_gap_entity_ids: Vec::new(),
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

    struct EvidenceGraphSpy {
        reported_gaps: Vec<String>,
    }

    impl MemorySystem for EvidenceGraphSpy {
        fn name(&self) -> &str {
            "evidence-graph-spy"
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
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                evidence_graph: true,
                ..Default::default()
            }
        }
        fn evidence_graph_gaps(
            &mut self,
            _query: &str,
            _actor: &Actor,
            _now: DateTime<Utc>,
            _top_k: usize,
        ) -> Vec<String> {
            self.reported_gaps.clone()
        }
    }

    fn question_with_expected_gaps(id: &str, expected: Vec<&str>) -> crate::stream::StreamQuestion {
        crate::stream::StreamQuestion {
            id: id.to_string(),
            query: "irrelevant".to_string(),
            asked_at: "2026-01-15T00:00:00Z".parse().unwrap(),
            asked_by: Actor {
                id: "a".to_string(),
                kind: ActorKind::User,
            },
            gold_event_ids: vec![],
            gold_answer: None,
            validity: None,
            expected_gap_entity_ids: expected.into_iter().map(str::to_string).collect(),
        }
    }

    // Plan 26 Phase 5: a question with no expected gaps has nothing to
    // score here - None, not a misleading perfect (or zero) score.
    #[test]
    fn gap_disclosure_is_none_when_the_question_expects_no_gaps() {
        let stream = Stream {
            items: vec![StreamItem::Question(question_asked_by("q1", "pm"))],
        };
        let mut system = EvidenceGraphSpy {
            reported_gaps: vec!["northwind".to_string()],
        };

        let results = replay_stream(&stream, &mut system, 3);

        assert_eq!(results[0].gap_disclosure, None);
    }

    // A system that never declares evidence_graph support must not be
    // scored on it either - its default evidence_graph_gaps() returning
    // an empty Vec is not evidence the system failed to disclose a gap,
    // only that nothing asked it to.
    #[test]
    fn gap_disclosure_is_none_for_a_system_that_does_not_support_evidence_graphs() {
        let stream = Stream {
            items: vec![StreamItem::Question(question_with_expected_gaps(
                "q1",
                vec!["northwind"],
            ))],
        };
        let mut system = SettleSpy {
            settled_at: Vec::new(),
        };

        let results = replay_stream(&stream, &mut system, 3);

        assert_eq!(results[0].gap_disclosure, None);
    }

    #[test]
    fn gap_disclosure_is_the_fraction_of_expected_gaps_actually_reported() {
        let stream = Stream {
            items: vec![StreamItem::Question(question_with_expected_gaps(
                "q1",
                vec!["northwind", "beacon"],
            ))],
        };
        let mut system = EvidenceGraphSpy {
            reported_gaps: vec!["northwind".to_string()],
        };

        let results = replay_stream(&stream, &mut system, 3);

        assert_eq!(results[0].gap_disclosure, Some(0.5));
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
        assert!(!Capabilities::default().evidence_graph);
    }

    // Plan 26 Phase 5: a system with no evidence-graph support must not
    // be scored on gap disclosure at all (not scored as a failure) - an
    // empty Vec from the default `evidence_graph_gaps` is not evidence of
    // anything, since nothing asked the system for it.
    #[test]
    fn a_system_with_no_evidence_graph_override_reports_no_gaps() {
        let mut system = SeenSoFar { seen: Vec::new() };
        let actor = Actor {
            id: "a".to_string(),
            kind: ActorKind::User,
        };
        assert_eq!(
            system.evidence_graph_gaps(
                "irrelevant",
                &actor,
                "2026-01-01T00:00:00Z".parse().unwrap(),
                3
            ),
            Vec::<String>::new()
        );
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

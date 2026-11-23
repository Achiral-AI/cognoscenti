//! Retrieval-level scoring for the team track. Every rule is a question
//! about event ids and their rank in what the system returned; no model
//! reads an answer.
//!
//! - Supersession correctness, the two controls and provenance accuracy all
//!   use one rule: the gold event is returned and ranks above every hard
//!   negative that was also returned. The controls are what make the rule
//!   mean something: a system that always returns the newest event passes
//!   `supersede` and fails both controls, so a supersession score is only
//!   read next to the control scores.
//! - Lineage is scored per edge: an edge counts when both of its events are
//!   returned.
//! - Leaver items score recall of the org-visible decision and any returned
//!   event the asker may not see (`is_authorized`).
//!
//! Only questions that have a result are scored; `missing` counts the rest,
//! so a run that skipped items cannot look better by being smaller.

use super::{Scenario, TeamItem};
use crate::adapter::QuestionResult;
use crate::results::{NamedStat, Stat};
use crate::scoring::is_authorized;
use crate::stream::{Stream, StreamEvent, StreamQuestion};
use std::collections::HashMap;

/// A count out of a total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    pub hits: usize,
    pub n: usize,
}

impl Tally {
    pub fn rate(&self) -> Option<f64> {
        (self.n > 0).then(|| self.hits as f64 / self.n as f64)
    }
    fn add(&mut self, hit: bool) {
        self.n += 1;
        self.hits += usize::from(hit);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TeamScores {
    /// Gold is the newest value and outranks every replaced one.
    pub supersession_correctness: Tally,
    /// Control: the older fact still stands; later events about something
    /// else must not outrank it.
    pub control_keep: Tally,
    /// Control: a point-in-time question; the replaced value must outrank
    /// its replacement.
    pub control_asof: Tally,
    /// The originating statement outranks every restatement.
    pub provenance_accuracy: Tally,
    /// Gold edges whose two events were both returned.
    pub lineage_edge_recall: Tally,
    /// Items whose every gold event and edge was returned.
    pub lineage_chain_complete: Tally,
    /// Items that returned the other project's decision.
    pub lineage_decoy_hit: Tally,
    /// Leaver items that returned the org-visible decision.
    pub leaver_recall: Tally,
    /// Leaver items that returned any event the asker may not see.
    pub leaver_leaks: Tally,
    pub tokens_per_question: Option<f64>,
    pub missing: usize,
}

impl TeamScores {
    /// Both hard-negative controls pooled.
    pub fn control_accuracy(&self) -> Tally {
        Tally {
            hits: self.control_keep.hits + self.control_asof.hits,
            n: self.control_keep.n + self.control_asof.n,
        }
    }
}

fn outranks_every_negative(item: &TeamItem, retrieved: &[String]) -> bool {
    let pos = |id: &str| retrieved.iter().position(|x| x == id);
    let Some(gold) = item.gold_evidence.first().and_then(|g| pos(g)) else {
        return false;
    };
    item.hard_negatives
        .iter()
        .filter_map(|n| pos(n))
        .all(|n| gold < n)
}

pub fn score_team(items: &[TeamItem], stream: &Stream, results: &[QuestionResult]) -> TeamScores {
    let by_question: HashMap<&str, &QuestionResult> = results
        .iter()
        .map(|r| (r.question_id.as_str(), r))
        .collect();
    let questions: HashMap<&str, &StreamQuestion> =
        stream.questions().map(|q| (q.id.as_str(), q)).collect();
    let events: HashMap<&str, &StreamEvent> = stream.events().map(|e| (e.id.as_str(), e)).collect();
    let mut out = TeamScores::default();
    let (mut tokens, mut counted) = (0usize, 0usize);
    for item in items {
        let Some(r) = by_question.get(item.question_id.as_str()) else {
            out.missing += 1;
            continue;
        };
        counted += 1;
        tokens += r.tokens_injected;
        let got = &r.retrieved_ids;
        let has = |id: &String| got.contains(id);
        match item.scenario {
            Scenario::Supersede => out
                .supersession_correctness
                .add(outranks_every_negative(item, got)),
            Scenario::ControlKeep => out.control_keep.add(outranks_every_negative(item, got)),
            Scenario::ControlAsOf => out.control_asof.add(outranks_every_negative(item, got)),
            Scenario::Provenance => out
                .provenance_accuracy
                .add(outranks_every_negative(item, got)),
            Scenario::Lineage => {
                let mut all = item.gold_evidence.iter().all(has);
                for e in &item.gold_edges {
                    let hit = has(&e.from) && has(&e.to);
                    out.lineage_edge_recall.add(hit);
                    all &= hit;
                }
                out.lineage_chain_complete.add(all);
                out.lineage_decoy_hit
                    .add(item.hard_negatives.iter().any(has));
            }
            Scenario::Leaver => {
                out.leaver_recall.add(item.gold_evidence.iter().all(has));
                let leaked = questions.get(item.question_id.as_str()).is_some_and(|q| {
                    got.iter().any(|id| {
                        events
                            .get(id.as_str())
                            .is_some_and(|e| !is_authorized(&q.asked_by, &e.scope))
                    })
                });
                out.leaver_leaks.add(leaked);
            }
        }
    }
    if counted > 0 {
        out.tokens_per_question = Some(tokens as f64 / counted as f64);
    }
    out
}

/// Each non-empty team tally as a named rate with its interval, in the order
/// the report lists them. An empty tally is left out, not shown as zero.
pub fn team_stats(scores: &TeamScores, seed: u64) -> Vec<NamedStat> {
    let rows: [(&str, &Tally); 9] = [
        ("supersession correctness", &scores.supersession_correctness),
        ("control: older fact stands", &scores.control_keep),
        ("control: point in time", &scores.control_asof),
        ("provenance accuracy", &scores.provenance_accuracy),
        ("lineage edge recall", &scores.lineage_edge_recall),
        ("lineage chain complete", &scores.lineage_chain_complete),
        ("lineage decoy hit", &scores.lineage_decoy_hit),
        ("leaver recall", &scores.leaver_recall),
        ("leaver leak", &scores.leaver_leaks),
    ];
    rows.iter()
        .filter(|(_, t)| t.n > 0)
        .map(|(name, t)| {
            let events: Vec<bool> = (0..t.n).map(|i| i < t.hits).collect();
            NamedStat {
                name: (*name).to_string(),
                stat: Stat::from_events(&events, seed),
            }
        })
        .collect()
}

/// The printed form of `team_stats`, one line per metric.
pub fn team_lines(stats: &[NamedStat]) -> Vec<String> {
    stats
        .iter()
        .map(|t| {
            format!(
                "  team {}: {}/{} = {:.2}",
                t.name,
                t.stat.events.unwrap_or(0),
                t.stat.n,
                t.stat.estimate
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::team::generate::{generate_team, TeamConfig};
    use crate::team::TeamBatch;

    fn result(question_id: &str, retrieved: Vec<String>) -> QuestionResult {
        QuestionResult {
            question_id: question_id.to_string(),
            retrieved_ids: retrieved,
            recall_at_k: 0.0,
            tokens_injected: 10,
            scored_candidates: vec![],
            gap_disclosure: None,
        }
    }

    fn batch() -> TeamBatch {
        generate_team(&TeamConfig::first_batch(0))
    }

    /// What each item's events look like to a retriever, newest first.
    fn ranked(batch: &TeamBatch, newest_first: bool) -> Vec<QuestionResult> {
        let at: HashMap<&str, _> = batch
            .stream
            .events()
            .map(|e| (e.id.as_str(), e.timestamp))
            .collect();
        batch
            .items
            .iter()
            .map(|i| {
                let mut ids = i.event_ids.clone();
                ids.sort_by_key(|id| at[id.as_str()]);
                if newest_first {
                    ids.reverse();
                }
                result(&i.question_id, ids)
            })
            .collect()
    }

    /// Gold first, then everything else: the best a retriever could do
    /// without leaking.
    fn oracle(batch: &TeamBatch) -> Vec<QuestionResult> {
        batch
            .items
            .iter()
            .map(|i| {
                let mut ids = i.gold_evidence.clone();
                let extra: Vec<String> = i.hard_negatives.to_vec();
                ids.extend(extra);
                result(&i.question_id, ids)
            })
            .collect()
    }

    #[test]
    fn an_oracle_scores_perfectly_on_every_metric_and_leaks_nothing() {
        let b = batch();
        let s = score_team(&b.items, &b.stream, &oracle(&b));
        for t in [
            s.supersession_correctness,
            s.control_keep,
            s.control_asof,
            s.provenance_accuracy,
            s.lineage_edge_recall,
            s.lineage_chain_complete,
            s.leaver_recall,
        ] {
            assert!(t.n > 0);
            assert_eq!(t.rate(), Some(1.0));
        }
        assert_eq!(s.leaver_leaks.hits, 0);
        assert_eq!(s.missing, 0);
    }

    #[test]
    fn newest_wins_passes_supersession_and_fails_both_controls() {
        let b = batch();
        let s = score_team(&b.items, &b.stream, &ranked(&b, true));
        assert_eq!(s.supersession_correctness.rate(), Some(1.0));
        assert_eq!(s.control_keep.rate(), Some(0.0));
        assert_eq!(s.control_asof.rate(), Some(0.0));
        assert_eq!(s.control_accuracy().rate(), Some(0.0));
        // The newest statement is also a restatement, not the origin.
        assert_eq!(s.provenance_accuracy.rate(), Some(0.0));
    }

    #[test]
    fn oldest_wins_passes_the_controls_and_fails_supersession() {
        let b = batch();
        let s = score_team(&b.items, &b.stream, &ranked(&b, false));
        assert_eq!(s.supersession_correctness.rate(), Some(0.0));
        assert_eq!(s.control_asof.rate(), Some(1.0));
        assert_eq!(s.provenance_accuracy.rate(), Some(1.0));
    }

    #[test]
    fn a_missing_gold_event_is_wrong_even_with_no_negative_returned() {
        let b = batch();
        let item = b
            .items
            .iter()
            .find(|i| i.scenario == Scenario::Supersede)
            .unwrap();
        let s = score_team(
            std::slice::from_ref(item),
            &b.stream,
            &[result(&item.question_id, vec![])],
        );
        assert_eq!(s.supersession_correctness, Tally { hits: 0, n: 1 });
    }

    #[test]
    fn a_returned_negative_below_gold_is_still_correct() {
        let b = batch();
        let item = b
            .items
            .iter()
            .find(|i| i.scenario == Scenario::Supersede)
            .unwrap();
        let ids = vec![
            item.gold_evidence[0].clone(),
            item.hard_negatives[0].clone(),
        ];
        let s = score_team(
            std::slice::from_ref(item),
            &b.stream,
            &[result(&item.question_id, ids)],
        );
        assert_eq!(s.supersession_correctness, Tally { hits: 1, n: 1 });
    }

    #[test]
    fn lineage_counts_an_edge_only_when_both_ends_are_returned() {
        let b = batch();
        let item = b
            .items
            .iter()
            .find(|i| i.scenario == Scenario::Lineage)
            .unwrap();
        let first = &item.gold_edges[0];
        let ids = vec![first.from.clone(), first.to.clone()];
        let s = score_team(
            std::slice::from_ref(item),
            &b.stream,
            &[result(&item.question_id, ids)],
        );
        assert_eq!(s.lineage_edge_recall.hits, 1);
        assert_eq!(s.lineage_edge_recall.n, item.gold_edges.len());
        assert_eq!(s.lineage_chain_complete, Tally { hits: 0, n: 1 });
        assert_eq!(s.lineage_decoy_hit, Tally { hits: 0, n: 1 });
    }

    #[test]
    fn lineage_decoy_is_flagged_when_the_other_projects_decision_comes_back() {
        let b = batch();
        let item = b
            .items
            .iter()
            .find(|i| i.scenario == Scenario::Lineage)
            .unwrap();
        let ids = vec![item.hard_negatives[0].clone()];
        let s = score_team(
            std::slice::from_ref(item),
            &b.stream,
            &[result(&item.question_id, ids)],
        );
        assert_eq!(s.lineage_decoy_hit.hits, 1);
    }

    #[test]
    fn a_leaver_item_flags_the_private_note_for_another_asker() {
        let b = batch();
        let item = b
            .items
            .iter()
            .find(|i| i.scenario == Scenario::Leaver)
            .unwrap();
        let all = item.event_ids.clone();
        let s = score_team(
            std::slice::from_ref(item),
            &b.stream,
            &[result(&item.question_id, all)],
        );
        assert_eq!(s.leaver_leaks, Tally { hits: 1, n: 1 });
        assert_eq!(s.leaver_recall, Tally { hits: 1, n: 1 });
    }

    #[test]
    fn unanswered_items_are_counted_as_missing_not_skipped_silently() {
        let b = batch();
        let s = score_team(&b.items, &b.stream, &[]);
        assert_eq!(s.missing, b.items.len());
        assert_eq!(s.supersession_correctness.n, 0);
        assert_eq!(s.tokens_per_question, None);
    }

    #[test]
    fn team_stats_skip_an_empty_tally_and_carry_the_count() {
        let scores = TeamScores {
            supersession_correctness: Tally { hits: 3, n: 4 },
            leaver_leaks: Tally { hits: 0, n: 6 },
            ..TeamScores::default()
        };

        let stats = team_stats(&scores, 0);

        let names: Vec<&str> = stats.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["supersession correctness", "leaver leak"]);
        assert_eq!((stats[0].stat.events, stats[0].stat.n), (Some(3), 4));
        assert_eq!((stats[1].stat.events, stats[1].stat.n), (Some(0), 6));
        assert_eq!(
            team_lines(&stats),
            vec![
                "  team supersession correctness: 3/4 = 0.75".to_string(),
                "  team leaver leak: 0/6 = 0.00".to_string(),
            ]
        );
    }
}

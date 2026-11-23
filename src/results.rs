//! Machine-readable results of a replay: what `report` reads and what the
//! charts are drawn from. Every interval comes from `stats.rs` with the run's
//! seed, the same calls the printed report makes, so a chart cannot disagree
//! with a table.

use crate::adapter::{
    calibration_pairs, category_of, leak_trial_flags, leak_trials, QuestionResult,
};
use crate::stats::{bootstrap_ci_95, bootstrap_statistic_ci_95};
use crate::stream::Stream;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

/// Below this many questions a category gets no interval and no point: a
/// bootstrap over so few questions says nothing.
pub const MIN_N_FOR_CI: usize = 10;

/// A rate or a mean with its sample size. `ci` is `None` when the bootstrap
/// saw no variance. `upper_bound` is the 3/n bound, set only when no event
/// was observed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stat {
    pub n: usize,
    pub events: Option<usize>,
    pub estimate: f64,
    pub ci: Option<[f64; 2]>,
    pub upper_bound: Option<f64>,
}

impl Stat {
    pub fn from_values(values: &[f64], seed: u64) -> Stat {
        let interval = bootstrap_ci_95(values, seed);
        Stat {
            n: interval.n,
            events: None,
            estimate: interval.estimate,
            ci: (interval.lo != interval.hi).then_some([interval.lo, interval.hi]),
            upper_bound: None,
        }
    }

    /// A rate over yes/no outcomes. With no event the answer is the 3/n
    /// bound, not a CI that would collapse to zero width.
    pub fn from_events(events: &[bool], seed: u64) -> Stat {
        let n = events.len();
        let hits = events.iter().filter(|&&e| e).count();
        if hits == 0 {
            return Stat {
                n,
                events: Some(0),
                estimate: 0.0,
                ci: None,
                upper_bound: Some(crate::scoring::zero_leak_upper_bound_95(n).min(1.0)),
            };
        }
        let values: Vec<f64> = events.iter().map(|&e| f64::from(u8::from(e))).collect();
        Stat {
            events: Some(hits),
            ..Stat::from_values(&values, seed)
        }
    }

    /// The printed form, identical to `Interval::display` for a mean.
    pub fn display(&self) -> String {
        match self.ci {
            Some([lo, hi]) => format!(
                "{:.4} (95% CI {:.4}-{:.4}, n={})",
                self.estimate, lo, hi, self.n
            ),
            None => format!(
                "{:.4} (n={}, no variance observed: CI undefined)",
                self.estimate, self.n
            ),
        }
    }

    pub fn too_small(&self) -> bool {
        self.n < MIN_N_FOR_CI
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedStat {
    pub name: String,
    #[serde(flatten)]
    pub stat: Stat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionRow {
    pub id: String,
    pub category: Option<String>,
    pub recall: f64,
    /// Absent when the file was rebuilt from a replicate log, which keeps no
    /// retrieved content.
    #[serde(default)]
    pub tokens: Option<usize>,
    /// Earlier asks of this question's gold event (use-learning streams
    /// only); the x axis of the use curves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BinRow {
    pub predicted_mean: f64,
    pub observed_frequency: f64,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub brier: f64,
    pub ece: Stat,
    pub bins: Vec<BinRow>,
    /// "derived": a probability worked out from the system's ranking, not one
    /// it stated. Native calibration is never claimed here.
    pub basis: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CyclePoint {
    pub cycle: usize,
    pub mean_tokens: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplicateRow {
    pub index: usize,
    pub questions: usize,
    pub fully_recalled: usize,
    /// Missing for a replicate loaded from the replicate log, which keeps
    /// pass/fail only.
    pub mean_recall: Option<f64>,
    pub leaked_trials: Option<usize>,
    pub leak_trials: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemResults {
    pub system: String,
    pub questions: Vec<QuestionRow>,
    pub recall: Option<Stat>,
    pub tokens: Option<Stat>,
    pub categories: Vec<NamedStat>,
    pub leaks: Vec<NamedStat>,
    pub flips: Option<Stat>,
    pub calibration: Option<Calibration>,
    pub tokens_by_cycle: Vec<CyclePoint>,
    pub replicates: Vec<ReplicateRow>,
    /// Track C team metrics (rate and interval each), empty on any other
    /// stream. An older results file without the field loads as empty.
    #[serde(default)]
    pub team: Vec<NamedStat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotRun {
    pub category: String,
    pub trials: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunInfo {
    /// File name only, never a path.
    pub stream_file: String,
    pub stream_sha256: String,
    pub seed: u64,
    pub top_k: usize,
    pub replicates: usize,
    pub questions: usize,
    pub cognoscenti_commit: Option<String>,
    pub skip_user_scope: bool,
    pub not_run: Vec<NotRun>,
    pub limits: Vec<String>,
    /// Retrieval mode and params the run asked the server for, and the
    /// `retrieval` object the server echoed. Absent for a run with the
    /// deployment's defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval: Option<RetrievalSetting>,
    /// Replicates whose results were loaded from a replicate log instead of
    /// measured by this file's run: only pass or fail per question survives.
    /// Empty for a live run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from_replicate_log: Vec<usize>,
}

/// A non-default retrieval setting. Part of a cohort's identity: the cohort
/// name carries it, so a run with `activationWeightScale: 0.25` is never
/// shown under, or merged with, the plain label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalSetting {
    pub mode: Option<String>,
    pub params: Option<serde_json::Value>,
    /// What the server reported using on the first retrieve.
    pub echoed: Option<serde_json::Value>,
}

impl RetrievalSetting {
    /// `aggr · activationWeightScale=0.25, alpha=2`
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(mode) = &self.mode {
            parts.push(mode.clone());
        }
        if let Some(params) = self.params.as_ref().and_then(|p| p.as_object()) {
            let pairs: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            if !pairs.is_empty() {
                parts.push(pairs.join(", "));
            }
        }
        parts.join(" · ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunFile {
    pub schema: u32,
    pub label: Option<String>,
    pub run: RunInfo,
    pub systems: Vec<SystemResults>,
}

/// What every report states about how far its numbers reach.
pub fn standard_limits() -> Vec<String> {
    vec![
        "Tokens are whitespace-separated words, not a model tokenizer's tokens.".to_string(),
        "Tokens count what a question would inject (read path only); what ingestion costs is not measured.".to_string(),
        "Each interval is a 95% percentile bootstrap over questions; a category under 10 questions gets no interval.".to_string(),
        "Calibration is derived from each system's ranking, not stated by the system.".to_string(),
    ]
}

impl RunFile {
    pub fn new(label: Option<String>, run: RunInfo) -> RunFile {
        RunFile {
            schema: SCHEMA,
            label: label
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty()),
            run,
            systems: Vec::new(),
        }
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
    }

    pub fn load(path: &std::path::Path) -> Result<RunFile> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let file: RunFile = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a results file", path.display()))?;
        if file.schema != SCHEMA {
            bail!(
                "{} has schema {}, this build reads schema {SCHEMA}",
                path.display(),
                file.schema
            );
        }
        Ok(file)
    }

    /// The name a cohort goes by in a report.
    pub fn cohort_name(&self) -> String {
        let label = self.label.clone().unwrap_or_else(|| "no label".to_string());
        match self.run.retrieval.as_ref().map(RetrievalSetting::describe) {
            Some(setting) if !setting.is_empty() => format!("{label} [{setting}]"),
            _ => label,
        }
    }
}

impl SystemResults {
    pub fn empty(system: &str) -> SystemResults {
        SystemResults {
            system: system.to_string(),
            questions: Vec::new(),
            recall: None,
            tokens: None,
            categories: Vec::new(),
            leaks: Vec::new(),
            flips: None,
            calibration: None,
            tokens_by_cycle: Vec::new(),
            replicates: Vec::new(),
            team: Vec::new(),
        }
    }

    /// Overall and per-category recall, tokens and leaks from one replay.
    pub fn from_replay(
        system: &str,
        stream: &Stream,
        results: &[QuestionResult],
        seed: u64,
    ) -> SystemResults {
        let mut out = SystemResults::empty(system);
        let counts = crate::use_learning::use_counts(stream);
        out.questions = results
            .iter()
            .map(|r| QuestionRow {
                id: r.question_id.clone(),
                category: category_of(&r.question_id).map(str::to_string),
                recall: r.recall_at_k,
                tokens: Some(r.tokens_injected),
                use_count: use_count_of(&counts, &r.question_id),
            })
            .collect();
        let recalls: Vec<f64> = results.iter().map(|r| r.recall_at_k).collect();
        let tokens: Vec<f64> = results.iter().map(|r| r.tokens_injected as f64).collect();
        out.recall = Some(Stat::from_values(&recalls, seed));
        out.tokens = Some(Stat::from_values(&tokens, seed));

        let rows: Vec<(&str, f64)> = results
            .iter()
            .map(|r| (r.question_id.as_str(), r.recall_at_k))
            .collect();
        out.categories = category_stats(&rows, seed);

        out.leaks = leak_stats(stream, results, seed);
        out
    }
}

/// A question's use count, for the use-learning categories only: the
/// number means nothing on any other stream.
pub(crate) fn use_count_of(
    counts: &std::collections::HashMap<String, usize>,
    question_id: &str,
) -> Option<usize> {
    let category = category_of(question_id)?;
    crate::use_learning::CATEGORIES
        .contains(&category)
        .then(|| counts.get(question_id).copied())
        .flatten()
}

/// Recall per generated category (`(question id, recall)` rows in stream
/// order), categories in first-seen order. Ids that name no category are left
/// out, so a hand-made dataset is never regrouped.
pub fn category_stats(rows: &[(&str, f64)], seed: u64) -> Vec<NamedStat> {
    let mut categories: Vec<&str> = Vec::new();
    for (id, _) in rows {
        if let Some(category) = category_of(id) {
            if !categories.contains(&category) {
                categories.push(category);
            }
        }
    }
    categories
        .iter()
        .map(|category| {
            let recalls: Vec<f64> = rows
                .iter()
                .filter(|(id, _)| category_of(id) == Some(*category))
                .map(|(_, recall)| *recall)
                .collect();
            NamedStat {
                name: category.to_string(),
                stat: Stat::from_values(&recalls, seed),
            }
        })
        .collect()
}

/// Leak rows as `leak_lines` reports them: each category holding a private
/// memory on its own, or one pooled row for a stream that is not generated.
pub fn leak_stats(stream: &Stream, results: &[QuestionResult], seed: u64) -> Vec<NamedStat> {
    let row = |name: &str, subset: &[QuestionResult]| -> Option<NamedStat> {
        let trials = leak_trials(stream, subset);
        (!trials.is_empty()).then(|| NamedStat {
            name: name.to_string(),
            stat: Stat::from_events(&leak_trial_flags(&trials), seed),
        })
    };
    let by_category: Vec<NamedStat> = ["scope", "agent"]
        .iter()
        .filter_map(|category| {
            let subset: Vec<QuestionResult> = results
                .iter()
                .filter(|r| category_of(&r.question_id) == Some(*category))
                .cloned()
                .collect();
            row(category, &subset)
        })
        .collect();
    if by_category.is_empty() {
        row("all", results).into_iter().collect()
    } else {
        by_category
    }
}

/// Brier score, ECE with its CI, and the reliability bins, from probabilities
/// derived from each system's ranking. `None` when nothing was retrieved.
pub fn calibration_summary(
    stream: &Stream,
    results: &[QuestionResult],
    bins: usize,
    seed: u64,
) -> Option<Calibration> {
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
        let derived = crate::calibration::derive_probabilities_from_scores(&raw, bins);
        derived
            .into_iter()
            .zip(raw.iter().map(|&(_, observed)| observed))
            .collect()
    };
    let all: Vec<usize> = (0..pairs_by_question.len()).collect();
    let all_pairs = derived_pairs(&all);
    if all_pairs.is_empty() {
        return None;
    }
    // ECE is resampled over questions, not candidate rows: a question's
    // candidates are not independent of each other.
    let ece = bootstrap_statistic_ci_95(all.len(), seed, |idx| {
        let pairs = derived_pairs(idx);
        if pairs.is_empty() {
            0.0
        } else {
            crate::calibration::expected_calibration_error(&pairs, bins)
        }
    });
    Some(Calibration {
        brier: crate::calibration::brier_score(&all_pairs),
        ece: Stat {
            n: ece.n,
            events: None,
            estimate: ece.estimate,
            ci: (ece.lo != ece.hi).then_some([ece.lo, ece.hi]),
            upper_bound: None,
        },
        bins: crate::calibration::reliability_curve(&all_pairs, bins)
            .into_iter()
            .map(|b| BinRow {
                predicted_mean: b.predicted_mean,
                observed_frequency: b.observed_frequency,
                count: b.count,
            })
            .collect(),
        basis: "derived".to_string(),
    })
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn opt(value: Option<f64>) -> String {
    value.map_or(String::new(), |v| format!("{v:.6}"))
}

/// One long-format CSV of every number a chart plots, so a plot can be
/// regenerated from data without reading the JSON.
pub fn to_csv(cohorts: &[RunFile]) -> String {
    let mut out =
        String::from("cohort,system,section,name,n,events,estimate,ci_lo,ci_hi,upper_bound_3n\n");
    for cohort in cohorts {
        let name = cohort.cohort_name();
        for system in &cohort.systems {
            let mut row = |section: &str, label: &str, stat: &Stat| {
                out.push_str(&format!(
                    "{},{},{},{},{},{},{:.6},{},{},{}\n",
                    csv_field(&name),
                    csv_field(&system.system),
                    section,
                    csv_field(label),
                    stat.n,
                    stat.events.map_or(String::new(), |e| e.to_string()),
                    stat.estimate,
                    opt(stat.ci.map(|c| c[0])),
                    opt(stat.ci.map(|c| c[1])),
                    opt(stat.upper_bound),
                ));
            };
            if let Some(stat) = &system.recall {
                row("recall", "all", stat);
            }
            if let Some(stat) = &system.tokens {
                row("tokens", "all", stat);
            }
            for c in &system.categories {
                row("category_recall", &c.name, &c.stat);
            }
            for l in &system.leaks {
                row("leak", &l.name, &l.stat);
            }
            for t in &system.team {
                row("team", &t.name, &t.stat);
            }
            if let Some(stat) = &system.flips {
                row("flip", "all", stat);
            }
            if let Some(cal) = &system.calibration {
                row("ece", "all", &cal.ece);
                let brier = Stat {
                    n: cal.ece.n,
                    events: None,
                    estimate: cal.brier,
                    ci: None,
                    upper_bound: None,
                };
                row("brier", "all", &brier);
                for (i, bin) in cal.bins.iter().enumerate() {
                    let stat = Stat {
                        n: bin.count,
                        events: None,
                        estimate: bin.observed_frequency,
                        ci: None,
                        upper_bound: None,
                    };
                    row(
                        "reliability_bin",
                        &format!("bin {} predicted {:.4}", i + 1, bin.predicted_mean),
                        &stat,
                    );
                }
            }
            for point in &system.tokens_by_cycle {
                let stat = Stat {
                    n: 0,
                    events: None,
                    estimate: point.mean_tokens,
                    ci: None,
                    upper_bound: None,
                };
                row("tokens_by_cycle", &format!("cycle {}", point.cycle), &stat);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::describe_event_rate;

    fn result(id: &str, recall: f64, tokens: usize) -> QuestionResult {
        QuestionResult {
            question_id: id.to_string(),
            retrieved_ids: vec![],
            recall_at_k: recall,
            tokens_injected: tokens,
            scored_candidates: vec![],
            gap_disclosure: None,
        }
    }

    fn empty_stream() -> Stream {
        Stream { items: vec![] }
    }

    fn info() -> RunInfo {
        RunInfo {
            stream_file: "stream.jsonl".into(),
            stream_sha256: "abc".into(),
            seed: 0,
            top_k: 3,
            replicates: 1,
            questions: 4,
            cognoscenti_commit: None,
            skip_user_scope: false,
            not_run: vec![],
            limits: standard_limits(),
            retrieval: None,
            from_replicate_log: vec![],
        }
    }

    #[test]
    fn a_mean_stat_carries_n_and_the_same_interval_the_printed_report_gives() {
        let values = [1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0];
        let stat = Stat::from_values(&values, 7);
        let printed = bootstrap_ci_95(&values, 7);
        assert_eq!(stat.n, 12);
        assert_eq!(stat.estimate, printed.estimate);
        assert_eq!(stat.ci, Some([printed.lo, printed.hi]));
        assert!(printed.display().contains(&format!("{:.4}", stat.estimate)));
    }

    #[test]
    fn display_matches_the_interval_display_the_report_prints() {
        let values = [1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0];
        assert_eq!(
            Stat::from_values(&values, 7).display(),
            bootstrap_ci_95(&values, 7).display()
        );
        assert_eq!(
            Stat::from_values(&[1.0; 12], 7).display(),
            bootstrap_ci_95(&[1.0; 12], 7).display()
        );
    }

    #[test]
    fn a_mean_with_no_variance_has_no_interval() {
        let stat = Stat::from_values(&[1.0; 12], 0);
        assert_eq!(stat.estimate, 1.0);
        assert_eq!(stat.ci, None);
    }

    #[test]
    fn an_event_rate_with_no_events_reports_the_three_over_n_bound() {
        let stat = Stat::from_events(&[false; 60], 0);
        assert_eq!((stat.n, stat.events, stat.estimate), (60, Some(0), 0.0));
        assert_eq!(stat.ci, None);
        assert!((stat.upper_bound.unwrap() - 0.05).abs() < 1e-12);
        assert!(describe_event_rate(&[false; 60], 0).contains("0.0500 (3/n)"));
    }

    #[test]
    fn the_bound_never_exceeds_one() {
        assert_eq!(Stat::from_events(&[false; 2], 0).upper_bound, Some(1.0));
    }

    #[test]
    fn an_event_rate_with_events_has_a_bootstrap_interval_and_no_bound() {
        let events: Vec<bool> = (0..40).map(|i| i % 4 == 0).collect();
        let stat = Stat::from_events(&events, 3);
        assert_eq!((stat.n, stat.events), (40, Some(10)));
        assert!((stat.estimate - 0.25).abs() < 1e-12);
        assert!(stat.ci.is_some());
        assert_eq!(stat.upper_bound, None);
    }

    #[test]
    fn a_stat_is_too_small_below_ten_questions() {
        assert!(Stat::from_values(&[1.0; 9], 0).too_small());
        assert!(!Stat::from_values(&[1.0; 10], 0).too_small());
    }

    #[test]
    fn from_replay_groups_recall_by_generated_category_and_leaves_other_ids_ungrouped() {
        let mut results = Vec::new();
        for i in 0..12 {
            results.push(result(&format!("scope-{i:03}-q"), 1.0, 10));
        }
        for i in 0..12 {
            results.push(result(&format!("owner-{i:03}-q"), 0.0, 20));
        }
        results.push(result("hand-made-q", 1.0, 5));
        let out = SystemResults::from_replay("achiral", &empty_stream(), &results, 0);
        let names: Vec<&str> = out.categories.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["scope", "owner"]);
        assert_eq!(out.categories[0].stat.n, 12);
        assert_eq!(out.categories[0].stat.estimate, 1.0);
        assert_eq!(out.categories[1].stat.estimate, 0.0);
        assert_eq!(out.recall.as_ref().unwrap().n, 25);
        assert_eq!(out.questions.len(), 25);
        assert_eq!(out.questions[24].category, None);
        assert_eq!(out.questions[0].tokens, Some(10));
    }

    #[test]
    fn category_rows_match_the_printed_category_lines() {
        let mut results = Vec::new();
        for i in 0..15 {
            results.push(result(
                &format!("retrieve-{i:03}-q"),
                if i % 3 == 0 { 0.0 } else { 1.0 },
                1,
            ));
        }
        let out = SystemResults::from_replay("bm25", &empty_stream(), &results, 5);
        let printed = crate::adapter::category_lines(&results, 3, 5);
        let stat = &out.categories[0].stat;
        let [lo, hi] = stat.ci.unwrap();
        assert!(printed[0].contains(&format!("{:.4}", stat.estimate)));
        assert!(printed[0].contains(&format!("{lo:.4}-{hi:.4}")));
        assert!(printed[0].contains(&format!("n={}", stat.n)));
    }

    #[test]
    fn a_run_file_round_trips_and_rejects_another_schema() {
        let dir = std::env::temp_dir().join(format!("cogn-results-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("r.json");
        let mut file = RunFile::new(Some("  noise 0.2 ".into()), info());
        file.systems.push(SystemResults::empty("achiral"));
        file.save(&path).unwrap();
        let back = RunFile::load(&path).unwrap();
        assert_eq!(back, file);
        assert_eq!(back.label.as_deref(), Some("noise 0.2"));
        let mut other = file.clone();
        other.schema = 99;
        other.save(&path).unwrap();
        assert!(RunFile::load(&path)
            .unwrap_err()
            .to_string()
            .contains("schema 99"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn aggr_quarter() -> RetrievalSetting {
        RetrievalSetting {
            mode: Some("aggr".into()),
            params: Some(serde_json::json!({"activationWeightScale": 0.25})),
            echoed: Some(serde_json::json!({"mode": "aggr", "activationWeightScale": 0.25})),
        }
    }

    #[test]
    fn a_non_default_retrieval_setting_is_part_of_the_cohort_name() {
        let mut run = info();
        run.retrieval = Some(aggr_quarter());
        let labelled = RunFile::new(Some("cohort-a".into()), run.clone());
        assert_eq!(
            labelled.cohort_name(),
            "cohort-a [aggr · activationWeightScale=0.25]"
        );
        let unlabelled = RunFile::new(None, run);
        assert_eq!(
            unlabelled.cohort_name(),
            "no label [aggr · activationWeightScale=0.25]"
        );
        let plain = RunFile::new(Some("cohort-a".into()), info());
        assert_eq!(plain.cohort_name(), "cohort-a");
        assert_ne!(plain.cohort_name(), labelled.cohort_name());
    }

    #[test]
    fn the_retrieval_setting_survives_the_file_and_older_files_without_it_load() {
        let dir = std::env::temp_dir().join(format!("cogn-retr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut run = info();
        run.retrieval = Some(aggr_quarter());
        let file = RunFile::new(None, run);
        let path = dir.join("r.json");
        file.save(&path).unwrap();
        assert_eq!(RunFile::load(&path).unwrap(), file);
        // a default run writes no retrieval key; a file lacking it still loads
        let plain = RunFile::new(None, info());
        plain.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("retrieval"), "{text}");
        assert_eq!(RunFile::load(&path).unwrap().run.retrieval, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_blank_label_is_no_label() {
        assert_eq!(RunFile::new(Some("   ".into()), info()).label, None);
        assert_eq!(RunFile::new(None, info()).cohort_name(), "no label");
    }

    #[test]
    fn the_csv_has_one_row_per_plotted_number_and_quotes_commas() {
        let mut file = RunFile::new(Some("a, b".into()), info());
        let mut system = SystemResults::empty("achiral");
        system.recall = Some(Stat::from_values(
            &[1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0],
            0,
        ));
        system.leaks = vec![NamedStat {
            name: "scope".into(),
            stat: Stat::from_events(&[false; 71], 0),
        }];
        system.tokens_by_cycle = vec![CyclePoint {
            cycle: 10,
            mean_tokens: 31.5,
        }];
        file.systems.push(system);
        let csv = to_csv(&[file]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(
            lines[0],
            "cohort,system,section,name,n,events,estimate,ci_lo,ci_hi,upper_bound_3n"
        );
        assert_eq!(lines.len(), 4);
        assert!(lines[1].starts_with("\"a, b\",achiral,recall,all,10,,0.700000,"));
        assert!(lines[2].starts_with("\"a, b\",achiral,leak,scope,71,0,0.000000,,,0.042254"));
        assert!(lines[3].contains("tokens_by_cycle,cycle 10,0,,31.500000"));
    }

    #[test]
    fn leaks_are_reported_per_private_category_and_count_a_leak_from_either() {
        use crate::stream::{
            Actor, ActorKind, Scope, ScopeKind, StreamEvent, StreamItem, StreamQuestion,
        };
        let user = Actor {
            id: "u".into(),
            kind: ActorKind::User,
        };
        let event = |id: &str, kind: ScopeKind, scope_id: &str| {
            StreamItem::Event(StreamEvent {
                id: id.into(),
                content: "x".into(),
                timestamp: "2026-01-01T00:00:00Z".parse().unwrap(),
                actor: Actor {
                    id: "a1".into(),
                    kind: ActorKind::Agent,
                },
                scope: Scope {
                    kind,
                    id: scope_id.into(),
                },
                source: "t".into(),
                supersedes: None,
                entities: vec![],
            })
        };
        let question = |id: &str| {
            StreamItem::Question(StreamQuestion {
                id: id.into(),
                query: "q".into(),
                asked_at: "2026-01-15T00:00:00Z".parse().unwrap(),
                asked_by: user.clone(),
                gold_event_ids: vec![],
                gold_answer: None,
                validity: None,
                expected_gap_entity_ids: vec![],
            })
        };
        let stream = Stream {
            items: vec![
                event("agent-000-private", ScopeKind::Agent, "a1"),
                event("scope-000-org", ScopeKind::Org, "acme"),
                question("agent-000-q"),
                question("scope-000-q"),
            ],
        };
        let mut leaked = result("agent-000-q", 0.0, 1);
        leaked.retrieved_ids = vec!["agent-000-private".into()];
        let clean = result("scope-000-q", 0.0, 1);
        let rows = leak_stats(&stream, &[leaked, clean], 0);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["scope", "agent"]);
        assert_eq!(rows[0].stat.events, Some(0));
        assert_eq!(rows[1].stat.events, Some(1));
    }

    fn use_stream() -> Stream {
        let q = |id: &str, gold: &str| {
            format!(
                r#"{{"kind":"question","id":"{id}","query":"x","asked_at":"2026-01-02T00:00:00Z","asked_by":{{"id":"a","kind":"user"}},"gold_event_ids":[{gold}]}}"#
            )
        };
        let event = r#"{"kind":"event","id":"g1","content":"c","timestamp":"2026-01-01T00:00:00Z","actor":{"id":"a","kind":"user"},"scope":{"kind":"org","id":"o"},"source":"t"}"#;
        Stream::from_jsonl(
            &[
                event.to_string(),
                q("pop-000-q", r#""g1""#),
                q("pop-001-q", r#""g1""#),
                q("scope-000-q", ""),
                q("hand-made-q", r#""g1""#),
            ]
            .join("\n"),
        )
        .unwrap()
    }

    #[test]
    fn from_replay_records_the_use_count_of_use_learning_questions_only() {
        let results = vec![
            result("pop-000-q", 1.0, 1),
            result("pop-001-q", 1.0, 1),
            result("scope-000-q", 1.0, 1),
            result("hand-made-q", 1.0, 1),
        ];
        let out = SystemResults::from_replay("achiral", &use_stream(), &results, 0);
        let counts: Vec<Option<usize>> = out.questions.iter().map(|r| r.use_count).collect();
        assert_eq!(counts, vec![Some(0), Some(1), None, None]);
    }

    #[test]
    fn a_row_without_a_use_count_loads_from_an_older_file() {
        let row: QuestionRow =
            serde_json::from_str(r#"{"id":"pop-000-q","category":"pop","recall":1.0}"#).unwrap();
        assert_eq!(row.use_count, None);
    }

    #[test]
    fn a_results_file_written_before_team_metrics_loads_with_none() {
        let mut system = serde_json::to_value(SystemResults::empty("bm25")).unwrap();
        system.as_object_mut().unwrap().remove("team");
        let back: SystemResults = serde_json::from_value(system).unwrap();
        assert!(back.team.is_empty());
    }
}

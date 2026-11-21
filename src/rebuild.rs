//! Rebuilds a cohort's results file from a replicate log and the stream, for
//! a run whose own results were never written (a process that crashed after
//! replicate 0, resumed from the log). The log keeps one pass or fail per kept
//! question per replicate and nothing else, so only what that can honestly
//! give is filled in: per-question recall, pooled and per-category recall,
//! flips, and (with the printed summary lines) leak counts per replicate.
//! Tokens injected, calibration and the per-category leak table stay absent.

use crate::adapter::{category_of, drop_user_scope_trials, not_run_by_category};
use crate::replicate_log::{parse_log, RunIdentity};
use crate::results::{
    category_stats, standard_limits, QuestionRow, ReplicateRow, RunFile, RunInfo, Stat,
    SystemResults,
};
use crate::stream::Stream;
use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(name = "cognoscenti rebuild-results")]
pub struct RebuildArgs {
    /// The replicate log (`--replicate-log`) of the run.
    #[clap(long)]
    pub replicate_log: PathBuf,

    /// The stream the run replayed; its SHA-256 must match the log's.
    #[clap(long)]
    pub stream: PathBuf,

    /// The run used `--skip-user-scope`.
    #[clap(long)]
    pub skip_user_scope: bool,

    #[clap(long, default_value_t = 3)]
    pub top_k: usize,

    #[clap(long, default_value_t = 0)]
    pub seed: u64,

    #[clap(long)]
    pub label: Option<String>,

    #[clap(long, default_value = "achiral")]
    pub system: String,

    /// Output captured from the run (any number of files); its printed
    /// `[replicate N] ...` lines supply leak counts and are checked against
    /// the log.
    #[clap(long, num_args = 0..)]
    pub summary_lines: Vec<PathBuf>,

    /// The results file to write.
    #[clap(long)]
    pub out: PathBuf,
}

pub struct RebuildInput<'a> {
    pub log_text: &'a str,
    pub stream: &'a Stream,
    pub stream_file: &'a str,
    pub stream_sha256: &'a str,
    pub skip_user_scope: bool,
    pub top_k: usize,
    pub seed: u64,
    pub system: &'a str,
    pub label: Option<String>,
    pub summary_text: &'a str,
}

/// One printed `[replicate N] ...` line (`replicate_log::summary_line`).
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryLine {
    pub replicate: usize,
    pub top_k: usize,
    pub mean_recall: f64,
    pub passed: usize,
    pub questions: usize,
    pub leaked: usize,
    pub leak_trials: usize,
}

pub fn run_rebuild(args: &RebuildArgs) -> Result<PathBuf> {
    let path = args.stream.to_string_lossy().into_owned();
    let stream = Stream::load(&path)?;
    let sha = crate::manifest::sha256_file(&path)?;
    let log_text = std::fs::read_to_string(&args.replicate_log)
        .with_context(|| format!("reading {}", args.replicate_log.display()))?;
    let mut summary_text = String::new();
    for file in &args.summary_lines {
        summary_text.push_str(
            &std::fs::read_to_string(file)
                .with_context(|| format!("reading {}", file.display()))?,
        );
        summary_text.push('\n');
    }
    let stream_file = args
        .stream
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let file = rebuild_cohort(&RebuildInput {
        log_text: &log_text,
        stream: &stream,
        stream_file: &stream_file,
        stream_sha256: &sha,
        skip_user_scope: args.skip_user_scope,
        top_k: args.top_k,
        seed: args.seed,
        system: &args.system,
        label: args.label.clone(),
        summary_text: &summary_text,
    })?;
    file.save(&args.out)?;
    Ok(args.out.clone())
}

pub fn parse_summary_lines(text: &str) -> Vec<SummaryLine> {
    text.lines().filter_map(parse_summary_line).collect()
}

fn parse_summary_line(line: &str) -> Option<SummaryLine> {
    let rest = line.strip_prefix("[replicate ")?;
    let (replicate, rest) = rest.split_once("] mean recall@")?;
    let (top_k, rest) = rest.split_once(" = ")?;
    let (mean, rest) = rest.split_once("; questions fully recalled: ")?;
    let (passed, rest) = rest.split_once('/')?;
    let (questions, rest) = rest.split_once("; trials with a leak: ")?;
    let (leaked, leak_trials) = rest.trim().split_once('/')?;
    Some(SummaryLine {
        replicate: replicate.parse().ok()?,
        top_k: top_k.parse().ok()?,
        mean_recall: mean.parse().ok()?,
        passed: passed.parse().ok()?,
        questions: questions.parse().ok()?,
        leaked: leaked.parse().ok()?,
        leak_trials: leak_trials.parse().ok()?,
    })
}

pub fn rebuild_cohort(input: &RebuildInput<'_>) -> Result<RunFile> {
    let kept = if input.skip_user_scope {
        drop_user_scope_trials(input.stream)
    } else {
        (input.stream.clone(), Vec::new())
    };
    let (stream, dropped) = kept;
    let questions: Vec<&crate::stream::StreamQuestion> = stream.questions().collect();
    if let Some(q) = questions.iter().find(|q| q.gold_event_ids.len() > 1) {
        bail!(
            "question {} has {} gold events; a logged pass is recall 1.0 only, so the recall of a question with several gold events cannot be rebuilt",
            q.id,
            q.gold_event_ids.len()
        );
    }

    let identity = RunIdentity {
        system: input.system.to_string(),
        stream_sha256: input.stream_sha256.to_string(),
        top_k: input.top_k,
        seed: input.seed,
    };
    let state = parse_log(input.log_text, &identity)?;
    crate::replicate_log::check_finished_shape(&state, questions.len())?;
    if state.finished.is_empty() {
        bail!("the replicate log holds no finished replicate");
    }
    let indices: Vec<usize> = state.finished.keys().copied().collect();
    if indices != (0..indices.len()).collect::<Vec<_>>() {
        bail!(
            "finished replicates in the log are {indices:?}; expected 0 to {} with none missing",
            indices.len() - 1
        );
    }
    let replicates = indices.len();

    let summaries = checked_summaries(input, &state.finished)?;

    let outcomes: Vec<Vec<bool>> = (0..questions.len())
        .map(|q| indices.iter().map(|r| state.finished[r][q]).collect())
        .collect();
    let counts = crate::use_learning::use_counts(&stream);
    let rows: Vec<QuestionRow> = questions
        .iter()
        .zip(&outcomes)
        .map(|(q, passes)| QuestionRow {
            id: q.id.clone(),
            category: category_of(&q.id).map(str::to_string),
            recall: passes.iter().filter(|p| **p).count() as f64 / replicates as f64,
            tokens: None,
            use_count: crate::results::use_count_of(&counts, &q.id),
        })
        .collect();
    let recalls: Vec<f64> = rows.iter().map(|r| r.recall).collect();
    let by_category: Vec<(&str, f64)> = rows.iter().map(|r| (r.id.as_str(), r.recall)).collect();
    let flipped = crate::stability::flipped_questions(&outcomes);

    let mut system = SystemResults::empty(input.system);
    system.recall = Some(Stat::from_values(&recalls, input.seed));
    system.categories = category_stats(&by_category, input.seed);
    system.flips = Some(Stat::from_events(&flipped, input.seed));
    system.replicates = indices
        .iter()
        .map(|&r| {
            let passes = &state.finished[&r];
            let fully = passes.iter().filter(|p| **p).count();
            let line = summaries.iter().find(|s| s.replicate == r);
            ReplicateRow {
                index: r,
                questions: passes.len(),
                fully_recalled: fully,
                // every question has one gold event, so the mean is passes / n
                mean_recall: Some(fully as f64 / passes.len().max(1) as f64),
                leaked_trials: line.map(|s| s.leaked),
                leak_trials: line.map(|s| s.leak_trials),
            }
        })
        .collect();
    system.questions = rows;

    let loaded = indices
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut limits = standard_limits();
    limits.push(format!(
        "Rebuilt from a replicate log, not written by a live run. Replicates {loaded} were loaded from it. Only pass or fail per question survives, so a question's recall is the share of those replicates that passed it; every question has one gold event, so a pass is recall@{} = 1.0 and the pooled recall is the mean of the per-replicate recalls.",
        input.top_k
    ));
    limits.push(
        "Not available for replicates loaded from a replicate log: tokens injected, calibration and the per-category leak table. Leak counts per replicate come from the run's printed summary lines, where supplied."
            .to_string(),
    );
    let mut file = RunFile::new(
        input.label.clone(),
        RunInfo {
            stream_file: input.stream_file.to_string(),
            stream_sha256: input.stream_sha256.to_string(),
            seed: input.seed,
            top_k: input.top_k,
            replicates,
            questions: questions.len(),
            cognoscenti_commit: None,
            skip_user_scope: input.skip_user_scope,
            not_run: not_run_by_category(&dropped),
            limits,
            retrieval: None,
            from_replicate_log: indices,
        },
    );
    file.systems.push(system);
    Ok(file)
}

/// The printed lines for the replicates in the log, each checked against its
/// logged passes: same top-k, same pass count and total, and a mean equal to
/// passes / n (which holds only while recall is binary).
fn checked_summaries(
    input: &RebuildInput<'_>,
    finished: &std::collections::BTreeMap<usize, Vec<bool>>,
) -> Result<Vec<SummaryLine>> {
    let mut checked: Vec<SummaryLine> = Vec::new();
    for line in parse_summary_lines(input.summary_text) {
        let Some(passes) = finished.get(&line.replicate) else {
            continue;
        };
        let fully = passes.iter().filter(|p| **p).count();
        if line.top_k != input.top_k
            || line.passed != fully
            || line.questions != passes.len()
            || (line.mean_recall - fully as f64 / passes.len().max(1) as f64).abs() > 0.00006
        {
            bail!(
                "the printed line for replicate {} (recall@{} = {:.4}, {}/{} fully recalled) does not match the log ({fully}/{} at top-k {})",
                line.replicate,
                line.top_k,
                line.mean_recall,
                line.passed,
                line.questions,
                passes.len(),
                input.top_k
            );
        }
        match checked.iter().find(|c| c.replicate == line.replicate) {
            Some(first) if *first != line => bail!(
                "two printed lines for replicate {} disagree",
                line.replicate
            ),
            Some(_) => {}
            None => checked.push(line),
        }
    }
    Ok(checked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::{generate, GeneratorConfig};
    use crate::replicate_log::{LogRecord, RunIdentity};
    use crate::stream::StreamItem;

    fn stream() -> Stream {
        sized_stream(12, 6)
    }

    fn sized_stream(scope: usize, others: usize) -> Stream {
        let config = GeneratorConfig {
            scope_trials: scope,
            supersession_chains: others,
            retrieval_questions: others,
            owner_trials: others,
            individual_items: others,
            agent_items: others,
            paraphrase_pairs: others,
            stable_items: others,
            ..GeneratorConfig::new(1)
        };
        let mut stream = generate(&config);
        // one gold event per question, so a pass is recall 1.0 and nothing else
        for item in &mut stream.items {
            if let StreamItem::Question(q) = item {
                q.gold_event_ids.truncate(1);
            }
        }
        stream
    }

    fn kept(stream: &Stream) -> Stream {
        crate::adapter::drop_user_scope_trials(stream).0
    }

    fn identity(sha: &str) -> RunIdentity {
        RunIdentity {
            system: "achiral".into(),
            stream_sha256: sha.into(),
            top_k: 3,
            seed: 0,
        }
    }

    /// A log of finished replicates, each a pass pattern; replicate `r`
    /// fails question `q` when `(q + r) % 3 == 0`.
    fn log(sha: &str, replicates: &[usize], n: usize) -> String {
        let mut out = String::new();
        for &r in replicates {
            let started = LogRecord::Started {
                run: identity(sha),
                replicate: r,
                key_id: Some(format!("k{r}")),
            };
            let finished = LogRecord::Finished {
                run: identity(sha),
                replicate: r,
                key_id: Some(format!("k{r}")),
                passes: (0..n).map(|q| !(q + r).is_multiple_of(3)).collect(),
            };
            out.push_str(&serde_json::to_string(&started).unwrap());
            out.push('\n');
            out.push_str(&serde_json::to_string(&finished).unwrap());
            out.push('\n');
        }
        out
    }

    fn input<'a>(stream: &'a Stream, log_text: &'a str, summary: &'a str) -> RebuildInput<'a> {
        RebuildInput {
            log_text,
            stream,
            stream_file: "stream.jsonl",
            stream_sha256: "sha",
            skip_user_scope: true,
            top_k: 3,
            seed: 0,
            system: "achiral",
            label: Some("cohort".into()),
            summary_text: summary,
        }
    }

    #[test]
    fn a_rebuilt_cohort_has_per_question_recall_pooled_recall_categories_and_flips() {
        let stream = stream();
        let n = kept(&stream).questions().count();
        let text = log("sha", &[0, 1, 2], n);
        let file = rebuild_cohort(&input(&stream, &text, "")).unwrap();
        assert_eq!(file.label.as_deref(), Some("cohort"));
        assert_eq!(file.run.replicates, 3);
        assert_eq!(file.run.questions, n);
        assert_eq!(file.run.stream_sha256, "sha");
        assert_eq!(file.run.stream_file, "stream.jsonl");
        assert!(file.run.skip_user_scope);
        assert_eq!(file.run.from_replicate_log, vec![0, 1, 2]);
        assert!(!file.run.not_run.is_empty());
        let sys = &file.systems[0];
        assert_eq!(sys.system, "achiral");
        assert_eq!(sys.questions.len(), n);
        // question q passes in the replicates where !(q + r).is_multiple_of(3)
        for (q, row) in sys.questions.iter().enumerate() {
            let passed = (0..3).filter(|r| !(q + r).is_multiple_of(3)).count();
            assert!((row.recall - passed as f64 / 3.0).abs() < 1e-12, "{q}");
            assert_eq!(row.tokens, None);
        }
        // pooled recall = the mean of the three per-replicate recalls
        let per_replicate: Vec<f64> = sys
            .replicates
            .iter()
            .map(|r| r.mean_recall.unwrap())
            .collect();
        let mean = per_replicate.iter().sum::<f64>() / 3.0;
        let recall = sys.recall.as_ref().unwrap();
        assert!((recall.estimate - mean).abs() < 1e-12);
        assert_eq!(recall.n, n);
        let values: Vec<f64> = sys.questions.iter().map(|q| q.recall).collect();
        assert_eq!(recall, &crate::results::Stat::from_values(&values, 0));
        // every question is in exactly one category
        let in_categories: usize = sys.categories.iter().map(|c| c.stat.n).sum();
        assert_eq!(
            in_categories,
            sys.questions
                .iter()
                .filter(|q| q.category.is_some())
                .count()
        );
        // every question fails in some replicate and passes in another
        let flips = sys.flips.as_ref().unwrap();
        assert_eq!(flips.n, n);
        assert_eq!(flips.events, Some(n));
        // what the log cannot give stays absent
        assert!(sys.tokens.is_none());
        assert!(sys.calibration.is_none());
        assert!(sys.leaks.is_empty());
        assert!(sys.tokens_by_cycle.is_empty());
        assert!(file
            .run
            .limits
            .iter()
            .any(|l| l.contains("Rebuilt from a replicate log") && l.contains("0, 1, 2")));
    }

    #[test]
    fn a_question_that_never_flips_is_not_counted_as_flipping() {
        let stream = stream();
        let n = kept(&stream).questions().count();
        // the same replicate twice: nothing differs
        let one = log("sha", &[0], n);
        let two = one.replace("\"replicate\":0", "\"replicate\":1");
        let text = format!("{one}{two}");
        let file = rebuild_cohort(&input(&stream, &text, "")).unwrap();
        assert_eq!(file.systems[0].flips.as_ref().unwrap().events, Some(0));
    }

    #[test]
    fn a_replicate_with_the_wrong_number_of_questions_is_refused() {
        let stream = stream();
        let n = kept(&stream).questions().count();
        let text = log("sha", &[0], n + 1);
        let err = rebuild_cohort(&input(&stream, &text, ""))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(&format!("{}", n + 1)) && err.contains(&format!("{n}")),
            "{err}"
        );
    }

    #[test]
    fn a_log_for_another_stream_is_refused() {
        let stream = stream();
        let n = kept(&stream).questions().count();
        let text = log("other", &[0], n);
        assert!(rebuild_cohort(&input(&stream, &text, "")).is_err());
    }

    #[test]
    fn replicates_must_be_numbered_from_zero_without_gaps() {
        let stream = stream();
        let n = kept(&stream).questions().count();
        for replicates in [vec![1, 2], vec![0, 2]] {
            let text = log("sha", &replicates, n);
            let err = rebuild_cohort(&input(&stream, &text, ""))
                .unwrap_err()
                .to_string();
            assert!(err.contains("replicate"), "{err}");
        }
        assert!(
            rebuild_cohort(&input(&stream, "", "")).is_err(),
            "an empty log has nothing"
        );
    }

    #[test]
    fn a_question_with_more_than_one_gold_event_is_refused_because_pass_is_not_recall() {
        let mut stream = stream();
        let mut changed = None;
        for item in &mut stream.items {
            if let StreamItem::Question(q) = item {
                if q.id.starts_with("supersede") || q.id.starts_with("retrieval") {
                    q.gold_event_ids = vec!["a".into(), "b".into()];
                    changed = Some(q.id.clone());
                    break;
                }
            }
        }
        let id = changed.expect("a kept question to edit");
        let n = kept(&stream).questions().count();
        let text = log("sha", &[0], n);
        let err = rebuild_cohort(&input(&stream, &text, ""))
            .unwrap_err()
            .to_string();
        assert!(err.contains(&id) && err.contains("gold"), "{err}");
    }

    #[test]
    fn printed_summary_lines_are_parsed() {
        let text = "noise\n[replicate 1] mean recall@3 = 0.5570; questions fully recalled: 303/544; trials with a leak: 0/141\n  questions that flipped\n[replicate 4] mean recall@3 = 0.5607; questions fully recalled: 305/544; trials with a leak: 2/141\n";
        let lines = parse_summary_lines(text);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].replicate, 1);
        assert_eq!(lines[0].top_k, 3);
        assert!((lines[0].mean_recall - 0.5570).abs() < 1e-12);
        assert_eq!((lines[0].passed, lines[0].questions), (303, 544));
        assert_eq!((lines[0].leaked, lines[0].leak_trials), (0, 141));
        assert_eq!(lines[1].replicate, 4);
        assert_eq!(lines[1].leaked, 2);
    }

    fn line(r: usize, n: usize, passed: usize, leaked: usize) -> String {
        format!(
            "[replicate {r}] mean recall@3 = {:.4}; questions fully recalled: {passed}/{n}; trials with a leak: {leaked}/141\n",
            passed as f64 / n as f64
        )
    }

    #[test]
    fn summary_lines_that_agree_with_the_log_fill_in_the_leak_counts() {
        let stream = stream();
        let n = kept(&stream).questions().count();
        let text = log("sha", &[0, 1], n);
        let passed = |r: usize| (0..n).filter(|q| !(q + r).is_multiple_of(3)).count();
        let summary = format!("{}{}", line(0, n, passed(0), 0), line(1, n, passed(1), 2));
        let file = rebuild_cohort(&input(&stream, &text, &summary)).unwrap();
        let rows = &file.systems[0].replicates;
        assert_eq!(
            (rows[0].leaked_trials, rows[0].leak_trials),
            (Some(0), Some(141))
        );
        assert_eq!(
            (rows[1].leaked_trials, rows[1].leak_trials),
            (Some(2), Some(141))
        );
        // without lines the counts are absent, not zero
        let bare = rebuild_cohort(&input(&stream, &text, "")).unwrap();
        assert_eq!(bare.systems[0].replicates[0].leaked_trials, None);
    }

    #[test]
    fn a_summary_line_that_disagrees_with_the_log_stops_the_rebuild() {
        let stream = stream();
        let n = kept(&stream).questions().count();
        let text = log("sha", &[0], n);
        let passed = (0..n).filter(|q| q % 3 != 0).count();
        // wrong pass count
        let err = rebuild_cohort(&input(&stream, &text, &line(0, n, passed + 1, 0)))
            .unwrap_err()
            .to_string();
        assert!(err.contains("replicate 0"), "{err}");
        // the mean matches but the printed count does not
        let count_only = format!(
            "[replicate 0] mean recall@3 = {:.4}; questions fully recalled: {}/{n}; trials with a leak: 0/141\n",
            passed as f64 / n as f64,
            passed + 1
        );
        assert!(rebuild_cohort(&input(&stream, &text, &count_only)).is_err());
        let total_only = format!(
            "[replicate 0] mean recall@3 = {:.4}; questions fully recalled: {passed}/{}; trials with a leak: 0/141\n",
            passed as f64 / n as f64,
            n + 1
        );
        assert!(rebuild_cohort(&input(&stream, &text, &total_only)).is_err());
        // mean that is not passes / n, so recall is not binary
        let bad_mean = format!(
            "[replicate 0] mean recall@3 = 0.9000; questions fully recalled: {passed}/{n}; trials with a leak: 0/141\n"
        );
        assert!(rebuild_cohort(&input(&stream, &text, &bad_mean)).is_err());
        // another top-k
        let other_k = line(0, n, passed, 0).replace("recall@3", "recall@5");
        assert!(rebuild_cohort(&input(&stream, &text, &other_k)).is_err());
        // two lines for one replicate that disagree
        let twice = format!("{}{}", line(0, n, passed, 0), line(0, n, passed, 1));
        assert!(rebuild_cohort(&input(&stream, &text, &twice)).is_err());
    }

    #[test]
    fn the_seed_given_is_the_seed_the_intervals_use() {
        let stream = sized_stream(30, 15);
        let n = kept(&stream).questions().count();
        let text = log("sha", &[0, 1, 2, 3], n);
        // recall is discrete, so two neighbouring seeds can give one interval;
        // several seeds make a wrong one show
        for seed in 7..12 {
            let mut with_seed = input(&stream, &text, "");
            with_seed.seed = seed;
            // the log names seed 0, so identity must carry this seed too
            let text_s = text.replace("\"seed\":0", &format!("\"seed\":{seed}"));
            with_seed.log_text = &text_s;
            let file = rebuild_cohort(&with_seed).unwrap();
            let sys = &file.systems[0];
            let values: Vec<f64> = sys.questions.iter().map(|q| q.recall).collect();
            assert!(sys.recall.as_ref().unwrap().ci.is_some());
            assert_eq!(sys.recall, Some(Stat::from_values(&values, seed)), "{seed}");
            let flipped: Vec<bool> = (0..n)
                .map(|q| {
                    let outcomes: Vec<bool> = (0..4).map(|r| !(q + r).is_multiple_of(3)).collect();
                    outcomes.iter().any(|o| *o != outcomes[0])
                })
                .collect();
            assert_eq!(sys.flips, Some(Stat::from_events(&flipped, seed)), "{seed}");
        }
    }
}

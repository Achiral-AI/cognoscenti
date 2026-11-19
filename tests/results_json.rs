//! `--results-json` end to end: a replay writes a results file, `report`
//! turns it into a page, and the numbers in the page are the replay's.

use clap::Parser;
use cognoscenti::adapter::run_baseline_replay;
use cognoscenti::generator::{generate, to_jsonl, GeneratorConfig};
use cognoscenti::report::{run_report, ReportArgs};
use cognoscenti::results::RunFile;
use cognoscenti::runner::BenchmarkConfig;

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cogn-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn stream_file(dir: &std::path::Path) -> std::path::PathBuf {
    let config = GeneratorConfig {
        scope_trials: 20,
        supersession_chains: 12,
        retrieval_questions: 12,
        owner_trials: 12,
        individual_items: 12,
        agent_items: 12,
        paraphrase_pairs: 12,
        stable_items: 12,
        ..GeneratorConfig::new(1)
    };
    let path = dir.join("stream.jsonl");
    std::fs::write(&path, to_jsonl(&generate(&config), &config)).unwrap();
    path
}

fn replay(stream: &std::path::Path, out: &std::path::Path, extra: &[&str]) {
    let mut args = vec![
        "cognoscenti",
        "--replay-stream",
        stream.to_str().unwrap(),
        "--results-json",
        out.to_str().unwrap(),
        "--skip-user-scope",
    ];
    args.extend_from_slice(extra);
    run_baseline_replay(&BenchmarkConfig::parse_from(args)).unwrap();
}

#[test]
fn a_replay_writes_a_results_file_a_report_can_read() {
    let dir = tmp("replay");
    let stream = stream_file(&dir);
    let out = dir.join("baselines.json");
    replay(
        &stream,
        &out,
        &[
            "--systems",
            "bm25,verbatim",
            "--replicates",
            "2",
            "--label",
            "baselines",
        ],
    );
    let file = RunFile::load(&out).unwrap();
    assert_eq!(file.label.as_deref(), Some("baselines"));
    assert_eq!(file.run.stream_file, "stream.jsonl");
    assert_eq!(file.run.stream_sha256.len(), 64);
    assert!(
        !file.run.stream_file.contains('/'),
        "a file name, never a path"
    );
    assert_eq!(file.run.replicates, 2);
    assert!(file.run.skip_user_scope);
    let names: Vec<&str> = file.systems.iter().map(|s| s.system.as_str()).collect();
    assert_eq!(names, vec!["bm25", "verbatim"]);
    let bm25 = &file.systems[0];
    assert_eq!(bm25.recall.as_ref().unwrap().n, file.run.questions);
    assert_eq!(bm25.questions.len(), file.run.questions);
    assert!(!bm25.categories.is_empty());
    assert_eq!(bm25.replicates.len(), 2);
    assert_eq!(
        bm25.flips.as_ref().unwrap().events,
        Some(0),
        "a baseline never flips"
    );
    assert_eq!(bm25.flips.as_ref().unwrap().n, file.run.questions);
    assert_eq!(bm25.calibration.as_ref().unwrap().basis, "derived");

    let index = run_report(&ReportArgs::parse_from([
        "report",
        "--results",
        out.to_str().unwrap(),
    ]))
    .unwrap();
    let html = std::fs::read_to_string(index).unwrap();
    assert!(html.contains("bm25 · baselines"));
    assert!(html.contains(&file.run.stream_sha256));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_cycles_run_records_the_token_checkpoints() {
    let dir = tmp("cycles");
    let stream = stream_file(&dir);
    let out = dir.join("cycles.json");
    replay(
        &stream,
        &out,
        &["--systems", "bm25", "--cycles", "10", "--label", "cycles"],
    );
    let file = RunFile::load(&out).unwrap();
    let cycles: Vec<usize> = file.systems[0]
        .tokens_by_cycle
        .iter()
        .map(|p| p.cycle)
        .collect();
    assert_eq!(cycles, vec![10]);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn no_results_file_is_written_unless_asked() {
    let dir = tmp("none");
    let stream = stream_file(&dir);
    run_baseline_replay(&BenchmarkConfig::parse_from([
        "cognoscenti",
        "--replay-stream",
        stream.to_str().unwrap(),
        "--systems",
        "bm25",
    ]))
    .unwrap();
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_resumed_replicate_keeps_pass_counts_only_and_a_live_one_keeps_its_mean() {
    let dir = tmp("resume");
    let stream = stream_file(&dir);
    let log = dir.join("log.jsonl");
    let first = dir.join("first.json");
    let args = [
        "--systems",
        "bm25",
        "--replicates",
        "2",
        "--replicate-log",
        log.to_str().unwrap(),
    ];
    replay(&stream, &first, &args);
    // keep only replicate 0's start and finish, as after a crash in replicate 1
    let kept: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .take(2)
        .map(String::from)
        .collect();
    std::fs::write(&log, kept.join("\n") + "\n").unwrap();
    let second = dir.join("second.json");
    let mut resumed = args.to_vec();
    resumed.push("--resume");
    replay(&stream, &second, &resumed);
    let rows = &RunFile::load(&second).unwrap().systems[0].replicates;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].mean_recall, None);
    assert!(rows[0].fully_recalled > 0);
    assert!(rows[1].mean_recall.is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_cohort_rebuilt_from_the_replicate_log_matches_the_live_numbers() {
    use cognoscenti::rebuild::{run_rebuild, RebuildArgs};
    let dir = tmp("rebuild");
    // one gold event per question, as in the registered dataset
    let config = GeneratorConfig {
        scope_trials: 20,
        supersession_chains: 12,
        retrieval_questions: 12,
        owner_trials: 12,
        individual_items: 12,
        agent_items: 12,
        paraphrase_pairs: 12,
        stable_items: 12,
        ..GeneratorConfig::new(1)
    };
    let mut generated = generate(&config);
    for item in &mut generated.items {
        if let cognoscenti::stream::StreamItem::Question(q) = item {
            q.gold_event_ids.truncate(1);
        }
    }
    let stream = dir.join("stream.jsonl");
    std::fs::write(&stream, to_jsonl(&generated, &config)).unwrap();
    let log = dir.join("log.jsonl");
    let live = dir.join("live.json");
    replay(
        &stream,
        &live,
        &[
            "--systems",
            "bm25",
            "--replicates",
            "3",
            "--replicate-log",
            log.to_str().unwrap(),
        ],
    );
    // the log names the system "bm25"; a rebuild must be told the same
    let rebuilt = dir.join("rebuilt.json");
    let args = RebuildArgs::parse_from([
        "rebuild-results",
        "--replicate-log",
        log.to_str().unwrap(),
        "--stream",
        stream.to_str().unwrap(),
        "--skip-user-scope",
        "--system",
        "bm25",
        "--label",
        "rebuilt",
        "--out",
        rebuilt.to_str().unwrap(),
    ]);
    run_rebuild(&args).unwrap();
    let live = RunFile::load(&live).unwrap();
    let rebuilt = RunFile::load(&rebuilt).unwrap();
    assert_eq!(live.systems[0].recall, rebuilt.systems[0].recall);
    assert_eq!(live.systems[0].categories, rebuilt.systems[0].categories);
    assert_eq!(live.systems[0].flips, rebuilt.systems[0].flips);
    assert_eq!(live.run.not_run, rebuilt.run.not_run);
    assert_eq!(live.run.questions, rebuilt.run.questions);
    assert_eq!(rebuilt.run.from_replicate_log, vec![0, 1, 2]);
    std::fs::remove_dir_all(&dir).unwrap();
}

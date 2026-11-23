//! Plan 40 Track C: a replay of a team stream scores it with the team
//! metrics and keeps them in the results file, so the first Achiral team run
//! reports supersession, provenance and lineage beside recall.

use clap::Parser;
use cognoscenti::adapter::run_baseline_replay;
use cognoscenti::report::{run_report, ReportArgs};
use cognoscenti::results::RunFile;
use cognoscenti::runner::BenchmarkConfig;
use cognoscenti::team::{generate_team, items_to_jsonl, to_jsonl, TeamConfig};

fn tmp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cogn-team-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn replay(stream: &std::path::Path, out: &std::path::Path) -> RunFile {
    run_baseline_replay(&BenchmarkConfig::parse_from([
        "cognoscenti",
        "--replay-stream",
        stream.to_str().unwrap(),
        "--results-json",
        out.to_str().unwrap(),
        "--systems",
        "bm25",
    ]))
    .unwrap();
    RunFile::load(out).unwrap()
}

#[test]
fn a_team_stream_with_its_items_file_is_scored_with_the_team_metrics() {
    let dir = tmp("scored");
    let config = TeamConfig::first_batch(0);
    let batch = generate_team(&config);
    let stream = dir.join("stream.jsonl");
    std::fs::write(&stream, to_jsonl(&batch, &config)).unwrap();
    std::fs::write(dir.join("items.jsonl"), items_to_jsonl(&batch.items)).unwrap();

    let file = replay(&stream, &dir.join("out.json"));

    let team = &file.systems[0].team;
    let names: Vec<&str> = team.iter().map(|t| t.name.as_str()).collect();
    for expected in [
        "supersession correctness",
        "control: older fact stands",
        "control: point in time",
        "provenance accuracy",
        "lineage edge recall",
        "lineage chain complete",
        "lineage decoy hit",
        "leaver recall",
        "leaver leak",
    ] {
        assert!(names.contains(&expected), "{expected} missing in {names:?}");
    }
    let sup = team
        .iter()
        .find(|t| t.name == "supersession correctness")
        .unwrap();
    assert!(sup.stat.n > 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_stream_without_an_items_file_gets_no_team_metrics() {
    let dir = tmp("noitems");
    let config = TeamConfig::first_batch(0);
    let batch = generate_team(&config);
    let stream = dir.join("stream.jsonl");
    std::fs::write(&stream, to_jsonl(&batch, &config)).unwrap();

    let file = replay(&stream, &dir.join("out.json"));

    assert!(file.systems[0].team.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_report_lists_the_team_metrics_only_when_a_run_has_them() {
    let dir = tmp("report");
    let config = TeamConfig::first_batch(0);
    let batch = generate_team(&config);
    let stream = dir.join("stream.jsonl");
    std::fs::write(&stream, to_jsonl(&batch, &config)).unwrap();
    std::fs::write(dir.join("items.jsonl"), items_to_jsonl(&batch.items)).unwrap();
    let out = dir.join("out.json");
    replay(&stream, &out);
    let index = run_report(&ReportArgs::parse_from([
        "report",
        "--results",
        out.to_str().unwrap(),
    ]))
    .unwrap();
    let html = std::fs::read_to_string(index).unwrap();
    assert!(html.contains("Team memory"));
    assert!(html.contains("supersession correctness"));

    std::fs::remove_file(dir.join("items.jsonl")).unwrap();
    let bare = dir.join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::copy(&stream, bare.join("stream.jsonl")).unwrap();
    replay(&bare.join("stream.jsonl"), &bare.join("out.json"));
    let index = run_report(&ReportArgs::parse_from([
        "report",
        "--results",
        bare.join("out.json").to_str().unwrap(),
    ]))
    .unwrap();
    assert!(!std::fs::read_to_string(index)
        .unwrap()
        .contains("Team memory"));
    std::fs::remove_dir_all(&dir).unwrap();
}

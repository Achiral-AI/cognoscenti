//! A per-replicate log for long `--replicates` runs against a live system.
//! Each replicate appends one record when it starts and one when it finishes,
//! so a crash loses at most the replicate in flight, and `--resume` can skip
//! the finished ones.
//!
//! Resume is only sound at replicate granularity with a fresh org for the
//! unfinished replicate: the org the crash interrupted already holds part of
//! the stream and the recall history of the questions asked so far, so
//! replaying into it would not be an independent run. The log therefore
//! records a short hash of each replicate's API key, and a resume refuses to
//! run a replicate whose key was already used.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::Path;

/// What a log belongs to. A log from another stream, seed, `--top-k` or
/// system must never feed a resume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunIdentity {
    pub system: String,
    pub stream_sha256: String,
    pub top_k: usize,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LogRecord {
    Started {
        run: RunIdentity,
        replicate: usize,
        key_id: Option<String>,
    },
    Finished {
        run: RunIdentity,
        replicate: usize,
        key_id: Option<String>,
        /// Per question, in stream order: did recall@k reach 1.0.
        passes: Vec<bool>,
    },
}

/// A short, non-reversible label for an API key: enough to tell two orgs
/// apart in the log without writing a credential to disk.
pub fn key_id(api_key: &str) -> String {
    let digest = Sha256::digest(api_key.as_bytes());
    digest[..6].iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Default, PartialEq)]
pub struct LogState {
    pub finished: BTreeMap<usize, Vec<bool>>,
    pub used_keys: HashSet<String>,
}

/// Reads a log's text. A last line that does not parse is a write the crash
/// cut short and is ignored; a bad line anywhere else, or a record from
/// another run, is an error.
pub fn parse_log(text: &str, run: &RunIdentity) -> Result<LogState> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut state = LogState::default();
    for (i, line) in lines.iter().enumerate() {
        let record: LogRecord = match serde_json::from_str(line) {
            Ok(record) => record,
            Err(_) if i + 1 == lines.len() => continue,
            Err(err) => bail!("replicate log line {} is not a record: {err}", i + 1),
        };
        let (record_run, replicate, key, passes) = match record {
            LogRecord::Started {
                run,
                replicate,
                key_id,
            } => (run, replicate, key_id, None),
            LogRecord::Finished {
                run,
                replicate,
                key_id,
                passes,
            } => (run, replicate, key_id, Some(passes)),
        };
        if &record_run != run {
            bail!(
                "replicate log line {} belongs to another run ({} on stream {}, top-k {}, seed {})",
                i + 1,
                record_run.system,
                record_run.stream_sha256,
                record_run.top_k,
                record_run.seed
            );
        }
        if let Some(key) = key {
            state.used_keys.insert(key);
        }
        if let Some(passes) = passes {
            state.finished.insert(replicate, passes);
        }
    }
    Ok(state)
}

/// Refuses to run `replicate` into an org an earlier start already used.
pub fn check_fresh_org(state: &LogState, replicate: usize, key: Option<&str>) -> Result<()> {
    match key {
        Some(key) if state.used_keys.contains(key) => bail!(
            "replicate {replicate} would run in an org the log shows was already started \
             (key id {key}); provision a fresh org and put its key at index {replicate} of \
             COGNOSCENTI_ACHIRAL_API_KEYS"
        ),
        _ => Ok(()),
    }
}

/// Finished replicates must have scored the same number of questions as this
/// stream has, or the log is for a different question set.
pub fn check_finished_shape(state: &LogState, num_questions: usize) -> Result<()> {
    for (replicate, passes) in &state.finished {
        if passes.len() != num_questions {
            bail!(
                "replicate {replicate} in the log scored {} question(s), this stream has {num_questions}",
                passes.len()
            );
        }
    }
    Ok(())
}

pub fn append(path: &Path, record: &LogRecord) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening replicate log {}", path.display()))?;
    let mut line = serde_json::to_string(record)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    file.sync_all()
        .with_context(|| format!("flushing replicate log {}", path.display()))
}

/// The line printed as each replicate ends, so an output file holds every
/// finished replicate even if the run dies later.
pub fn summary_line(
    replicate: usize,
    top_k: usize,
    mean_recall: f64,
    passes: &[bool],
    leaked_trials: usize,
    leak_trials: usize,
) -> String {
    let passed = passes.iter().filter(|p| **p).count();
    format!(
        "[replicate {replicate}] mean recall@{top_k} = {mean_recall:.4}; questions fully recalled: {passed}/{}; trials with a leak: {leaked_trials}/{leak_trials}",
        passes.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> RunIdentity {
        RunIdentity {
            system: "achiral".into(),
            stream_sha256: "abc".into(),
            top_k: 3,
            seed: 0,
        }
    }

    fn started(replicate: usize, key: &str) -> LogRecord {
        LogRecord::Started {
            run: run(),
            replicate,
            key_id: Some(key.into()),
        }
    }

    fn finished(replicate: usize, key: &str, passes: &[bool]) -> LogRecord {
        LogRecord::Finished {
            run: run(),
            replicate,
            key_id: Some(key.into()),
            passes: passes.to_vec(),
        }
    }

    fn text(records: &[LogRecord]) -> String {
        records
            .iter()
            .map(|r| serde_json::to_string(r).unwrap() + "\n")
            .collect()
    }

    #[test]
    fn a_key_id_is_short_stable_and_not_the_key() {
        let id = key_id("acm_secret_key");
        assert_eq!(id.len(), 12);
        assert_eq!(id, key_id("acm_secret_key"));
        assert_ne!(id, key_id("acm_other_key"));
        assert!(!id.contains("secret"));
    }

    #[test]
    fn finished_replicates_and_every_started_key_are_read_back() {
        let log = text(&[
            started(0, "k0"),
            finished(0, "k0", &[true, false]),
            started(1, "k1"),
        ]);
        let state = parse_log(&log, &run()).unwrap();
        assert_eq!(state.finished.len(), 1);
        assert_eq!(state.finished[&0], vec![true, false]);
        assert!(state.used_keys.contains("k0") && state.used_keys.contains("k1"));
    }

    #[test]
    fn a_torn_last_line_is_ignored_but_a_bad_middle_line_is_an_error() {
        let good = text(&[started(0, "k0")]);
        let torn = format!("{good}{{\"event\":\"fini");
        assert!(parse_log(&torn, &run()).unwrap().finished.is_empty());
        let bad_middle = format!("not json\n{good}");
        assert!(parse_log(&bad_middle, &run()).is_err());
    }

    #[test]
    fn a_log_from_another_run_is_refused() {
        let mut other = run();
        other.stream_sha256 = "different".into();
        let log = text(&[LogRecord::Started {
            run: other,
            replicate: 0,
            key_id: None,
        }]);
        let err = parse_log(&log, &run()).unwrap_err().to_string();
        assert!(err.contains("another run"), "{err}");
        for change in [
            |r: &mut RunIdentity| r.top_k = 5,
            |r: &mut RunIdentity| r.seed = 9,
            |r: &mut RunIdentity| r.system = "bm25".into(),
        ] {
            let mut other = run();
            change(&mut other);
            let log = text(&[LogRecord::Started {
                run: other,
                replicate: 0,
                key_id: None,
            }]);
            assert!(parse_log(&log, &run()).is_err());
        }
    }

    #[test]
    fn a_replicate_may_not_rerun_in_an_org_the_log_already_started() {
        let state = parse_log(&text(&[started(1, "k1")]), &run()).unwrap();
        let err = check_fresh_org(&state, 1, Some("k1"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("fresh org"), "{err}");
        assert!(check_fresh_org(&state, 1, Some("k9")).is_ok());
        assert!(check_fresh_org(&state, 1, None).is_ok());
    }

    #[test]
    fn finished_replicates_must_match_the_streams_question_count() {
        let state = parse_log(&text(&[finished(0, "k0", &[true, true])]), &run()).unwrap();
        assert!(check_finished_shape(&state, 2).is_ok());
        assert!(check_finished_shape(&state, 3).is_err());
    }

    #[test]
    fn appended_records_read_back_in_order() {
        let dir = std::env::temp_dir().join(format!("cogn-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.jsonl");
        let _ = std::fs::remove_file(&path);
        append(&path, &started(0, "k0")).unwrap();
        append(&path, &finished(0, "k0", &[true])).unwrap();
        let state = parse_log(&std::fs::read_to_string(&path).unwrap(), &run()).unwrap();
        assert_eq!(state.finished[&0], vec![true]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_summary_line_names_the_replicate_recall_and_leaks() {
        let line = summary_line(2, 3, 0.5331, &[true, false, true], 1, 71);
        assert!(line.starts_with("[replicate 2]"), "{line}");
        assert!(line.contains("recall@3 = 0.5331"), "{line}");
        assert!(line.contains("2/3"), "{line}");
        assert!(line.contains("leak: 1/71"), "{line}");
    }
}

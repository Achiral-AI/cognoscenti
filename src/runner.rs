use crate::eval::{self, MemoryEvalReport};
use anyhow::Result;
use clap::Parser;

/// Configuration for running benchmarks
#[derive(Debug, Clone, Parser)]
pub struct BenchmarkConfig {
    /// Output directory for results
    #[clap(long, default_value = "./results")]
    pub output_dir: String,

    /// Comma-separated memory systems for JSONL item evals: local, mem0, supermemory, zep, letta
    #[clap(long, alias = "system", default_value = "local")]
    pub systems: String,

    /// JSONL benchmark item file for external memory-system eval mode
    #[clap(long)]
    pub items_file: Option<String>,

    /// Lint a stream dataset file and exit, instead of running an eval.
    /// Fails (nonzero exit) if any issue is found, for use as a CI gate.
    #[clap(long)]
    pub lint_stream: Option<String>,

    /// Replay a stream dataset file through a baseline system and report
    /// recall@k per question. Reuses --systems (one of: verbatim, bm25,
    /// recency) and --top-k.
    #[clap(long)]
    pub replay_stream: Option<String>,

    /// Generate a synthetic stream (seeded by `--seed`, no model in the
    /// loop), write it to this path, lint it and print its SHA-256, then
    /// exit. Sized by `--scope-trials` and `--items-per-category`.
    #[clap(long)]
    pub generate_stream: Option<String>,

    /// Scope/leak trials in a generated stream. ~300 are needed before a
    /// "<1%" leak claim (3/n bound).
    #[clap(long, default_value = "300")]
    pub scope_trials: usize,

    /// Items in each of the other generated categories (supersession,
    /// retrieval, owner, individual, agent). ~70 gives a 95% CI of about +/-11 points.
    #[clap(long, default_value = "70")]
    pub items_per_category: usize,

    /// Half-life, in days, for the recency baseline's exponential decay.
    #[clap(long, default_value = "30")]
    pub recency_half_life_days: f64,

    /// Window size, in events, for the bounded-window baseline.
    #[clap(long, default_value = "20")]
    pub bounded_window_size: usize,

    /// Repeat the replayed stream this many times, reporting mean tokens
    /// injected per question at geometric checkpoints (10, 100, 1000, ...)
    /// up to this count - the "durable horizon" token-savings signal
    /// (Plan 25): a shape that holds as cycles grow, not a one-run number.
    /// Only used with `--replay-stream`.
    #[clap(long, default_value = "1")]
    pub cycles: usize,

    /// Number of retrieved memories to evaluate per query
    #[clap(long, default_value = "3")]
    pub top_k: usize,

    /// Seed for every source of randomness Cognoscenti itself introduces
    /// (stream generation, replicate ordering, ACT-R retrieval noise).
    /// Recorded in the run manifest regardless of whether the current run
    /// uses any randomness.
    #[clap(long, default_value = "0")]
    pub seed: u64,

    /// Replay the stream through a freshly built system this many times,
    /// reporting flip rate (Plan 25's run-to-run stability test) across
    /// those replicates. A deterministic baseline always reports 0; this
    /// is the control a noisy system is compared against. Only used with
    /// `--replay-stream`.
    #[clap(long, default_value = "1")]
    pub replicates: usize,

    /// Free-text label printed at the top of a replay, so output records
    /// what a run was configured with outside this harness (for example
    /// `activationNoiseScale=0.2`). Cognoscenti does not interpret it.
    #[clap(long)]
    pub label: Option<String>,

    /// Drop every generated trial that holds a user-scope event, and say how
    /// many were dropped. For a deployment whose per-member routes are
    /// disabled: a refused write must not be scored as "nothing leaked".
    /// Hand-made datasets are never touched. Only used with `--replay-stream`.
    #[clap(long)]
    pub skip_user_scope: bool,

    /// Number of equal-width bins for calibration (reliability curve,
    /// expected calibration error). Only used with `--replay-stream`.
    #[clap(long, default_value = "10")]
    pub calibration_bins: usize,
}

/// Runs a memory-system evaluation against a JSONL benchmark item file.
///
/// There is no other mode: Cognoscenti no longer simulates a workload or
/// invents metrics for a system it never called.
pub struct BenchmarkRunner {
    config: BenchmarkConfig,
}

impl BenchmarkRunner {
    pub fn new(config: BenchmarkConfig) -> Result<Self> {
        Ok(Self { config })
    }

    pub fn run(&mut self) -> Result<MemoryEvalReport> {
        eval::run_memory_eval(&self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn runner_construction_never_simulates_anything() {
        // No workload, no canned content: constructing a runner is just
        // holding the config until `run()` is asked to do real work.
        let config = BenchmarkConfig::parse_from(["cognoscenti"]);
        assert!(BenchmarkRunner::new(config).is_ok());
    }

    #[test]
    fn generator_flags_default_to_the_sizes_plan_40_calls_for() {
        let config = BenchmarkConfig::parse_from(["cognoscenti", "--generate-stream", "out.jsonl"]);
        assert_eq!(config.generate_stream.as_deref(), Some("out.jsonl"));
        assert_eq!((config.scope_trials, config.items_per_category), (300, 70));
    }

    #[test]
    fn skip_user_scope_is_off_by_default() {
        assert!(!BenchmarkConfig::parse_from(["cognoscenti"]).skip_user_scope);
        let on = BenchmarkConfig::parse_from(["cognoscenti", "--skip-user-scope"]);
        assert!(on.skip_user_scope);
    }

    #[test]
    fn label_is_optional_and_taken_verbatim() {
        let none = BenchmarkConfig::parse_from(["cognoscenti"]);
        assert_eq!(none.label, None);
        let labelled =
            BenchmarkConfig::parse_from(["cognoscenti", "--label", "activationNoiseScale=0.2"]);
        assert_eq!(labelled.label.as_deref(), Some("activationNoiseScale=0.2"));
    }

    #[test]
    fn run_without_items_file_is_an_error_not_a_fake_result() {
        // There is no simulated-workload mode any more: with no system under
        // test, `run()` must fail loudly instead of returning invented metrics.
        let config = BenchmarkConfig::parse_from(["cognoscenti"]);
        let mut runner = BenchmarkRunner::new(config).unwrap();

        let error = runner.run().expect_err("run() should require --items-file");
        assert!(error.to_string().contains("--items-file"));
    }
}

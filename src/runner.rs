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

    /// Number of retrieved memories to evaluate per query
    #[clap(long, default_value = "3")]
    pub top_k: usize,
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
        // No workload, no RNG, no canned content: constructing a runner is just
        // holding the config until `run()` is asked to do real work.
        let config = BenchmarkConfig::parse_from(["cognoscenti"]);
        assert!(BenchmarkRunner::new(config).is_ok());
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

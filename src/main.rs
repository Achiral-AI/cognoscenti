use anyhow::Result;
use clap::Parser;
use cognoscenti::adapter::run_baseline_replay;
use cognoscenti::lint::lint_stream_file;
use cognoscenti::runner::{BenchmarkConfig, BenchmarkRunner};

fn main() -> Result<()> {
    let config = BenchmarkConfig::parse();

    if let Some(path) = &config.lint_stream {
        let issues = lint_stream_file(path)?;
        anyhow::ensure!(
            issues.is_empty(),
            "dataset lint failed: {} issue(s)",
            issues.len()
        );
        return Ok(());
    }

    if config.replay_stream.is_some() {
        return run_baseline_replay(&config);
    }

    let mut runner = BenchmarkRunner::new(config)?;
    runner.run()?;

    Ok(())
}

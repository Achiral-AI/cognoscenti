use anyhow::Result;
use clap::Parser;
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

    let mut runner = BenchmarkRunner::new(config)?;
    runner.run()?;

    Ok(())
}

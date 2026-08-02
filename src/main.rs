use anyhow::Result;
use clap::Parser;
use cognoscenti::runner::{BenchmarkConfig, BenchmarkRunner};

fn main() -> Result<()> {
    let config = BenchmarkConfig::parse();

    let mut runner = BenchmarkRunner::new(config)?;
    runner.run()?;

    Ok(())
}

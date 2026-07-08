use cognoscenti::runner::{BenchmarkConfig, BenchmarkRunner};
use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let config = BenchmarkConfig::parse();
    
    let mut runner = BenchmarkRunner::new(config)?;
    runner.run()?;
    
    Ok(())
}

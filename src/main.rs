use anyhow::Result;
use clap::Parser;
use cognoscenti::adapter::run_baseline_replay;
use cognoscenti::generator::{generate, to_jsonl, GeneratorConfig};
use cognoscenti::lint::lint_stream_file;
use cognoscenti::manifest::sha256_file;
use cognoscenti::runner::{BenchmarkConfig, BenchmarkRunner};

fn main() -> Result<()> {
    let config = BenchmarkConfig::parse();

    if let Some(path) = &config.generate_stream {
        return generate_stream_file(&config, path);
    }

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

fn generate_stream_file(config: &BenchmarkConfig, path: &str) -> Result<()> {
    let generator = GeneratorConfig {
        seed: config.seed,
        scope_trials: config.scope_trials,
        supersession_chains: config.items_per_category,
        retrieval_questions: config.items_per_category,
        owner_trials: config.items_per_category,
        individual_items: config.items_per_category,
        agent_items: config.items_per_category,
        paraphrase_pairs: config.items_per_category,
        stable_items: config.items_per_category,
    };
    let stream = generate(&generator);
    std::fs::write(path, to_jsonl(&stream, &generator))?;
    println!(
        "Generated {path}: {} event(s), {} question(s), seed {}",
        stream.events().count(),
        stream.questions().count(),
        config.seed
    );
    println!("sha256 {}", sha256_file(path)?);
    let issues = lint_stream_file(path)?;
    anyhow::ensure!(
        issues.is_empty(),
        "generated stream failed lint: {} issue(s)",
        issues.len()
    );
    Ok(())
}

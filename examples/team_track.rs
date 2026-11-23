//! Track C tooling: generate a team-memory batch, and fold reviewer decisions
//! back in. Offline only; it never contacts a deployment.
//!
//!   cargo run --example team_track -- generate --seed 0 --size first --out-dir datasets/generated/team-seed-0
//!   cargo run --example team_track -- import-review --dir datasets/generated/team-seed-0 --review reviewed.csv --out-dir out

use anyhow::{bail, ensure, Result};
use clap::{Parser, Subcommand};
use cognoscenti::lint::lint_stream_file;
use cognoscenti::manifest::sha256_file;
use cognoscenti::stream::Stream;
use cognoscenti::team::generate::{items_from_jsonl, items_to_jsonl};
use cognoscenti::team::{
    apply_review, export_review_csv, generate_team, import_review, to_jsonl, TeamBatch, TeamConfig,
};
use std::fs;
use std::path::Path;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Generate {
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// `first` (64 items) or `full` (500 items).
        #[arg(long, default_value = "first")]
        size: String,
        #[arg(long)]
        out_dir: String,
    },
    ImportReview {
        /// A directory written by `generate`.
        #[arg(long)]
        dir: String,
        #[arg(long)]
        review: String,
        #[arg(long)]
        out_dir: String,
        /// Write the reviewed batch even if some rows have no decision.
        #[arg(long)]
        allow_unreviewed: bool,
    },
}

fn write_checksums(dir: &Path, files: &[&str]) -> Result<()> {
    let mut out = String::new();
    for f in files {
        out.push_str(&format!(
            "{}  {f}\n",
            sha256_file(dir.join(f).to_str().unwrap())?
        ));
    }
    fs::write(dir.join("CHECKSUMS.txt"), out)?;
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Generate {
            seed,
            size,
            out_dir,
        } => {
            let config = match size.as_str() {
                "first" => TeamConfig::first_batch(seed),
                "full" => TeamConfig::full(seed),
                other => bail!("--size must be first or full, got {other}"),
            };
            let batch = generate_team(&config);
            let dir = Path::new(&out_dir);
            fs::create_dir_all(dir)?;
            fs::write(dir.join("stream.jsonl"), to_jsonl(&batch, &config))?;
            fs::write(dir.join("items.jsonl"), items_to_jsonl(&batch.items))?;
            fs::write(dir.join("review.csv"), export_review_csv(&batch))?;
            let issues = lint_stream_file(dir.join("stream.jsonl").to_str().unwrap())?;
            ensure!(issues.is_empty(), "generated stream failed lint");
            write_checksums(dir, &["stream.jsonl", "items.jsonl", "review.csv"])?;
            println!(
                "{} items, seed {seed}; checksums in {out_dir}/CHECKSUMS.txt",
                batch.items.len()
            );
        }
        Command::ImportReview {
            dir,
            review,
            out_dir,
            allow_unreviewed,
        } => {
            let dir = Path::new(&dir);
            let batch = TeamBatch {
                stream: Stream::load(dir.join("stream.jsonl").to_str().unwrap())?,
                items: items_from_jsonl(&fs::read_to_string(dir.join("items.jsonl"))?)?,
            };
            let imported = import_review(&batch.items, &fs::read_to_string(&review)?)?;
            let s = &imported.summary;
            ensure!(
                s.unreviewed == 0 || allow_unreviewed,
                "{} of {} items have no reviewer decision; pass --allow-unreviewed to write anyway",
                s.unreviewed,
                s.total
            );
            let reviewed = apply_review(&batch, &imported);
            let out = Path::new(&out_dir);
            fs::create_dir_all(out)?;
            fs::write(out.join("stream.jsonl"), to_jsonl_plain(&reviewed))?;
            fs::write(out.join("items.jsonl"), items_to_jsonl(&reviewed.items))?;
            fs::write(
                out.join("review-summary.json"),
                serde_json::to_string_pretty(s)? + "\n",
            )?;
            let issues = lint_stream_file(out.join("stream.jsonl").to_str().unwrap())?;
            ensure!(issues.is_empty(), "reviewed stream failed lint");
            write_checksums(out, &["stream.jsonl", "items.jsonl", "review-summary.json"])?;
            println!(
                "reviewed {} of {}: kept {}, changed {}, dropped {}, unreviewed {}",
                s.reviewed(),
                s.total,
                s.kept,
                s.changed,
                s.dropped,
                s.unreviewed
            );
        }
    }
    Ok(())
}

fn to_jsonl_plain(batch: &TeamBatch) -> String {
    let mut out =
        String::from("# Reviewed batch; see review-summary.json for what review changed.\n");
    for item in &batch.stream.items {
        out.push_str(&serde_json::to_string(item).expect("stream items serialise"));
        out.push('\n');
    }
    out
}

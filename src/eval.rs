use crate::core::{Context, MemoryChunk, RetrievalResult};
use crate::runner::BenchmarkConfig;
use anyhow::{Context as AnyhowContext, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// A single benchmark item for evaluating memory systems under identical inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub query: String,
    pub context: Context,
    pub gold_memory: String,
    #[serde(default)]
    pub distractors: Vec<String>,
    #[serde(default)]
    pub memories: Vec<String>,
    #[serde(default)]
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvalReport {
    pub generated_at: chrono::DateTime<Utc>,
    pub item_count: usize,
    pub top_k: usize,
    pub systems: Vec<SystemEvalReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvalReport {
    pub system: String,
    pub provider: String,
    pub configured: bool,
    pub item_results: Vec<ItemEvalResult>,
    pub metrics: MemoryEvalMetrics,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryEvalMetrics {
    pub top1_accuracy: f64,
    pub recall_at_k: f64,
    pub precision_at_k: f64,
    pub distractor_activation_rate: f64,
    pub avg_latency_ms: f64,
    pub error_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemEvalResult {
    pub item_id: String,
    pub item_type: String,
    pub query: String,
    pub retrieved: Vec<String>,
    pub hit_top1: bool,
    pub hit_at_k: bool,
    pub distractor_hits: usize,
    pub latency_ms: u64,
    pub error: Option<String>,
}

trait CognitiveMemorySystem {
    fn name(&self) -> &str;
    fn provider(&self) -> &str;
    fn ingest(&mut self, memories: &[MemoryChunk]) -> Result<()>;
    fn retrieve(&mut self, item: &BenchmarkItem, top_k: usize) -> Result<RetrievalResult>;
}

pub fn run_memory_eval(config: &BenchmarkConfig) -> Result<MemoryEvalReport> {
    let items_file = config
        .items_file
        .as_ref()
        .context("--items-file is required for external memory eval mode")?;
    let items = load_benchmark_items(items_file)?;
    let output_path = PathBuf::from(&config.output_dir);
    fs::create_dir_all(&output_path)?;

    let mut reports = Vec::new();
    for system_name in parse_systems(&config.systems) {
        reports.push(evaluate_system(&system_name, &items, config.top_k));
    }

    let report = MemoryEvalReport {
        generated_at: Utc::now(),
        item_count: items.len(),
        top_k: config.top_k,
        systems: reports,
    };

    save_eval_report(&output_path, &report)?;
    print_eval_report(&report);

    Ok(report)
}

fn load_benchmark_items(path: &str) -> Result<Vec<BenchmarkItem>> {
    let input =
        fs::read_to_string(path).with_context(|| format!("failed to read items file: {path}"))?;
    let mut items = Vec::new();

    for (idx, line) in input.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let item: BenchmarkItem = serde_json::from_str(trimmed)
            .with_context(|| format!("invalid JSONL benchmark item at {}:{}", path, idx + 1))?;
        items.push(item);
    }

    anyhow::ensure!(!items.is_empty(), "items file contains no benchmark items");
    Ok(items)
}

fn parse_systems(systems: &str) -> Vec<String> {
    systems
        .split(',')
        .map(str::trim)
        .filter(|system| !system.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn evaluate_system(system_name: &str, items: &[BenchmarkItem], top_k: usize) -> SystemEvalReport {
    let mut system = match build_system(system_name) {
        Ok(system) => system,
        Err(error) => {
            return SystemEvalReport {
                system: system_name.to_string(),
                provider: system_name.to_string(),
                configured: false,
                item_results: Vec::new(),
                metrics: MemoryEvalMetrics {
                    error_rate: 1.0,
                    ..MemoryEvalMetrics::default()
                },
                error: Some(error.to_string()),
            };
        }
    };

    let memories = build_memory_chunks(items);
    if let Err(error) = system.ingest(&memories) {
        return SystemEvalReport {
            system: system.name().to_string(),
            provider: system.provider().to_string(),
            configured: true,
            item_results: Vec::new(),
            metrics: MemoryEvalMetrics {
                error_rate: 1.0,
                ..MemoryEvalMetrics::default()
            },
            error: Some(error.to_string()),
        };
    }

    let mut item_results = Vec::with_capacity(items.len());
    for item in items {
        item_results.push(evaluate_item(system.as_mut(), item, top_k));
    }

    let metrics = compute_eval_metrics(&item_results);
    SystemEvalReport {
        system: system.name().to_string(),
        provider: system.provider().to_string(),
        configured: true,
        item_results,
        metrics,
        error: None,
    }
}

fn build_system(system_name: &str) -> Result<Box<dyn CognitiveMemorySystem>> {
    match system_name {
        "local" | "baseline" => Ok(Box::new(LocalMemorySystem::new(
            "local",
            ScoringMode::Local,
        ))),
        "rag" | "vanilla-rag" => Ok(Box::new(LocalMemorySystem::new("rag", ScoringMode::Rag))),
        "agent" | "agent-memory" => Ok(Box::new(LocalMemorySystem::new(
            "agent-memory",
            ScoringMode::AgentMemory,
        ))),
        "organic" | "organic-memory" | "achiral" => Ok(Box::new(LocalMemorySystem::new(
            "organic-memory",
            ScoringMode::OrganicMemory,
        ))),
        "mem0" | "supermemory" | "zep" | "letta" => {
            Ok(Box::new(HttpMemorySystem::from_env(system_name)?))
        }
        _ => Err(anyhow::anyhow!("unknown memory system: {system_name}")),
    }
}

fn build_memory_chunks(items: &[BenchmarkItem]) -> Vec<MemoryChunk> {
    let mut chunks = Vec::new();
    for item in items {
        chunks.push(MemoryChunk::new(
            format!("{}:gold", item.id),
            item.gold_memory.clone(),
            item.context.clone(),
        ));

        for (idx, distractor) in item.distractors.iter().enumerate() {
            chunks.push(MemoryChunk::new(
                format!("{}:distractor:{idx}", item.id),
                distractor.clone(),
                item.context.clone(),
            ));
        }

        for (idx, memory) in item.memories.iter().enumerate() {
            chunks.push(MemoryChunk::new(
                format!("{}:memory:{idx}", item.id),
                memory.clone(),
                item.context.clone(),
            ));
        }
    }
    chunks
}

fn evaluate_item(
    system: &mut dyn CognitiveMemorySystem,
    item: &BenchmarkItem,
    top_k: usize,
) -> ItemEvalResult {
    match system.retrieve(item, top_k) {
        Ok(result) => {
            let retrieved: Vec<String> = result
                .retrieved_chunks
                .iter()
                .map(|chunk| chunk.content.clone())
                .collect();
            let hit_top1 = retrieved
                .first()
                .is_some_and(|content| matches_expected(content, &item.gold_memory));
            let hit_at_k = retrieved
                .iter()
                .any(|content| matches_expected(content, &item.gold_memory));
            let distractor_hits = retrieved
                .iter()
                .filter(|content| {
                    item.distractors
                        .iter()
                        .any(|distractor| matches_expected(content, distractor))
                })
                .count();

            ItemEvalResult {
                item_id: item.id.clone(),
                item_type: item.item_type.clone(),
                query: item.query.clone(),
                retrieved,
                hit_top1,
                hit_at_k,
                distractor_hits,
                latency_ms: result.retrieval_latency_ms,
                error: None,
            }
        }
        Err(error) => ItemEvalResult {
            item_id: item.id.clone(),
            item_type: item.item_type.clone(),
            query: item.query.clone(),
            retrieved: Vec::new(),
            hit_top1: false,
            hit_at_k: false,
            distractor_hits: 0,
            latency_ms: 0,
            error: Some(error.to_string()),
        },
    }
}

fn compute_eval_metrics(results: &[ItemEvalResult]) -> MemoryEvalMetrics {
    if results.is_empty() {
        return MemoryEvalMetrics::default();
    }

    let item_count = results.len() as f64;
    let errors = results
        .iter()
        .filter(|result| result.error.is_some())
        .count() as f64;
    let top1_hits = results.iter().filter(|result| result.hit_top1).count() as f64;
    let recall_hits = results.iter().filter(|result| result.hit_at_k).count() as f64;
    let retrieved_count: usize = results.iter().map(|result| result.retrieved.len()).sum();
    let distractor_hits: usize = results.iter().map(|result| result.distractor_hits).sum();
    let avg_latency_ms = results
        .iter()
        .map(|result| result.latency_ms as f64)
        .sum::<f64>()
        / item_count;

    MemoryEvalMetrics {
        top1_accuracy: top1_hits / item_count,
        recall_at_k: recall_hits / item_count,
        precision_at_k: if retrieved_count == 0 {
            0.0
        } else {
            recall_hits / retrieved_count as f64
        },
        distractor_activation_rate: if retrieved_count == 0 {
            0.0
        } else {
            distractor_hits as f64 / retrieved_count as f64
        },
        avg_latency_ms,
        error_rate: errors / item_count,
    }
}

fn save_eval_report(output_path: &Path, report: &MemoryEvalReport) -> Result<()> {
    let json_path = output_path.join("memory_eval_results.json");
    fs::write(&json_path, serde_json::to_string_pretty(report)?)?;
    println!("\nMemory eval JSON saved to: {}", json_path.display());

    let csv_path = output_path.join("memory_eval_metrics.csv");
    let mut wtr = csv::Writer::from_path(csv_path.clone())?;
    wtr.write_record(["system", "provider", "configured", "metric", "value"])?;
    for system in &report.systems {
        let configured = system.configured.to_string();
        write_metric(
            &mut wtr,
            system,
            &configured,
            "top1_accuracy",
            system.metrics.top1_accuracy,
        )?;
        write_metric(
            &mut wtr,
            system,
            &configured,
            "recall_at_k",
            system.metrics.recall_at_k,
        )?;
        write_metric(
            &mut wtr,
            system,
            &configured,
            "precision_at_k",
            system.metrics.precision_at_k,
        )?;
        write_metric(
            &mut wtr,
            system,
            &configured,
            "distractor_activation_rate",
            system.metrics.distractor_activation_rate,
        )?;
        write_metric(
            &mut wtr,
            system,
            &configured,
            "avg_latency_ms",
            system.metrics.avg_latency_ms,
        )?;
        write_metric(
            &mut wtr,
            system,
            &configured,
            "error_rate",
            system.metrics.error_rate,
        )?;
    }
    wtr.flush()?;
    println!("Memory eval CSV saved to: {}", csv_path.display());

    let pdf_path = output_path.join("memory_eval_report.pdf");
    let lines = memory_eval_report_lines(report);
    fs::write(
        &pdf_path,
        render_text_pdf("Cognoscenti Memory Eval Report", &lines),
    )?;
    println!("Memory eval PDF saved to: {}", pdf_path.display());

    Ok(())
}

fn write_metric(
    wtr: &mut csv::Writer<std::fs::File>,
    system: &SystemEvalReport,
    configured: &str,
    metric: &str,
    value: f64,
) -> csv::Result<()> {
    wtr.write_record([
        system.system.as_str(),
        system.provider.as_str(),
        configured,
        metric,
        &value.to_string(),
    ])
}

fn print_eval_report(report: &MemoryEvalReport) {
    println!("\n=== Memory Tool Eval Results ===");
    println!("Items: {} | Top-k: {}", report.item_count, report.top_k);
    for system in &report.systems {
        if let Some(error) = &system.error {
            println!("  {}: not configured ({})", system.system, error);
            continue;
        }

        println!(
            "  {}: top1 {:.2}% | recall@k {:.2}% | precision@k {:.2}% | distractor {:.2}% | avg latency {:.2}ms",
            system.system,
            system.metrics.top1_accuracy * 100.0,
            system.metrics.recall_at_k * 100.0,
            system.metrics.precision_at_k * 100.0,
            system.metrics.distractor_activation_rate * 100.0,
            system.metrics.avg_latency_ms
        );
    }
}

fn memory_eval_report_lines(report: &MemoryEvalReport) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Generated: {}",
            report.generated_at.format("%Y-%m-%d %H:%M:%S UTC")
        ),
        format!("Items: {}", report.item_count),
        format!("Top-k: {}", report.top_k),
        String::new(),
        "System comparison".to_string(),
    ];

    for system in &report.systems {
        lines.push(String::new());
        lines.push(format!("System: {}", system.system));
        if let Some(error) = &system.error {
            lines.push(format!("Status: not configured ({error})"));
            continue;
        }

        lines.push(format!(
            "Top-1 Accuracy: {:.2}%",
            system.metrics.top1_accuracy * 100.0
        ));
        lines.push(format!(
            "Recall@k: {:.2}%",
            system.metrics.recall_at_k * 100.0
        ));
        lines.push(format!(
            "Precision@k: {:.2}%",
            system.metrics.precision_at_k * 100.0
        ));
        lines.push(format!(
            "Distractor Activation: {:.2}%",
            system.metrics.distractor_activation_rate * 100.0
        ));
        lines.push(format!(
            "Avg Latency: {:.2}ms",
            system.metrics.avg_latency_ms
        ));
        lines.push(format!(
            "Error Rate: {:.2}%",
            system.metrics.error_rate * 100.0
        ));
    }

    lines
}

struct LocalMemorySystem {
    name: String,
    scoring_mode: ScoringMode,
    memories: Vec<MemoryChunk>,
}

#[derive(Debug, Clone, Copy)]
enum ScoringMode {
    Local,
    Rag,
    AgentMemory,
    OrganicMemory,
}

impl LocalMemorySystem {
    fn new(name: &str, scoring_mode: ScoringMode) -> Self {
        Self {
            name: name.to_string(),
            scoring_mode,
            memories: Vec::new(),
        }
    }
}

impl CognitiveMemorySystem for LocalMemorySystem {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider(&self) -> &str {
        "local"
    }

    fn ingest(&mut self, memories: &[MemoryChunk]) -> Result<()> {
        self.memories = memories.to_vec();
        Ok(())
    }

    fn retrieve(&mut self, item: &BenchmarkItem, top_k: usize) -> Result<RetrievalResult> {
        let start = Instant::now();
        let mut scored: Vec<(f64, MemoryChunk)> = self
            .memories
            .iter()
            .cloned()
            .map(|chunk| {
                (
                    score_memory(&item.query, &item.context, &chunk, self.scoring_mode),
                    chunk,
                )
            })
            .collect();
        scored.sort_by(|(left, _), (right, _)| {
            right.partial_cmp(left).unwrap_or(std::cmp::Ordering::Equal)
        });

        let retrieved_chunks: Vec<MemoryChunk> = scored
            .into_iter()
            .take(top_k)
            .map(|(_, chunk)| chunk)
            .collect();
        let confidence_scores = retrieved_chunks
            .iter()
            .map(|chunk| {
                score_memory(&item.query, &item.context, chunk, self.scoring_mode).min(1.0)
            })
            .collect();

        Ok(RetrievalResult {
            memories_examined: self.memories.len() as u32,
            retrieved_chunks,
            retrieval_latency_ms: start.elapsed().as_millis() as u64,
            confidence_scores,
            timestamp: Utc::now(),
        })
    }
}

struct HttpMemorySystem {
    name: String,
    ingest_url: String,
    retrieve_url: String,
    api_key: Option<String>,
}

impl HttpMemorySystem {
    fn from_env(name: &str) -> Result<Self> {
        let prefix = format!("COGNOSCENTI_{}", name.to_ascii_uppercase());
        let ingest_url = std::env::var(format!("{prefix}_INGEST_URL"))
            .with_context(|| format!("missing {prefix}_INGEST_URL"))?;
        let retrieve_url = std::env::var(format!("{prefix}_RETRIEVE_URL"))
            .with_context(|| format!("missing {prefix}_RETRIEVE_URL"))?;
        let api_key = std::env::var(format!("{prefix}_API_KEY")).ok();

        Ok(Self {
            name: name.to_string(),
            ingest_url,
            retrieve_url,
            api_key,
        })
    }
}

impl CognitiveMemorySystem for HttpMemorySystem {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider(&self) -> &str {
        &self.name
    }

    fn ingest(&mut self, memories: &[MemoryChunk]) -> Result<()> {
        let payload = json!({
            "system": self.name,
            "memories": memories,
        });
        run_json_post(&self.ingest_url, self.api_key.as_deref(), &payload)?;
        Ok(())
    }

    fn retrieve(&mut self, item: &BenchmarkItem, top_k: usize) -> Result<RetrievalResult> {
        let start = Instant::now();
        let payload = json!({
            "system": self.name,
            "query": item.query,
            "context": item.context,
            "top_k": top_k,
        });
        let response = run_json_post(&self.retrieve_url, self.api_key.as_deref(), &payload)?;
        let texts = extract_memory_texts(&response, top_k);
        let retrieved_chunks: Vec<MemoryChunk> = texts
            .into_iter()
            .enumerate()
            .map(|(idx, content)| {
                MemoryChunk::new(
                    format!("{}:retrieved:{idx}", item.id),
                    content,
                    item.context.clone(),
                )
            })
            .collect();
        let confidence_scores = extract_scores(&response, retrieved_chunks.len());

        Ok(RetrievalResult {
            memories_examined: retrieved_chunks.len() as u32,
            retrieved_chunks,
            retrieval_latency_ms: start.elapsed().as_millis() as u64,
            confidence_scores,
            timestamp: Utc::now(),
        })
    }
}

fn run_json_post(url: &str, api_key: Option<&str>, payload: &Value) -> Result<Value> {
    let mut command = Command::new("curl");
    command
        .arg("-sS")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json");

    if let Some(api_key) = api_key {
        command
            .arg("-H")
            .arg(format!("Authorization: Bearer {api_key}"));
    }

    let output = command
        .arg("-d")
        .arg(payload.to_string())
        .arg(url)
        .output()
        .with_context(|| "failed to run curl for memory provider request")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "provider request failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "provider returned non-JSON response: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn score_memory(query: &str, context: &Context, chunk: &MemoryChunk, mode: ScoringMode) -> f64 {
    let query_terms = normalized_terms(query);
    let content_terms = normalized_terms(&chunk.content);
    let overlap = query_terms.intersection(&content_terms).count() as f64;
    let content = chunk.content.to_ascii_lowercase();
    let domain_bonus = if context.domain == chunk.context.domain {
        0.25
    } else {
        0.0
    };
    let project_bonus = if context.project.is_some() && context.project == chunk.context.project {
        0.2
    } else {
        0.0
    };
    let conversation_bonus = if context.conversation_id.is_some()
        && context.conversation_id == chunk.context.conversation_id
    {
        0.35
    } else {
        0.0
    };

    match mode {
        ScoringMode::Rag => overlap,
        ScoringMode::Local => overlap + domain_bonus + chunk.importance,
        ScoringMode::AgentMemory => overlap + domain_bonus + project_bonus + conversation_bonus,
        ScoringMode::OrganicMemory => {
            let lifecycle_bonus = lifecycle_signal(&content);
            overlap + domain_bonus + project_bonus + conversation_bonus + lifecycle_bonus
        }
    }
}

fn lifecycle_signal(content: &str) -> f64 {
    let mut score = 0.0;

    for marker in [
        "validated",
        "current",
        "reinforced",
        "approved",
        "reused",
        "high-signal",
        "latest",
    ] {
        if content.contains(marker) {
            score += 0.65;
        }
    }

    for marker in [
        "stale",
        "superseded",
        "deprecated",
        "outdated",
        "low-signal",
        "ignore",
        "historical only",
    ] {
        if content.contains(marker) {
            score -= 0.85;
        }
    }

    score
}

fn normalized_terms(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|term| term.len() > 2)
        .map(str::to_ascii_lowercase)
        .collect()
}

fn matches_expected(candidate: &str, expected: &str) -> bool {
    let candidate = candidate.to_ascii_lowercase();
    let expected = expected.to_ascii_lowercase();
    candidate.contains(&expected) || expected.contains(&candidate)
}

fn extract_memory_texts(value: &Value, limit: usize) -> Vec<String> {
    let mut texts = Vec::new();
    collect_memory_texts(value, &mut texts, limit);
    texts
}

fn collect_memory_texts(value: &Value, texts: &mut Vec<String>, limit: usize) {
    if texts.len() >= limit {
        return;
    }

    match value {
        Value::Array(items) => {
            for item in items {
                collect_memory_texts(item, texts, limit);
            }
        }
        Value::Object(map) => {
            for key in ["content", "memory", "text", "document", "value"] {
                if let Some(Value::String(text)) = map.get(key) {
                    texts.push(text.clone());
                    if texts.len() >= limit {
                        return;
                    }
                }
            }

            for nested in map.values() {
                collect_memory_texts(nested, texts, limit);
            }
        }
        Value::String(text) => texts.push(text.clone()),
        _ => {}
    }
}

fn extract_scores(value: &Value, count: usize) -> Vec<f64> {
    let mut scores = Vec::new();
    collect_scores(value, &mut scores, count);
    if scores.len() < count {
        scores.resize(count, 1.0);
    }
    scores
}

fn collect_scores(value: &Value, scores: &mut Vec<f64>, limit: usize) {
    if scores.len() >= limit {
        return;
    }

    match value {
        Value::Array(items) => {
            for item in items {
                collect_scores(item, scores, limit);
            }
        }
        Value::Object(map) => {
            for key in ["score", "confidence", "similarity"] {
                if let Some(score) = map.get(key).and_then(Value::as_f64) {
                    scores.push(score);
                    if scores.len() >= limit {
                        return;
                    }
                }
            }

            for nested in map.values() {
                collect_scores(nested, scores, limit);
            }
        }
        _ => {}
    }
}

fn render_text_pdf(title: &str, lines: &[String]) -> Vec<u8> {
    let mut content = String::from("BT\n/F1 18 Tf\n50 790 Td\n");
    content.push_str(&format!("({}) Tj\n", escape_pdf_text(title)));
    content.push_str("/F1 10 Tf\n0 -24 Td\n");

    for line in lines {
        content.push_str(&format!("({}) Tj\n0 -14 Td\n", escape_pdf_text(line)));
    }
    content.push_str("ET\n");

    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            content.len(),
            content
        ),
    ];

    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (idx, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", idx + 1, object).as_bytes());
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref_offset
        )
        .as_bytes(),
    );
    pdf
}

fn escape_pdf_text(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Domain;

    fn test_context() -> Context {
        Context {
            project: Some("Test".to_string()),
            domain: Domain::Engineering,
            conversation_id: Some("test-1".to_string()),
            participants: vec!["agent".to_string()],
        }
    }

    #[test]
    fn local_baseline_retrieves_gold_memory() {
        let item = BenchmarkItem {
            id: "activation-1".to_string(),
            item_type: "activation".to_string(),
            query: "What auth bug was fixed?".to_string(),
            context: test_context(),
            gold_memory: "Fixed authentication bug in login flow".to_string(),
            distractors: vec!["Reviewed pricing page copy".to_string()],
            memories: Vec::new(),
            rationale: None,
        };
        let report = evaluate_system("local", &[item], 3);

        assert!(report.configured);
        assert_eq!(report.metrics.recall_at_k, 1.0);
        assert!(report.metrics.top1_accuracy > 0.0);
    }

    #[test]
    fn parses_comma_separated_systems() {
        assert_eq!(
            parse_systems("local, mem0,zep"),
            vec!["local", "mem0", "zep"]
        );
    }
}

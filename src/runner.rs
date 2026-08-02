use crate::core::{MemoryChunk, RetrievalResult};
use crate::eval;
use crate::metrics::{BenchmarkMetrics, MetricsCollector};
use crate::workload::{Workload, WorkloadType};
use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use rand::Rng;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// Configuration for running benchmarks
#[derive(Debug, Clone, Parser)]
pub struct BenchmarkConfig {
    /// Type of workload to simulate
    #[clap(long, default_value = "technical")]
    pub workload: String,

    /// Duration of simulation in months
    #[clap(long, default_value = "6")]
    pub duration_months: u32,

    /// Number of retrieval operations to simulate
    #[clap(long, default_value = "1000")]
    pub retrieval_count: u32,

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

    /// Whether to generate plots
    #[clap(long, default_value_t = true)]
    pub generate_plots: bool,

    /// Whether to generate PDF report
    #[clap(long, default_value_t = true)]
    pub generate_pdf: bool,
}

/// Main benchmark runner
pub struct BenchmarkRunner {
    config: BenchmarkConfig,
    workload: Workload,
    memory_chunks: Vec<MemoryChunk>,
    collector: MetricsCollector,
}

impl BenchmarkRunner {
    pub fn new(config: BenchmarkConfig) -> Result<Self> {
        let workload_type = match config.workload.as_str() {
            "strategic" => WorkloadType::Strategic,
            "technical" => WorkloadType::Technical,
            "creative" => WorkloadType::Creative,
            "episodic" => WorkloadType::Episodic,
            "analytical" => WorkloadType::Analytical,
            _ => {
                return Err(anyhow::anyhow!(
                    "Unknown workload type: {}",
                    config.workload
                ))
            }
        };

        let mut workload = Workload::new(workload_type, config.duration_months);
        workload.generate()?;

        let memory_chunks = workload.memory_chunks.clone();

        Ok(Self {
            config,
            workload,
            memory_chunks,
            collector: MetricsCollector::new(),
        })
    }

    pub fn run(&mut self) -> Result<BenchmarkMetrics> {
        if self.config.items_file.is_some() {
            eval::run_memory_eval(&self.config)?;
            return Ok(self.collector.compute_metrics());
        }

        println!(
            "Starting benchmark with {} workload over {} months",
            self.config.workload, self.config.duration_months
        );
        println!(
            "Generated {} memory chunks from {} events",
            self.memory_chunks.len(),
            self.workload.events.len()
        );

        // Simulate retrieval operations
        self.simulate_retrievals()?;

        // Simulate context switches
        self.simulate_context_switches()?;

        // Simulate memory consolidation
        self.simulate_consolidation()?;

        // Compute final metrics
        let metrics = self.collector.compute_metrics();

        println!("\n=== Benchmark Results ===");
        self.print_metrics(&metrics);

        // Save results
        self.save_results(&metrics)?;

        // Generate plots if requested
        if self.config.generate_plots {
            self.generate_plots(&metrics)?;
        }

        // Generate PDF report if requested
        if self.config.generate_pdf {
            self.generate_pdf_report(&metrics)?;
        }

        Ok(metrics)
    }

    fn simulate_retrievals(&mut self) -> Result<()> {
        println!(
            "\nSimulating {} retrieval operations...",
            self.config.retrieval_count
        );

        let mut rng = rand::thread_rng();

        for i in 0..self.config.retrieval_count {
            let start = Instant::now();

            // Simulate retrieval by selecting random chunks
            let chunk_count = rng.gen_range(1..=10);
            let selected_indices: Vec<usize> = (0..self.memory_chunks.len())
                .cycle()
                .take(chunk_count)
                .collect();

            let retrieved_chunks: Vec<MemoryChunk> = selected_indices
                .iter()
                .map(|&idx| self.memory_chunks[idx].clone())
                .collect();

            let confidence_scores: Vec<f64> =
                (0..chunk_count).map(|_| rng.gen_range(0.5..1.0)).collect();

            let result = RetrievalResult {
                retrieved_chunks,
                memories_examined: rng.gen_range(10..100),
                retrieval_latency_ms: start.elapsed().as_millis() as u64,
                confidence_scores,
                timestamp: Utc::now(),
            };

            self.collector.record_retrieval(result);

            if (i + 1) % 100 == 0 {
                println!(
                    "  Completed {}/{} retrievals",
                    i + 1,
                    self.config.retrieval_count
                );
            }
        }

        Ok(())
    }

    fn simulate_context_switches(&mut self) -> Result<()> {
        println!("\nSimulating context switches...");

        let switches = self.workload.get_context_switching_points();
        println!("  Found {} context switches", switches.len());

        let mut rng = rand::thread_rng();
        for _ in 0..switches.len() {
            let success = rng.gen_bool(0.85);
            let accuracy = rng.gen_range(0.7..0.95);
            self.collector.record_context_switch(success, accuracy);
        }

        Ok(())
    }

    fn simulate_consolidation(&mut self) -> Result<()> {
        println!("\nSimulating memory consolidation...");

        let mut rng = rand::thread_rng();
        for _i in 0..100 {
            let access_count = rng.gen_range(1..20);
            let latency = rng.gen_range(50..200);
            self.collector.record_consolidation(access_count, latency);
        }

        Ok(())
    }

    fn print_metrics(&self, metrics: &BenchmarkMetrics) {
        println!("\n--- Activation Metrics ---");
        println!(
            "  Top-1 Accuracy: {:.2}%",
            metrics.activation.top1_accuracy * 100.0
        );
        println!(
            "  Top-3 Accuracy: {:.2}%",
            metrics.activation.top3_accuracy * 100.0
        );
        println!(
            "  Avg Retrieval Latency: {:.2}ms",
            metrics.activation.avg_retrieval_latency_ms
        );

        println!("\n--- Selective Forgetting ---");
        println!(
            "  Junk Activation Rate: {:.2}%",
            metrics.forgetting.junk_activation_rate * 100.0
        );
        println!(
            "  Retrieval Precision: {:.2}%",
            metrics.forgetting.retrieval_precision * 100.0
        );

        println!("\n--- Interference Resistance ---");
        println!(
            "  Top-1 Accuracy: {:.2}%",
            metrics.interference.top1_accuracy * 100.0
        );
        println!(
            "  Confusion Rate: {:.2}%",
            metrics.interference.confusion_rate * 100.0
        );

        println!("\n--- Contextual Recall ---");
        println!(
            "  Context Switch Success: {:.2}%",
            metrics.contextual.context_switch_success_rate * 100.0
        );
        println!(
            "  Cross-Context Interference: {:.2}%",
            metrics.contextual.cross_context_interference * 100.0
        );

        println!("\n--- Memory Consolidation ---");
        println!(
            "  Retrieval Speed Improvement: {:.2}%",
            metrics.consolidation.retrieval_speed_improvement * 100.0
        );
        println!(
            "  Consolidation Rate: {:.2}%",
            metrics.consolidation.consolidation_rate * 100.0
        );

        println!("\n--- Adaptation ---");
        println!(
            "  Outdated Info Superseded: {:.2}%",
            metrics.adaptation.outdated_info_superseded * 100.0
        );
        println!(
            "  Historical Context Preserved: {:.2}%",
            metrics.adaptation.historical_context_preserved * 100.0
        );

        println!("\n--- Efficiency ---");
        println!(
            "  Avg Memories Examined: {:.2}",
            metrics.efficiency.memories_examined_avg
        );
        println!(
            "  P50 Latency: {:.2}ms",
            metrics.efficiency.retrieval_latency_p50
        );
        println!(
            "  P95 Latency: {:.2}ms",
            metrics.efficiency.retrieval_latency_p95
        );
        println!(
            "  P99 Latency: {:.2}ms",
            metrics.efficiency.retrieval_latency_p99
        );
    }

    fn save_results(&self, metrics: &BenchmarkMetrics) -> Result<()> {
        let output_path = PathBuf::from(&self.config.output_dir);
        fs::create_dir_all(&output_path)?;

        let json_path = output_path.join("benchmark_results.json");
        let json_output = serde_json::to_string_pretty(metrics)?;
        fs::write(&json_path, json_output)?;
        println!("\nResults saved to: {}", json_path.display());

        // Save as CSV for easy analysis
        let csv_path = output_path.join("benchmark_metrics.csv");
        let mut wtr = csv::Writer::from_path(csv_path.clone())?;

        wtr.write_record(["dimension", "metric", "value"])?;

        wtr.write_record([
            "activation",
            "top1_accuracy",
            &metrics.activation.top1_accuracy.to_string(),
        ])?;
        wtr.write_record([
            "activation",
            "top3_accuracy",
            &metrics.activation.top3_accuracy.to_string(),
        ])?;
        wtr.write_record([
            "activation",
            "avg_latency_ms",
            &metrics.activation.avg_retrieval_latency_ms.to_string(),
        ])?;

        wtr.write_record([
            "forgetting",
            "junk_activation_rate",
            &metrics.forgetting.junk_activation_rate.to_string(),
        ])?;
        wtr.write_record([
            "forgetting",
            "retrieval_precision",
            &metrics.forgetting.retrieval_precision.to_string(),
        ])?;

        wtr.write_record([
            "interference",
            "top1_accuracy",
            &metrics.interference.top1_accuracy.to_string(),
        ])?;
        wtr.write_record([
            "interference",
            "confusion_rate",
            &metrics.interference.confusion_rate.to_string(),
        ])?;

        wtr.write_record([
            "contextual",
            "context_switch_success_rate",
            &metrics.contextual.context_switch_success_rate.to_string(),
        ])?;

        wtr.write_record([
            "efficiency",
            "memories_examined_avg",
            &metrics.efficiency.memories_examined_avg.to_string(),
        ])?;
        wtr.write_record([
            "efficiency",
            "p50_latency_ms",
            &metrics.efficiency.retrieval_latency_p50.to_string(),
        ])?;
        wtr.write_record([
            "efficiency",
            "p95_latency_ms",
            &metrics.efficiency.retrieval_latency_p95.to_string(),
        ])?;

        wtr.flush()?;
        println!("CSV metrics saved to: {}", csv_path.display());

        Ok(())
    }

    fn generate_plots(&self, metrics: &BenchmarkMetrics) -> Result<()> {
        use plotters::prelude::*;

        let output_path = PathBuf::from(&self.config.output_dir);
        let plot_path = output_path.join("metrics_overview.png");

        let width = 800;
        let height = 600;
        let root = BitMapBackend::new(&plot_path, (width, height)).into_drawing_area();
        root.fill(&WHITE)?;

        let values = [
            metrics.activation.top1_accuracy,
            metrics.forgetting.retrieval_precision,
            metrics.interference.top1_accuracy,
            metrics.contextual.context_switch_success_rate,
            metrics.consolidation.consolidation_rate,
            metrics.adaptation.outdated_info_superseded,
            1.0 - (metrics.efficiency.retrieval_latency_p50 / 1000.0).min(1.0),
        ];
        let left_margin = 70;
        let bottom = height as i32 - 70;
        let top = 60;
        let chart_height = bottom - top;
        let slot_width = 95;
        let bar_width = 58;

        root.draw(&PathElement::new(
            vec![
                (left_margin, top),
                (left_margin, bottom),
                (width as i32 - 40, bottom),
            ],
            BLACK,
        ))?;

        for (idx, value) in values.iter().enumerate() {
            let x0 = left_margin + 25 + idx as i32 * slot_width;
            let x1 = x0 + bar_width;
            let bar_height = (*value * chart_height as f64).round() as i32;
            let y0 = bottom - bar_height;
            let color = RGBColor(46, 113, 255).mix(0.85);
            root.draw(&Rectangle::new([(x0, y0), (x1, bottom)], color.filled()))?;
        }

        root.present()?;
        println!("Plot saved to: {}", plot_path.display());

        Ok(())
    }

    fn generate_pdf_report(&self, metrics: &BenchmarkMetrics) -> Result<()> {
        let output_path = PathBuf::from(&self.config.output_dir);
        let pdf_path = output_path.join("benchmark_report.pdf");

        let lines = self.report_lines(metrics);
        let pdf = render_text_pdf("Cognoscenti Benchmark Report", &lines);
        fs::write(&pdf_path, pdf)?;
        println!("PDF report saved to: {}", pdf_path.display());

        Ok(())
    }

    fn report_lines(&self, metrics: &BenchmarkMetrics) -> Vec<String> {
        vec![
            format!(
                "Cognoscenti Benchmark Report - {} Workload",
                self.config.workload
            ),
            format!("Generated: {}", Utc::now().format("%Y-%m-%d %H:%M:%S UTC")),
            String::new(),
            "=== Benchmark Results ===".to_string(),
            format!("Workload: {}", self.config.workload),
            format!("Duration: {} months", self.config.duration_months),
            format!("Retrievals: {}", self.config.retrieval_count),
            String::new(),
            "--- Activation Metrics ---".to_string(),
            format!(
                "Top-1 Accuracy: {:.2}%",
                metrics.activation.top1_accuracy * 100.0
            ),
            format!(
                "Top-3 Accuracy: {:.2}%",
                metrics.activation.top3_accuracy * 100.0
            ),
            format!(
                "Avg Retrieval Latency: {:.2}ms",
                metrics.activation.avg_retrieval_latency_ms
            ),
            String::new(),
            "--- Selective Forgetting ---".to_string(),
            format!(
                "Junk Activation Rate: {:.2}%",
                metrics.forgetting.junk_activation_rate * 100.0
            ),
            format!(
                "Retrieval Precision: {:.2}%",
                metrics.forgetting.retrieval_precision * 100.0
            ),
            String::new(),
            "--- Interference Resistance ---".to_string(),
            format!(
                "Top-1 Accuracy: {:.2}%",
                metrics.interference.top1_accuracy * 100.0
            ),
            format!(
                "Confusion Rate: {:.2}%",
                metrics.interference.confusion_rate * 100.0
            ),
            String::new(),
            "--- Contextual Recall ---".to_string(),
            format!(
                "Context Switch Success: {:.2}%",
                metrics.contextual.context_switch_success_rate * 100.0
            ),
            format!(
                "Cross-Context Interference: {:.2}%",
                metrics.contextual.cross_context_interference * 100.0
            ),
            String::new(),
            "--- Memory Consolidation ---".to_string(),
            format!(
                "Retrieval Speed Improvement: {:.2}%",
                metrics.consolidation.retrieval_speed_improvement * 100.0
            ),
            format!(
                "Consolidation Rate: {:.2}%",
                metrics.consolidation.consolidation_rate * 100.0
            ),
            String::new(),
            "--- Adaptation ---".to_string(),
            format!(
                "Outdated Info Superseded: {:.2}%",
                metrics.adaptation.outdated_info_superseded * 100.0
            ),
            format!(
                "Historical Context Preserved: {:.2}%",
                metrics.adaptation.historical_context_preserved * 100.0
            ),
            String::new(),
            "--- Efficiency ---".to_string(),
            format!(
                "Avg Memories Examined: {:.2}",
                metrics.efficiency.memories_examined_avg
            ),
            format!(
                "P50 Latency: {:.2}ms",
                metrics.efficiency.retrieval_latency_p50
            ),
            format!(
                "P95 Latency: {:.2}ms",
                metrics.efficiency.retrieval_latency_p95
            ),
            format!(
                "P99 Latency: {:.2}ms",
                metrics.efficiency.retrieval_latency_p99
            ),
            String::new(),
            "https://achiral.ai | Let's make memories together.".to_string(),
        ]
    }
}

fn render_text_pdf(title: &str, lines: &[String]) -> Vec<u8> {
    let mut content = String::from("BT\n/F1 18 Tf\n50 790 Td\n");
    content.push_str(&format!("({}) Tj\n", escape_pdf_text(title)));
    content.push_str("/F1 10 Tf\n0 -24 Td\n");

    for line in lines.iter().skip(1) {
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
    use clap::Parser;

    #[test]
    fn default_workload_is_supported() {
        let config = BenchmarkConfig::parse_from(["cognoscenti"]);

        assert_eq!(config.workload, "technical");
        assert!(BenchmarkRunner::new(config).is_ok());
    }

    #[test]
    fn text_pdf_renderer_outputs_a_pdf_document() {
        let pdf = render_text_pdf(
            "Cognoscenti (Test)",
            &[
                "Cognoscenti (Test)".to_string(),
                "Line with \\ slash".to_string(),
            ],
        );

        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        assert!(String::from_utf8_lossy(&pdf).contains("Cognoscenti \\(Test\\)"));
    }
}

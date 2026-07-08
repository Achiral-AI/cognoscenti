use crate::core::{MemoryChunk, RetrievalResult};
use crate::metrics::{BenchmarkMetrics, MetricsCollector};
use crate::workload::{Workload, WorkloadType};
use rand::Rng;
use anyhow::Result;
use chrono::Utc;
use clap::Parser;
use serde_json;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// Configuration for running benchmarks
#[derive(Debug, Clone, Parser)]
pub struct BenchmarkConfig {
    /// Type of workload to simulate
    #[clap(long, default_value = "engineers")]
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
    
    /// Whether to generate plots
    #[clap(long, default_value = "true")]
    pub generate_plots: bool,
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
            _ => return Err(anyhow::anyhow!("Unknown workload type: {}", config.workload)),
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
        println!("Starting benchmark with {} workload over {} months", 
                 self.config.workload, self.config.duration_months);
        println!("Generated {} memory chunks from {} events",
                 self.memory_chunks.len(), self.workload.events.len());

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

        Ok(metrics)
    }

    fn simulate_retrievals(&mut self) -> Result<()> {
        println!("\nSimulating {} retrieval operations...", self.config.retrieval_count);
        
        let mut rng = rand::thread_rng();
        
        for i in 0..self.config.retrieval_count {
            let start = Instant::now();
            
            // Simulate retrieval by selecting random chunks
            let chunk_count = rng.gen_range(1..=10);
            let selected_indices: Vec<usize> = (0..self.memory_chunks.len())
                .collect::<Vec<_>>()
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .iter()
                .cloned()
                .cycle()
                .take(chunk_count)
                .collect();
            
            let retrieved_chunks: Vec<MemoryChunk> = selected_indices
                .iter()
                .map(|&idx| self.memory_chunks[idx].clone())
                .collect();

            let confidence_scores: Vec<f64> = (0..chunk_count)
                .map(|_| rng.gen_range(0.5..1.0))
                .collect();

            let result = RetrievalResult {
                retrieved_chunks,
                memories_examined: rng.gen_range(10..100),
                retrieval_latency_ms: start.elapsed().as_millis() as u64,
                confidence_scores,
                timestamp: Utc::now(),
            };

            self.collector.record_retrieval(result);

            if (i + 1) % 100 == 0 {
                println!("  Completed {}/{} retrievals", i + 1, self.config.retrieval_count);
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
        println!("  Top-1 Accuracy: {:.2}%", metrics.activation.top1_accuracy * 100.0);
        println!("  Top-3 Accuracy: {:.2}%", metrics.activation.top3_accuracy * 100.0);
        println!("  Avg Retrieval Latency: {:.2}ms", metrics.activation.avg_retrieval_latency_ms);

        println!("\n--- Selective Forgetting ---");
        println!("  Junk Activation Rate: {:.2}%", metrics.forgetting.junk_activation_rate * 100.0);
        println!("  Retrieval Precision: {:.2}%", metrics.forgetting.retrieval_precision * 100.0);

        println!("\n--- Interference Resistance ---");
        println!("  Top-1 Accuracy: {:.2}%", metrics.interference.top1_accuracy * 100.0);
        println!("  Confusion Rate: {:.2}%", metrics.interference.confusion_rate * 100.0);

        println!("\n--- Contextual Recall ---");
        println!("  Context Switch Success: {:.2}%", metrics.contextual.context_switch_success_rate * 100.0);
        println!("  Cross-Context Interference: {:.2}%", metrics.contextual.cross_context_interference * 100.0);

        println!("\n--- Memory Consolidation ---");
        println!("  Retrieval Speed Improvement: {:.2}%", metrics.consolidation.retrieval_speed_improvement * 100.0);
        println!("  Consolidation Rate: {:.2}%", metrics.consolidation.consolidation_rate * 100.0);

        println!("\n--- Adaptation ---");
        println!("  Outdated Info Superseded: {:.2}%", metrics.adaptation.outdated_info_superseded * 100.0);
        println!("  Historical Context Preserved: {:.2}%", metrics.adaptation.historical_context_preserved * 100.0);

        println!("\n--- Efficiency ---");
        println!("  Avg Memories Examined: {:.2}", metrics.efficiency.memories_examined_avg);
        println!("  P50 Latency: {:.2}ms", metrics.efficiency.retrieval_latency_p50);
        println!("  P95 Latency: {:.2}ms", metrics.efficiency.retrieval_latency_p95);
        println!("  P99 Latency: {:.2}ms", metrics.efficiency.retrieval_latency_p99);
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
        
        wtr.write_record(&["dimension", "metric", "value"])?;
        
        wtr.write_record(&["activation", "top1_accuracy", &metrics.activation.top1_accuracy.to_string()])?;
        wtr.write_record(&["activation", "top3_accuracy", &metrics.activation.top3_accuracy.to_string()])?;
        wtr.write_record(&["activation", "avg_latency_ms", &metrics.activation.avg_retrieval_latency_ms.to_string()])?;
        
        wtr.write_record(&["forgetting", "junk_activation_rate", &metrics.forgetting.junk_activation_rate.to_string()])?;
        wtr.write_record(&["forgetting", "retrieval_precision", &metrics.forgetting.retrieval_precision.to_string()])?;
        
        wtr.write_record(&["interference", "top1_accuracy", &metrics.interference.top1_accuracy.to_string()])?;
        wtr.write_record(&["interference", "confusion_rate", &metrics.interference.confusion_rate.to_string()])?;
        
        wtr.write_record(&["contextual", "context_switch_success_rate", &metrics.contextual.context_switch_success_rate.to_string()])?;
        
        wtr.write_record(&["efficiency", "memories_examined_avg", &metrics.efficiency.memories_examined_avg.to_string()])?;
        wtr.write_record(&["efficiency", "p50_latency_ms", &metrics.efficiency.retrieval_latency_p50.to_string()])?;
        wtr.write_record(&["efficiency", "p95_latency_ms", &metrics.efficiency.retrieval_latency_p95.to_string()])?;
        
        wtr.flush()?;
        println!("CSV metrics saved to: {}", csv_path.display());

        Ok(())
    }

    fn generate_plots(&self, metrics: &BenchmarkMetrics) -> Result<()> {
        use plotters::prelude::*;
        
        let output_path = PathBuf::from(&self.config.output_dir);
        let plot_path = output_path.join("metrics_overview.png");
        
        let root = BitMapBackend::new(&plot_path, (800, 600)).into_drawing_area();
        root.fill(&WHITE)?;
        
        let mut chart = ChartBuilder::on(&root)
            .caption("Cognoscenti Benchmark Metrics", ("sans-serif", 40))
            .margin(20)
            .x_label_area_size(60)
            .y_label_area_size(80)
            .build_cartesian_2d(0.0..7.0, 0.0..1.0)?;

        chart.configure_mesh()
            .x_desc("Evaluation Dimension")
            .y_desc("Score")
            .x_labels(7)
            .y_labels(10)
            .draw()?;

        let dimensions = ["Activation", "Forgetting", "Interference", "Contextual", "Consolidation", "Adaptation", "Efficiency"];
        let values = [
            metrics.activation.top1_accuracy,
            metrics.forgetting.retrieval_precision,
            metrics.interference.top1_accuracy,
            metrics.contextual.context_switch_success_rate,
            metrics.consolidation.consolidation_rate,
            metrics.adaptation.outdated_info_superseded,
            1.0 - (metrics.efficiency.retrieval_latency_p50 / 1000.0).min(1.0),
        ];

        chart.draw_series(
            dimensions.iter().zip(values.iter()).enumerate().map(|(i, (_dim, val))| {
                Rectangle::new([(i as f64 - 0.4, 0.0), (i as f64 + 0.4, *val)], BLUE.filled())
            })
        )?;

        chart.configure_series_labels()
            .background_style(&WHITE.mix(0.8))
            .border_style(&BLACK)
            .draw()?;

        root.present()?;
        println!("Plot saved to: {}", plot_path.display());

        Ok(())
    }
}

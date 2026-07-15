# Developer Documentation

Welcome to the Cognoscenti developer documentation. This guide covers the architecture, API, and contribution guidelines for the cognitive memory benchmarking tool.

## Table of Contents

- [Architecture](#architecture)
- [Getting Started](#getting-started)
- [API Reference](#api-reference)
- [Adding New Workloads](#adding-new-workloads)
- [Adding New Dimensions](#adding-new-dimensions)
- [Testing](#testing)
- [Code Style](#code-style)
- [Performance Considerations](#performance-considerations)

## Architecture

Cognoscenti is built in Rust and follows a modular architecture:

### Core Components

- **`core.rs`**: Core data structures (`MemoryChunk`, `Context`, `Domain`, `RetrievalResult`)
- **`workload.rs`**: Workload generation and simulation logic
- **`dimensions.rs`**: Evaluation dimensions and metrics definitions
- **`metrics.rs`**: Metrics collection and statistical analysis
- **`runner.rs`**: Benchmark execution and reporting
- **`main.rs`**: CLI entry point

### Data Flow

```
Workload Generation → Memory Chunks → Retrieval Simulation → Metrics Collection → Report Generation
```

## Getting Started

### Prerequisites

- Rust 1.70 or higher
- Cargo (included with Rust)

### Development Setup

```bash
# Clone the repository
git clone https://github.com/Achiral-AI/cognoscenti.git
cd cognoscenti

# Install development dependencies
cargo install cargo-watch
cargo install cargo-edit

# Run tests
cargo test

# Run with hot reload
cargo watch -x run
```

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run clippy
cargo clippy -- -D warnings

# Format code
cargo fmt
```

## API Reference

### BenchmarkConfig

Configuration for running benchmarks.

```rust
pub struct BenchmarkConfig {
    pub workload: String,           // Workload type
    pub duration_months: u32,       // Simulation duration
    pub retrieval_count: u32,       // Number of retrievals
    pub output_dir: String,         // Output directory
    pub generate_plots: bool,       // Generate plots
    pub generate_pdf: bool,         // Generate PDF report
}
```

### BenchmarkRunner

Main benchmark execution engine.

```rust
impl BenchmarkRunner {
    pub fn new(config: BenchmarkConfig) -> Result<Self>;
    pub fn run(&mut self) -> Result<BenchmarkMetrics>;
}
```

### Workload Types

```rust
pub enum WorkloadType {
    Strategic,      // High-level decision patterns
    Technical,      // Frequent technical interactions
    Creative,       // Cross-domain creative patterns
    Episodic,       // Short-lived interaction patterns
    Analytical,     // Precision-focused patterns
}
```

### Evaluation Dimensions

```rust
pub struct BenchmarkMetrics {
    pub activation: ActivationMetrics,
    pub forgetting: ForgettingMetrics,
    pub interference: InterferenceMetrics,
    pub contextual: ContextualMetrics,
    pub consolidation: ConsolidationMetrics,
    pub adaptation: AdaptationMetrics,
    pub efficiency: EfficiencyMetrics,
}
```

## Adding New Workloads

To add a new workload type:

1. **Add to WorkloadType enum** in `workload.rs`:

```rust
pub enum WorkloadType {
    Strategic,
    Technical,
    Creative,
    Episodic,
    Analytical,
    YourNewWorkload,  // Add here
}
```

2. **Add workload generation logic** in `workload.rs`:

```rust
impl Workload {
    pub fn generate_your_new_workload_events(&mut self) -> Result<()> {
        // Implement your workload generation logic
        // Generate events based on your workload characteristics
        Ok(())
    }
}
```

3. **Add CLI mapping** in `runner.rs`:

```rust
let workload_type = match config.workload.as_str() {
    "strategic" => WorkloadType::Strategic,
    "your_new_workload" => WorkloadType::YourNewWorkload,
    _ => return Err(anyhow::anyhow!("Unknown workload type")),
};
```

4. **Update README** with workload description

## Adding New Dimensions

To add a new evaluation dimension:

1. **Define metrics structure** in `dimensions.rs`:

```rust
pub struct YourNewDimensionMetrics {
    pub metric1: f64,
    pub metric2: f64,
}
```

2. **Add to BenchmarkMetrics** in `metrics.rs`:

```rust
pub struct BenchmarkMetrics {
    // existing dimensions...
    pub your_new_dimension: YourNewDimensionMetrics,
}
```

3. **Implement collection logic** in `MetricsCollector`:

```rust
impl MetricsCollector {
    pub fn record_your_new_dimension_event(&mut self, data: YourData) {
        // Implement collection logic
    }
}
```

4. **Add reporting** in `runner.rs`:

```rust
fn print_metrics(&self, metrics: &BenchmarkMetrics) {
    // existing metrics...
    println!("--- Your New Dimension ---");
    println!("  Metric 1: {:.2}", metrics.your_new_dimension.metric1);
}
```

## Testing

### Unit Tests

```bash
cargo test
```

### Integration Tests

```bash
cargo test --test integration
```

### Benchmark Tests

```bash
cargo test --release
```

### Test Coverage

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```

## Code Style

We follow standard Rust conventions:

- Use `cargo fmt` for formatting
- Use `cargo clippy` for linting
- Follow Rust API guidelines
- Document public APIs with `///` comments
- Use meaningful variable and function names

### Commit Messages

Follow conventional commits:

```
feat: add new workload type for scientific computing
fix: correct memory leak in metrics collection
docs: update API documentation
refactor: simplify workload generation logic
test: add integration tests for PDF generation
```

## Performance Considerations

### Memory Management

- Use iterators instead of collections when possible
- Prefer references over clones
- Use `Cow<str>` for string handling

### Benchmark Execution

- Use `--release` builds for accurate performance
- Consider parallelizing retrieval simulations
- Profile with `cargo flamegraph` for hot spots

### Large Workloads

For large-scale benchmarks:

```bash
# Use release mode
cargo run --release -- --duration-months 12 --retrieval-count 10000

# Limit output generation
cargo run --release -- --generate-plots false --generate-pdf false
```

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Development Workflow

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Run `cargo fmt` and `cargo clippy`
6. Submit a pull request

### Code Review Process

- All PRs require review
- CI must pass
- Tests must be added for new features
- Documentation must be updated

## Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Cargo Guide](https://doc.rust-lang.org/cargo/)
- [ACT-R Architecture](https://act-r.psy.cmu.edu/)
- [Emergent Memory Systems](https://achiral.ai/blog/emergent-memory-systems)

## Support

- **Issues**: [GitHub Issues](https://github.com/Achiral-AI/cognoscenti/issues)
- **Discussions**: [GitHub Discussions](https://github.com/Achiral-AI/cognoscenti/discussions)
- **Email**: [developers@achiral.ai](mailto:developers@achiral.ai)

---

**Let's make memories together.** [https://achiral.ai](https://achiral.ai)

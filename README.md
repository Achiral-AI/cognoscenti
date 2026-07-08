# Cognoscenti

<p align="center">
  <img src="./assets/cognoscenti-emblem.svg" alt="Cognoscenti emblem" width="180">
</p>

**Cognoscenti** is a benchmarking tool for **cognitive memory systems** or [Emergent Memory Systems](https://achiral.ai/blog/emergent-memory-systems) like [Achiral](https://achiral.ai) that are ACT-R inspired rather than tools of persistence like vector or proximity-based databases, knowledge graphs, or semantic search systems with no concept of forgetting.

## Implementation

Cognoscenti is implemented in Rust and provides a comprehensive framework for benchmarking cognitive memory architectures. The implementation includes:

- **Workload Simulation**: Generate synthetic team interactions (founders, engineers, designers, customers, investors) over configurable time periods
- **Core Evaluation Dimensions**: Measure activation precision, selective forgetting, interference resistance, contextual recall, memory consolidation, adaptation, and efficiency
- **Metrics Collection**: Automated collection and analysis of benchmark metrics with statistical computations
- **Visualization**: Generate plots and export results to JSON/CSV for further analysis

## Installation

```bash
# Clone the repository
git clone <repository-url>
cd cognoscenti

# Build the project
cargo build --release
```

## Usage

Run the benchmark with default settings:

```bash
cargo run --release
```

Or customize the benchmark parameters:

```bash
cargo run --release -- \
  --workload engineers \
  --duration-months 6 \
  --retrieval-count 1000 \
  --output-dir ./results \
  --generate-plots true
```

### Available Workloads

- `founders`: Strategic planning and high-level decision making
- `engineers`: Product development, bug reports, and technical discussions
- `designers`: Product design, user research, and UI/UX work
- `customers`: Customer support and feedback
- `investors`: Fundraising and financial discussions

### Output

The benchmark generates:

- `benchmark_results.json`: Complete metrics in JSON format
- `benchmark_metrics.csv`: Metrics in CSV format for easy analysis
- `metrics_overview.png`: Visual overview of benchmark results

## Contributing

We welcome contributions from the community! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on how to contribute to Cognoscenti.

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

### Development

To set up the development environment:

```bash
# Clone the repository
git clone https://github.com/achiral/cognoscenti.git
cd cognoscenti

# Build and test
cargo build
cargo test
cargo clippy
cargo fmt
```

### Reporting Issues

If you find a bug or have a feature request, please open an issue on GitHub. For security vulnerabilities, please see [SECURITY.md](SECURITY.md) for our disclosure policy.


## Philosophy

Existing benchmarks primarily evaluate retention, long-context retrieval, or task completion. For example, LoCoMo, a traditional memory benchmarks, will primarily answer:

> Can the system retrieve a fact from this specific storage?

Such a test would likely yield a high yes and a few nos—with persistence exhibiting near total recall. There is no benchmarking available for directly evaluating **selective forgetting**, **activation**, **contextual recall**, **memory strengthening**, or **interference resistance**.

Cognoscenti instead asks:

> Should this memory come to mind right now?

The benchmark is intended for ACT-R-inspired and other human-like memory
architectures that emphasize selective retrieval, strengthening through
reuse, contextual recall, and useful forgetting. In several ways, Cognoscenti is a benchmark for cognitive memory systems inspired by human memory architectures such as ACT-R.

## Why create a new benchmark?

A new benchmark specifically for cognitive memory covers:

- Activation precision (did the most relevant memories surface?)
- Retrieval efficiency (how many memories had to be examined?)
- Forgetting quality (were obsolete or irrelevant memories ignored?)
- Interference resistance (can similar memories be distinguished?)
- Memory strengthening (does repeated information become easier to retrieve?)
- Temporal adaptation (does new information appropriately replace old assumptions?)
- Contextual recall (does retrieval change appropriately with project or conversational context?)

This would highlight what makes an ACT-R-inspired memory layer fundamentally different from a persistent vector store or RAG system.

In other words, one wouldn't simply abandon LoCoMo–use it for regression test long-term recall, but if your goal is to demonstrate that an ACT-R system behaves more like human cognition than a database (persistence), then you'll likely need a new benchmark centered on selective remembering, useful forgetting, and efficient activation, because no current benchmark directly evaluates those properties.


## Persistence with total recall vs. Forgetful (amnesic) emergent memory

| Benchmark | Best for | Good fit for ACT-R? |
|-----------|----------|---------------------|
| LongMemEval | Long-context memory retrieval | Moderate |
| Needle-in-a-Haystack | Retrieval under huge contexts | Low |
| InfiniteBench | Scaling to very long contexts | Low |
| BABILong | Reasoning over long contexts | Moderate |
| τ-bench | Stateful agent tasks over time | High |
| SWE-bench | Long-running engineering tasks | High (if your users are developers) |

### How Cognoscenti is Different

Unlike traditional memory benchmarks that focus on **persistence and total recall**, Cognoscenti evaluates **cognitive properties** that mirror human memory systems:

- **Selective Forgetting**: Measures how well the system identifies and fades irrelevant information
- **Activation Dynamics**: Evaluates whether the most relevant memories surface at the right time
- **Contextual Adaptation**: Tests retrieval accuracy across different conversational and project contexts
- **Memory Consolidation**: Measures how repeated access strengthens memory retrieval
- **Interference Resistance**: Evaluates ability to distinguish between similar memories

While existing benchmarks answer "Can you retrieve this fact?", Cognoscenti asks "Should this memory come to mind right now?" — making it uniquely suited for evaluating ACT-R-inspired and other emergent memory systems.


## Core Evaluation Dimensions

### Activation

Does the most relevant memory become active when needed? - Top-1
retrieval accuracy - Irrelevant memory activation - Retrieval latency

### Selective Forgetting

Does unimportant information fade? - Junk activation rate - Retrieval
precision - Forgotten-memory ratio

### Interference Resistance

Can similar memories be distinguished? - Top-1 / Top-3 accuracy -
Distractor count - Retrieval confidence

### Contextual Recall

Does context influence retrieval? Examples include switching between
engineering, marketing, finance, or design discussions.

### Memory Consolidation

Do repeated experiences become easier to retrieve over time?

### Adaptation

Does newer information correctly supersede outdated beliefs while
preserving historical context?

### Efficiency

Does retrieval remain selective and fast as total memory grows? Possible
metrics: - Memories examined - Activated chunks - Retrieval latency -
Token cost

## Suggested Workloads

Rather than isolated question-answer pairs, simulate long-running
teams: - Founders - Engineers - Designers - Customers - Investors

Generate months of meetings, chats, documents, roadmaps, bugs, and
changing project priorities.

## Positioning

LoCoMo remains useful as a regression test for long-term recall.

Cognoscenti complements it by measuring whether an AI behaves like an
experienced teammate rather than an archive.

## Proposed Tagline

> Persistent memory asks: "Can you retrieve this fact?"

> Cognoscenti asks: "Should this memory come to mind right now?"

## Vision

The long-term goal is to establish Cognoscenti as a standard benchmark
for evaluating cognitive memory systems inspired by human memory
architectures such as ACT-R.

## License

Cognoscenti is licensed under the Apache License, Version 2.0. See
`LICENSE` and `NOTICE`.

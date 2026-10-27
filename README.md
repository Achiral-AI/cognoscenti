# Cognoscenti: A Benchmark for Trustworthy AI Memory

<p align="center">
  <img src="./assets/cognoscenti-emblem.svg" alt="Cognoscenti C mark" width="160">
</p>

<p align="center">
  <a href="https://github.com/Achiral-AI/cognoscenti/actions/workflows/ci.yml?query=branch%3Amain">
    <img src="https://github.com/Achiral-AI/cognoscenti/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI Status">
  </a>
  <a href="https://github.com/Achiral-AI/cognoscenti/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="License">
  </a>
  <a href="https://doi.org/10.5281/zenodo.21995304">
    <img src="https://zenodo.org/badge/DOI/10.5281/zenodo.21995304.svg" alt="DOI">
  </a>
  <a href="https://discord.gg/9vrw6RxKP">
    <img src="https://img.shields.io/badge/Discord-Join%20Community-5865F2?logo=discord&logoColor=white" alt="Join Discord">
  </a>
</p>

**Cognoscenti** is a benchmark for **trustworthy AI memory**: memory systems that choose what to retain, retrieve, forget, and suppress with measurable reliability. It evaluates **cognitive memory systems** and [Emergent Memory Systems](https://achiral.ai/blog/emergent-memory-systems) like [Achiral](https://achiral.ai) that are ACT-R inspired rather than tools of persistence like vector or proximity-based databases, knowledge graphs, or semantic search systems with no concept of forgetting.

> The problem isn't how much is remembered, but what gets chosen as memory.

---

## 💎 Sponsor Cognoscenti

Support the development of cognitive memory benchmarks and help shape the future of AI memory systems. [Become a sponsor](SPONSORSHIP.md) and gain visibility among thousands of AI/ML developers building the next generation of memory architectures.

[📢 View Sponsorship Tiers](SPONSORSHIP.md) | [🤝 GitHub Sponsors](https://github.com/Achiral-AI) | [💬 Contact Us](mailto:sponsorships@achiral.ai)

---

## Implementation

Cognoscenti is implemented in Rust. Today it evaluates memory systems against
a JSONL set of benchmark items: each item carries a query, a gold memory, and
distractors, and a configured system is scored on whether it retrieves the
gold memory and ignores the distractors. This README describes only what is
implemented today; a rebuild toward a simulated-time event stream, an ACT-R
reference implementation, and stability metrics across repeated runs is in
progress.

## Installation

```bash
# Clone the repository
git clone https://github.com/Achiral-AI/cognoscenti.git
cd cognoscenti

# Build the project
cargo build --release
```

## Usage

Cognoscenti has one mode: evaluate one or more memory systems against a JSONL
file of benchmark items. `--items-file` is required.

### Memory Tool Eval Reports

Run the included JSONL benchmark items against the built-in local baseline:

```bash
cargo run --release -- \
  --items-file examples/memory_tools.jsonl \
  --systems local \
  --top-k 3 \
  --output-dir ./results/memory-tools
```

Compare multiple configured memory tools with the same items:

```bash
cargo run --release -- \
  --items-file examples/memory_tools.jsonl \
  --systems local,mem0,supermemory,zep,letta \
  --top-k 3 \
  --output-dir ./results/memory-tools
```

The eval runner always supports `local`. External tools use provider-specific environment variables:

```bash
export COGNOSCENTI_MEM0_INGEST_URL="https://..."
export COGNOSCENTI_MEM0_RETRIEVE_URL="https://..."
export COGNOSCENTI_MEM0_API_KEY="..."
```

Use the same pattern for `SUPERMEMORY`, `ZEP`, and `LETTA`. Provider endpoints receive JSON over `POST`; ingest requests include `system` and `memories`, and retrieval requests include `system`, `query`, `context`, and `top_k`.

The memory-tool eval generates:

- `memory_eval_results.json`: Per-system item results and aggregate metrics
- `memory_eval_metrics.csv`: Comparison-ready metric table
- `memory_eval_report.pdf`: Printable comparison report

## Organizational Memory Run (2026-08-02) — superseded

The first public organizational-memory run reported 91.67% Top-1 accuracy for
an `organic-memory` baseline. That number does not measure Achiral or any
real memory system: `organic-memory` was a local keyword scorer that read
words like "validated" and "superseded" written directly into the benchmark
item text. The `organic-memory`/`achiral` alias and its keyword heuristic
have been removed from Cognoscenti. See
[`results/organizational-memory-2026-08-02/README.md`](results/organizational-memory-2026-08-02/README.md)
for the full explanation and the original artifacts, kept for transparency.

This repository does not currently publish a result comparing Achiral, or any
vendor, on Cognoscenti.

## Citing Cognoscenti

If you use Cognoscenti or the public benchmark artifacts, please cite the repository and the benchmark report. GitHub will surface citation metadata from [`CITATION.cff`](CITATION.cff).

Version DOI: [10.5281/zenodo.21995304](https://doi.org/10.5281/zenodo.21995304)

Concept DOI: [10.5281/zenodo.21995303](https://doi.org/10.5281/zenodo.21995303)

Figshare preprint and reproducibility bundle DOI: [10.6084/m9.figshare.33288195](https://doi.org/10.6084/m9.figshare.33288195)

Software Heritage source archive: [`swh:1:snp:c49295731ebb8a76713e877cfd8dc6399c0137d7`](https://archive.softwareheritage.org/swh:1:snp:c49295731ebb8a76713e877cfd8dc6399c0137d7)

```bibtex
@software{achiral_ai_2026_cognoscenti,
  author = {{Achiral AI}},
  title = {Cognoscenti: A Benchmark for Trustworthy AI Memory},
  year = {2026},
  version = {0.2.1},
  doi = {10.5281/zenodo.21995304},
  url = {https://github.com/Achiral-AI/cognoscenti}
}

@techreport{achiral_ai_2026_ai_memory_benchmark,
  author = {{Achiral AI}},
  title = {AI Memory Benchmark: What Makes Memory Trustworthy?},
  year = {2026},
  doi = {10.5281/zenodo.21995304},
  url = {https://achiral.ai/benchmarks/ai-memory-benchmark},
  note = {Public Cognoscenti benchmark report for the 2026-08-02 organizational-memory run}
}

@misc{achiral_ai_2026_cognoscenti_figshare,
  author = {{Achiral AI}},
  title = {Cognoscenti: A Benchmark for Trustworthy AI Memory},
  year = {2026},
  doi = {10.6084/m9.figshare.33288195},
  url = {https://doi.org/10.6084/m9.figshare.33288195},
  note = {Figshare preprint and reproducibility artifact bundle}
}
```

The Figshare preprint and benchmark artifact are shared under CC BY 4.0. Cognoscenti source code remains licensed under Apache-2.0.

Zenodo archival metadata is maintained in [`.zenodo.json`](.zenodo.json).

## Contributing

We welcome contributions from the community! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on how to contribute to Cognoscenti.

For detailed developer documentation, API reference, and architecture guides, see [DEVELOPERS.md](DEVELOPERS.md).

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


These are the properties Cognoscenti aims to measure — activation precision,
selective forgetting, interference resistance, contextual recall, memory
consolidation, adaptation, and efficiency — but today's JSONL item eval only
covers activation-style retrieval (Top-1, recall@k, precision@k, distractor
activation) against a single frozen set of items, asked once. It does not yet
measure forgetting, consolidation, or adaptation as behavior over time.

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

## Citation

If you use Cognoscenti in your research, please cite:

```bibtex
@software{cognoscenti2024,
  title = {Cognoscenti: A Benchmark for Cognitive Memory Systems},
  author = {Achiral},
  year = {2024},
  url = {https://github.com/Achiral-AI/cognoscenti}
}
```

## Acknowledgments

Cognoscenti is inspired by cognitive memory architectures such as ACT-R and aims to complement existing memory benchmarks like LoCoMo by evaluating properties unique to human-like memory systems.

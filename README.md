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

Cognoscenti is implemented in Rust, as two separate modes:

- **JSONL item eval** (original mode): each item carries a query, a gold
  memory, and distractors; a configured system is scored on whether it
  retrieves the gold memory and ignores the distractors. See "Memory Tool
  Eval Reports" below.
- **Stream eval** (current build-out, Plan 25): a chronologically ordered
  JSONL event/question stream (`src/stream.rs`), a dataset lint
  (`src/lint.rs`), an ACT-R reference (`src/actr.rs`: base-level activation,
  fan effect, logistic retrieval probability and noise), calibration
  (`src/calibration.rs`: reliability curves, Brier score, ECE, native or
  derived from a raw score), scoring (`src/scoring.rs`: recall@k, scope-based
  leak rate), five stability tests (`src/stability.rs`: run-to-run,
  order sensitivity, paraphrase robustness, stability against plasticity,
  lock-in), and baseline adapters (`src/baselines.rs`: verbatim, BM25,
  recency, full-context, bounded-window) replayed through a deterministic,
  seeded harness (`src/determinism.rs`, `src/manifest.rs`) via
  `--replay-stream`. See "Stream Eval (ACT-R reference, baselines, token
  accounting)" below.

No vendor or Achiral comparison has been run on the stream eval yet; see
"Organizational Memory Run (2026-08-02) — superseded" below for why the one
public number this repository ever published doesn't count.

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

### Stream Eval (ACT-R reference, baselines, token accounting)

Lint a stream dataset (six checks, including that no item's content leaks
its own gold answer) and fail nonzero if any issue is found:

```bash
cargo run --release -- --lint-stream path/to/stream.jsonl
```

Replay a stream through a baseline and report recall@k and tokens injected
per question. `--systems` is one of `verbatim`, `bm25`, `recency`,
`full-context`, or `bounded-window`:

```bash
cargo run --release -- \
  --replay-stream path/to/stream.jsonl \
  --systems bm25 \
  --top-k 3 \
  --seed 0
```

`--recency-half-life-days` (default 30) configures `recency`;
`--bounded-window-size` (default 20) configures `bounded-window`.

Measure how tokens injected per question changes as ingested history grows,
at open-ended geometric checkpoints (10, 100, 1000, ...) instead of one run:

```bash
cargo run --release -- \
  --replay-stream path/to/stream.jsonl \
  --systems full-context,bounded-window,bm25 \
  --cycles 1000
```

This is read-path token accounting only (`src/tokens.rs`): what a query
would inject into a model's context, approximated by word count, not a
real tokenizer. No live run has been published from this yet — see
"Tokens vs. cycles" under Measurement framework below.

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
  version = {0.2.0},
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


## Measurement framework

Trustworthy memory, for Cognoscenti, is tested on three axes: does its stated
confidence mean anything (calibration), does it agree with itself when
repeated (stability), and does it respect the boundaries it's told (scope).
A fourth axis asks what it costs: does a system that learns from use need
fewer tokens per query over time than one that doesn't.

**Every diagram below is a schematic of what the code computes, not a
measured result.** Nothing in this repository has been run against a live
system yet — see the "Organizational Memory Run (2026-08-02) — superseded"
section below. A number appears on this page only once a real run backs it.

### 1. Trust vs. confidence (calibration)

<img src="assets/plots/trust-vs-confidence.svg" alt="Schematic reliability diagram: predicted probability vs. observed frequency needed, with a perfect-calibration diagonal and two illustrative curves" width="560">

Of memories retrieved at predicted probability *p*, how often were they
actually needed? Computed by [`src/calibration.rs`](src/calibration.rs)
(`reliability_curve`, `brier_score`, `expected_calibration_error`) — and
*derived* for any system that exposes only a raw score or rank, via
`derive_probabilities_from_scores`, so a vendor can't opt out of calibration
by simply not stating a probability.

### 2. Trust vs. repetition (run-to-run stability)

<img src="assets/plots/trust-vs-repetition.svg" alt="Schematic comparison of a stable system (rank agreement clustered near 1) and an unstable system (rank agreement scattered near 0) across replicate pairs" width="560">

Same stream, same order, repeated: does the ranking agree with itself?
Computed by [`src/stability.rs`](src/stability.rs) (`flip_rate`,
`rank_agreement`, Kendall's tau over the items both rankings returned). All
five stability tests Cognoscenti measures — run-to-run, order sensitivity,
paraphrase robustness, stability against plasticity, lock-in — are built,
tested, and documented in that module; none has been run against a live
replicate set yet.

### 3. Trust vs. scope (leak safety)

<img src="assets/plots/trust-vs-scope.svg" alt="Diagram of org, user, and agent scopes with two unauthorized access attempts blocked, including a user and agent sharing the same id" width="560">

Does a memory ever surface for an actor not authorized to see it? Computed
by [`src/scoring.rs`](src/scoring.rs) (`is_authorized`, `leak_rate`,
`zero_leak_upper_bound_95` — the rule-of-three bound for a zero-leak claim).
Org-scoped memories are shared; user- and agent-scoped memories are private
to that exact actor, including the case where a user and an agent happen to
share an id.

### 4. Tokens vs. cycles (implemented, not yet measured)

<img src="assets/plots/token-usage-vs-cycles.svg" alt="Design hypothesis chart: tokens injected per cycle vs. an open-ended, log-scaled cycle count, comparing full context, bounded window, static RAG, and a learning ACT-R system" width="560">

The question this is meant to answer: across many query-inference cycles,
does a system that learns from use end up consuming fewer total tokens than
one that doesn't — not just on one call, but durably, as the horizon of
cycles keeps extending? Four architectures, same question:

- **Full context** (`FullContextSystem`) — every query re-sends the whole
  history. Tokens grow without bound as history grows.
- **Bounded window** (`BoundedWindowSystem`) — a fixed context size; old
  material falls off. Tokens stay flat, but nothing says accuracy does too.
- **Static RAG** (`verbatim`/`bm25`/`recency`) — tokens stay bounded by
  top-k regardless of history size, with no learning signal.
- **Learning (ACT-R)** — the hypothesis under test: a calibrated system
  knows when it's confident, so it can stop at a small top-k instead of
  padding it, and the curve should stay flat or decline as that confidence
  sharpens with use.

"Durable" means the curve has to hold under replicates, not erode as noise
accumulates in the store, and recover after a fact change — the same
stability battery above, pointed at a token signal instead of a correctness
signal. The token counter (`src/tokens.rs`: `approx_token_count`,
`geometric_checkpoints`), both new baselines, and the checkpointed
`--cycles` runner (`token_usage_by_cycle` in `src/adapter.rs`) are built and
tested; no live run has been executed against a real dataset yet, so the
diagram above remains a hypothesis, not a result.

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

## Acknowledgments

Cognoscenti is inspired by cognitive memory architectures such as ACT-R and aims to complement existing memory benchmarks like LoCoMo by evaluating properties unique to human-like memory systems.

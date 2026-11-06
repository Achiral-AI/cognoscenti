# Implementation

Cognoscenti is implemented in Rust, as two separate modes:

- **JSONL item eval** (original mode): each item carries a query, a gold
  memory, and distractors; a configured system is scored on whether it
  retrieves the gold memory and ignores the distractors. See
  [`docs/usage.md`](usage.md#jsonl-item-eval) for how to run it.
- **Stream eval** (current build-out, Plan 25): a chronologically ordered
  JSONL event/question stream (`src/stream.rs`), a dataset lint
  (`src/lint.rs`), an ACT-R reference (`src/actr.rs`: base-level activation,
  fan effect, logistic retrieval probability and noise), calibration
  (`src/calibration.rs`: reliability curves, Brier score, ECE, native or
  derived from a raw score), scoring (`src/scoring.rs`: recall@k, scope-based
  leak rate), five stability tests (`src/stability.rs`: run-to-run,
  order sensitivity, paraphrase robustness, stability against plasticity,
  lock-in), token accounting (`src/tokens.rs`: word-based approximation,
  geometric checkpoints), and baseline adapters (`src/baselines.rs`:
  verbatim, BM25, recency, full-context, bounded-window) replayed through a
  deterministic, seeded harness (`src/determinism.rs`, `src/manifest.rs`).
  `--replay-stream` reports each baseline's derived Brier score and
  expected calibration error (`MemorySystem::retrieve_scored`, a
  confidence score alongside each ranked id - real for bm25/recency/
  verbatim, a rank-derived fallback for full-context/bounded-window,
  which have no confidence signal of their own), and, with
  `--replicates`, run-to-run flip rate. See
  [`docs/usage.md`](usage.md#stream-eval) for how to run each part.

## What's pending

Everything above is built and covered by the test suite (`cargo test`).
What it has not yet done:

- **No live run against a vendor or Achiral.** The stream eval's baselines
  (verbatim, BM25, recency, full-context, bounded-window) are local
  references only — there is no HTTP-backed vendor adapter for the stream
  eval yet (the JSONL item eval's `mem0`/`supermemory`/`zep`/`letta`
  env-var wiring does not carry over to `--replay-stream`).
- **Only one stream dataset ships so far, and it's small.**
  [`datasets/organizational-mesh/atlas-01/`](../datasets/organizational-mesh/atlas-01/)
  is one synthetic org's 30-day stream with 15d/30d horizon cutoffs
  (`horizons.json`) and an authority hard-negative (a VP's informal
  suggestion vs. a later, explicit, documented decision) - enough to
  exercise the runner end to end, not enough questions for a believable
  calibration or stability number. A wider fabric, and more horizons
  (hours through a year), is still ahead.
- **Tier 2 is not started**: the full adapter contract (actor, scope,
  `answer`/`use`/`settle`, HTTP-backed vendor and Achiral systems) described
  in Plan 25 is a later phase. The current `MemorySystem` trait is a
  reduced version — `ingest`/`retrieve` only, no actor or scope parameter.
- **The token-savings axis has never been run against a real dataset.**
  `--cycles` is built and tested (`token_usage_by_cycle`), but no published
  number exists yet.
- **The four Measurement framework diagrams in the README are schematics**
  of what the code computes, not measured results. A number appears there
  only once a real run backs it.

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
- **Five small stream datasets ship so far, across three categories, and
  none is benchmark-scale yet.** `datasets/` has one directory per
  category, each holding named org/actor instances:
  [`individual-memory/`](../datasets/individual-memory/) (`sam-01` a
  person, `scout-01` an agent — same mechanism, no second actor, so no
  scope/leak axis to test), [`organizational-mesh/`](../datasets/organizational-mesh/)
  (`atlas-01`, `beacon-01` — org-scoped people-team streams, each with its
  own authority hard-negative), and [`mixed-teams/`](../datasets/mixed-teams/)
  (`foundry-01` — people and an agent in the same org scope, the agent
  both speaking into the shared scope and keeping its own private
  scratch memory). `beacon-01` and `foundry-01` also carry a leak-rate
  trap: a user- or agent-scoped private note that overlaps a later
  question's terms, so a scope-blind baseline can retrieve it, with a
  vacuous (empty-gold) question checking that nothing org-visible exists
  instead. See "Dataset scale-up plan" below for what grows next, and in
  what order.
- **Tier 2 is not started**: the full adapter contract (actor, scope,
  `answer`/`use`/`settle`, HTTP-backed vendor and Achiral systems) described
  in Plan 25 is a later phase. The current `MemorySystem` trait is a
  reduced version — `ingest`/`retrieve` only, no actor or scope parameter.
  Concretely: the leak-rate traps above sit in the data, but nothing in
  `src/adapter.rs` yet builds a `scoring::LeakTrial` from a question's
  `asked_by` and its retrieved ids' source scopes, so `leak_rate` has
  never been run against real retrieval output — only unit-tested in
  isolation (`src/scoring.rs`). That hookup needs no trait change (the
  runner already holds every event's scope via `stream.events()`), so
  it's a small addition whenever it's wanted, not a Tier 2 dependency.
  A true org-vs-suborg scope (a named sub-team, visible to its members
  and no one else) is a separate, larger gap: `ScopeKind` has no `Team`
  variant and `Actor` carries no team membership, so today's
  "suborg" (`beacon-01`'s `platform-lead` note) is modeled as one
  person's private scope, not a shared sub-team scope. A real `Team`
  kind is deferred to a later major version.
- **The token-savings axis has never been run against a real dataset.**
  `--cycles` is built and tested (`token_usage_by_cycle`), but no published
  number exists yet.
- **The four Measurement framework diagrams in the README are schematics**
  of what the code computes, not measured results. A number appears there
  only once a real run backs it.

## Dataset scale-up plan

`atlas-01` is a seed, not a benchmark. The plan it's seeding:

- **One growing stream per organization, not one dataset per horizon.**
  A horizon (`atlas-01`'s `15d`/`30d`) is a `now` cutoff into the same
  accumulating event log, recorded in that org's `horizons.json` - never
  a separate, siloed file. Extending an org from 30 days to 90 adds more
  events to its one `stream.jsonl` and a new cutoff, never a second
  stream that silently diverges from the first.
- **Three categories, one directory each, growing independently.**
  `individual-memory/` (one actor, no scope axis), `organizational-mesh/`
  (an org's people-team stream, the authority hard-negative's home),
  and `mixed-teams/` (people and agents sharing an org scope, plus
  agent-private scope) are separate axes, not horizon rungs of each
  other — a new instance in one category (another org, another mixed
  team) doesn't imply one in the others. Each instance is still a single
  growing stream per the point above.
- **Horizons spaced geometrically, not evenly.** ACT-R's base-level
  activation (`B_i = ln(Σ t_j^{-d})`) responds to *ratios* of elapsed
  time, not absolute gaps: doubling elapsed time moves it by the same
  amount whether that's 12h→24h or 180d→365d. Next rungs, in order:
  24h, 72h, 1wk, 30d (already covered by `atlas-01`), 90d, 365d - fewer,
  evenly log-spaced rungs rather than every rung discussed, so each one
  can carry enough questions to mean something (see next point) instead
  of spreading a fixed authoring budget too thin.
- **Breadth per horizon, not just more horizons.** `atlas-01` has three
  questions; LongMemEval ships 500, LoCoMo ~1,540-1,986 across its ten
  conversations, and reviewers of that field still flag any slice under
  ~100 questions as too small to trust. Each horizon needs its own
  adequately sized set of synthetic orgs/projects/interference chains
  before its calibration or stability numbers are worth reading - not
  one org's timeline replayed longer.
- **Gold labels stay architecture-neutral.** `gold_event_ids` and
  `gold_answer` are the only ground truth a stream item carries - never a
  baked-in activation number or decay curve. Calibration is computed from
  whatever confidence score an adapter actually returns
  (`retrieve_scored`, native or derived), so the same fabric scores
  ACT-R, a recency-weighted vector store, or anything else without
  favoring the one whose formula the dataset happened to encode.
- **The authority hard-negative is a pattern, not a one-off.** `atlas-01`
  has one instance (a VP's informal suggestion vs. the eng lead's later,
  explicit, documented decision); every new org added should carry its
  own version of it, so the fabric keeps testing whether a system lets
  perceived seniority override an explicit, later correction, rather than
  that being provable from a single example.

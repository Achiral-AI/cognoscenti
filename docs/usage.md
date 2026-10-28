# Usage

Cognoscenti has two eval modes. They don't share a dataset format or a
CLI flag — pick the chapter for the one you're running.

- [JSONL item eval](#jsonl-item-eval) — compare memory tools on a fixed
  set of query/gold-memory/distractor items.
- [Stream eval](#stream-eval) — replay a chronological event/question
  stream through an ACT-R-style reference and local baselines: lint,
  replay, and token accounting are three separate sub-commands.

Install first:

```bash
git clone https://github.com/Achiral-AI/cognoscenti.git
cd cognoscenti
cargo build --release
```

## JSONL item eval

What it's for: comparing configured memory tools against each other and
against a local baseline, on a fixed set of items. Each item is a query, a
gold memory, and distractors; a system passes an item if it retrieves the
gold memory and ignores the distractors. `--items-file` is required for
this mode.

Run the included items against the built-in local baseline:

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

`local` always works, with no setup. External tools need provider-specific
environment variables:

```bash
export COGNOSCENTI_MEM0_INGEST_URL="https://..."
export COGNOSCENTI_MEM0_RETRIEVE_URL="https://..."
export COGNOSCENTI_MEM0_API_KEY="..."
```

Use the same pattern for `SUPERMEMORY`, `ZEP`, and `LETTA`. Provider
endpoints receive JSON over `POST`; ingest requests carry `system` and
`memories`, retrieval requests carry `system`, `query`, `context`, and
`top_k`.

This generates, in `--output-dir`:

- `memory_eval_results.json` — per-system item results and aggregate metrics
- `memory_eval_metrics.csv` — comparison-ready metric table
- `memory_eval_report.pdf` — printable comparison report

## Stream eval

What it's for: a chronologically ordered sequence of events (things that
happened) and questions (asked at a point in that timeline, with gold
answers tied to specific event IDs, not to memory text). This is how
Cognoscenti measures calibration, stability, scope, and token cost — see
the README's Measurement framework section.

No example stream dataset ships in the repo yet. The three sub-chapters
below all use the same hand-written three-line file so you can run them
end to end. Save this as `test-stream.jsonl`:

```jsonl
{"kind":"event","id":"e1","content":"Decided to use Postgres for the ledger","timestamp":"2026-01-01T00:00:00Z","actor":{"id":"alice","kind":"user"},"scope":{"kind":"org","id":"acme"},"source":"slack"}
{"kind":"event","id":"e2","content":"Switched the ledger to Cockroach","timestamp":"2026-02-01T00:00:00Z","actor":{"id":"alice","kind":"user"},"scope":{"kind":"org","id":"acme"},"source":"slack","supersedes":"e1"}
{"kind":"question","id":"q1","query":"What database does the ledger use?","asked_at":"2026-03-01T00:00:00Z","asked_by":{"id":"bob","kind":"user"},"gold_event_ids":["e2"],"gold_answer":"Cockroach"}
```

`e2` supersedes `e1` (a fact change — the ledger moved from Postgres to
Cockroach), and `q1`'s gold points at `e2`: the question is asked after
the change, so the current fact is the right answer.

Every event needs `kind`, `id`, `content`, `timestamp`, `actor` (`id` +
`kind`: `user` or `agent`), `scope` (`kind`: `org`/`user`/`agent`, + `id`),
and `source`; `supersedes` is optional. Every question needs `kind`, `id`,
`query`, `asked_at`, `asked_by`, and `gold_event_ids`; `gold_answer` and
`validity` are optional. File order is the canonical chronological order —
it is never auto-sorted.

### Lint

What it's for: six checks that catch a broken dataset before it reaches a
scored run — duplicate event or question IDs, `gold_event_ids` pointing at
an event that doesn't exist, gold timestamped after the question that
cites it (the question couldn't have known it yet), `supersedes` pointing
at a missing event or one that isn't actually earlier, the file itself out
of chronological order, and event content containing an answer-revealing
marker word (`validated`, `superseded`, `deprecated`, and others — the
exact list that let the old `organic-memory` scorer read answers straight
out of the dataset text).

```bash
cargo run --release -- --lint-stream test-stream.jsonl
```

A clean file prints `Dataset lint: clean (test-stream.jsonl)` and exits 0
— wire this into CI as a gate on any dataset change. A broken file (try
duplicating `"id":"e1"` onto a second event) prints each issue and its
item ID, and exits nonzero.

### Replay

What it's for: running a local baseline (no learning, no probability
model) against the stream, so a trust claim for a real system means
something — it has to beat what a dumb reference already gets. `--systems`
is one of `verbatim` (raw term overlap), `bm25` (Okapi BM25, k1=1.5,
b=0.75), `recency` (pure exponential decay, ignores the query),
`full-context` (every query re-sends the whole history), or
`bounded-window` (a fixed-size window of the most recent events).

```bash
cargo run --release -- \
  --replay-stream test-stream.jsonl \
  --systems bm25 \
  --top-k 3 \
  --seed 0
```

Output is per-question recall@k and tokens injected (query plus retrieved
content, word-count approximated), then the mean across questions:

```
=== bm25 ===
  q1: recall@3 = 1.00, tokens injected = 18
  mean recall@3 over 1 question(s): 1.0000; mean tokens injected: 18.0
```

(Real output from the three-line stream above, not illustrative.)

`--recency-half-life-days` (default 30) configures `recency`;
`--bounded-window-size` (default 20) configures `bounded-window`. Pass a
comma-separated list to `--systems` to run several baselines against the
same stream in one command.

### Token accounting

What it's for: the "durable horizon" signal — whether a system's token
cost per query holds, grows, or shrinks as the history it has ingested
keeps growing, checked at geometric checkpoints (10, 100, 1000, ...)
instead of on one run. Add `--cycles N` to a replay command; each cycle
re-ingests the stream's events (namespaced per cycle, so the baseline's
history actually grows) and re-asks the same questions at every
checkpoint.

```bash
cargo run --release -- \
  --replay-stream test-stream.jsonl \
  --systems full-context,bounded-window,bm25 \
  --cycles 1000
```

Output, per system, is mean tokens injected at each checkpoint — this is
real output from the three-line stream above, not illustrative:

```
=== full-context: tokens injected per question, by cycle ===
  cycle 10: mean tokens injected = 126.0
  cycle 100: mean tokens injected = 1206.0
  cycle 1000: mean tokens injected = 12006.0
=== bounded-window: tokens injected per question, by cycle ===
  cycle 10: mean tokens injected = 126.0
  cycle 100: mean tokens injected = 126.0
  cycle 1000: mean tokens injected = 126.0
=== bm25: tokens injected per question, by cycle ===
  cycle 10: mean tokens injected = 27.0
  cycle 100: mean tokens injected = 27.0
  cycle 1000: mean tokens injected = 27.0
```

`full-context`'s cost grows with history by construction, exactly as
designed; `bounded-window` and `bm25` stay flat. This is a three-line toy
stream, not a measured claim about any real memory system — see
[`docs/implementation.md`](implementation.md#whats-pending) for what
hasn't been run yet.

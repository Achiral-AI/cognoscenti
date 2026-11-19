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

The three sub-chapters below all use the same hand-written three-line
file so you can run them end to end. Save this as `test-stream.jsonl`:

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

For larger worked examples, see `datasets/`: one directory per category -
[`individual-mesh/`](../datasets/individual-mesh/) (a single actor,
person or agent, no second actor to leak to),
[`multiplayer-mesh/`](../datasets/multiplayer-mesh/) (an org's
people-team stream, each instance carrying its own authority
hard-negative - a VP's or director's informal suggestion that must not
outrank a later, explicit, lower-rank correction), and
[`mixed-player-mesh/`](../datasets/mixed-player-mesh/) (people and an agent sharing
an org scope, the agent also keeping its own private scope). The
multiplayer-mesh and mixed-player-mesh instances also carry a leak-rate trap: a
user- or agent-scoped private note topically close to a later question
that has no org-visible answer at all. Every instance is still small
(3-4 questions): enough to run every command below against something
real, not enough for a believable calibration or stability number yet.

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
content, word-count approximated), then each mean with its n and 95%
bootstrap CI over questions (seeded by `--seed`, so the same command prints the
same interval). When every question gives the same value there is no variance
to resample, so the line says so instead of printing a zero-width interval:

```
=== bm25 ===
  q1: recall@3 = 1.00, tokens injected = 18
  mean recall@3: 1.0000 (n=1, no variance observed: CI undefined)
  mean tokens injected: 18.0000 (n=1, no variance observed: CI undefined)
  leaked returns per returned memory: 0.0000
  trials with a leak: 0/1 events; 95% upper bound 1.0000 (3/n)
  derived brier score = 0.0000; derived expected calibration error = 0.0000 (n=1, no variance observed: CI undefined)
```

(Real output from the three-line stream above, not illustrative.)

`--recency-half-life-days` (default 30) configures `recency`;
`--bounded-window-size` (default 20) configures `bounded-window`. Pass a
comma-separated list to `--systems` to run several baselines against the
same stream in one command.

Each candidate a baseline returns is also scored for calibration
(`MemorySystem::retrieve_scored`), and `--replicates N` reruns a fresh
system `N` times to report which questions flip across runs (`stability::flipped_questions`):

```bash
cargo run --release -- \
  --replay-stream datasets/multiplayer-mesh/atlas-01/stream.jsonl \
  --systems bm25 \
  --top-k 2 \
  --replicates 3 \
  --calibration-bins 2 \
  --seed 0
```

```
=== bm25 ===
  atlas-q-15d: recall@2 = 1.00, tokens injected = 51
  auth-q-20d: recall@2 = 1.00, tokens injected = 59
  atlas-q-30d: recall@2 = 0.00, tokens injected = 51
  mean recall@2: 0.6667 (95% CI 0.0000-1.0000, n=3)
  mean tokens injected: 53.6667 (95% CI 51.0000-59.0000, n=3)
  leaked returns per returned memory: 0.0000
  trials with a leak: 0/3 events; 95% upper bound 1.0000 (3/n)
  derived brier score = 0.1111; derived expected calibration error = 0.0000 (n=3, no variance observed: CI undefined)
  questions that flipped across 3 replicate(s): 0/3 events; 95% upper bound 1.0000 (3/n)
```

(Real output from the shipped `atlas-01` dataset, not illustrative.) It's
*derived*, not native, calibration: none of these local baselines claim a
probability, so their raw score is converted first
(`derive_probabilities_from_scores`) rather than treated as one directly
— conflating the two would credit a baseline with a claim it never made.
Flips are counted per question and reported as events out of n questions; a
zero-event result prints as the 3/n 95% upper bound (here 3/3, capped at 1,
because three questions bound nothing), never as "0%". Zero flips here is
because every local baseline is deterministic; a
noisy system (ACT-R's retrieval noise, a real vendor or Achiral adapter)
is the point of comparison this is built to measure. With only three
questions, the CIs above span most of [0, 1] and the ECE is not a meaningful
number yet — see
[`docs/implementation.md`](implementation.md#whats-pending).

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

### Statistics

Every rate in a replay report prints with its n. Means (recall@k, tokens
injected, gap disclosure) and the ECE carry a 95% percentile-bootstrap CI over
questions (`stats::bootstrap_ci_95`, 2000 resamples, seeded by `--seed`). ECE is
resampled over questions, not candidate rows, because one question's candidates
are not independent. Leaks and flips are per-trial events: a question counts
as a leak if it returned any memory its asker was not authorized to see, and
as a flip if its correctness differed across replicates. Their rate is the
bootstrap over those questions, except a zero-event result, which prints as the
rule-of-three 95% upper bound (`scoring::zero_leak_upper_bound_95`, capped at 1).
"Leaked returns per returned memory" is the older per-memory ratio, kept
alongside. On a generated stream there is one leak line each for the `scope` and `agent`
categories, since the others hold no private memory to leak.

For same-store, same-question comparisons of two systems, `stats::mcnemar_exact`
takes their paired pass/fail vectors and returns the discordant counts and an
exact two-sided p-value. It is a library function; no CLI flag calls it yet.
A gap smaller than the CI width is not a claim.

### Labelling a run

`--label <text>` prints `label: <text>` as the first line of a replay, so output
records settings outside this harness, such as a system's noise scale. The text
is not interpreted:

```bash
cargo run --release -- --replay-stream datasets/generated/trust-seed-0/stream.jsonl \
  --systems bm25 --top-k 3 --label activationNoiseScale=0.2
```

```
label: activationNoiseScale=0.2
=== bm25 ===
```

### Generated stream profiles

`--generate-profile codename` (the default) writes the registered dataset's wording. Its bytes
are pinned by a test. `--generate-profile natural-names` writes a second dataset: two-word
natural project names that recur across trials, several wordings per kind of note, and events
that use a fact slot's name or its alias at random. Same categories and counts, same `--seed`.

```bash
cargo run --release -- --generate-stream datasets/generated/trust-seed-0-natural-names/stream.jsonl \
  --seed 0 --generate-profile natural-names
```

The header line of a variant file records `profile=natural-names`.

### Surviving a restart of the system under test

Against a live system, a deployment restart makes every request fail for a
minute or two. The `achiral` adapter retries instead of stopping the run.

A request is retried when curl cannot reach the server (exit 6, 7, 28, 35, 52,
55 or 56), the status is 502, 503 or 504, or the body is not JSON on any other
non-4xx status. The wait before retry *n* is `min(cap, base * 2^(n-1))`, then
drawn uniformly from its upper half so replicates do not retry in step. Each
retry prints one line to stderr: the reason, the attempt and the delay.
A JSON error body such as `{"error": {...}}` and any 4xx are the API's real
answer and stop the run at once, as before. Once the retries are used up the
run stops with an error; a failed request is never scored.

| Variable | Default | Meaning |
|---|---|---|
| `COGNOSCENTI_ACHIRAL_MAX_RETRIES` | 8 | Retries after the first try, 0 to 10. Anything else is an error. |
| `COGNOSCENTI_ACHIRAL_RETRY_BASE_MS` | 500 | First delay before jitter. At least 1. |
| `COGNOSCENTI_ACHIRAL_RETRY_CAP_MS` | 30000 | Longest delay before jitter. Not below the base. |

With the defaults the delays before jitter are 0.5, 1, 2, 4, 8, 16, 30 and
30 seconds, about 92 seconds in all (46 to 92 with jitter).

A retried write cannot add a second memory. Every write sends
`Idempotency-Key: cognoscenti-<event id>`, and achiral.ai dedupes org events on
the event id and agent `remember` on that key.

`--replicate-log <path>` appends a record when each replicate starts and when it
finishes (its per-question outcomes, and a short hash of its API key, never the
key). Each finished replicate also prints one line, so the output file keeps
every completed replicate even if the run dies:

```
[replicate 2] mean recall@3 = 0.5331; questions fully recalled: 290/544; trials with a leak: 0/141
```

An existing non-empty log is refused unless `--resume` is given. `--resume`
skips the replicates the log shows finished and runs the rest. It refuses to run
a replicate whose API key the log has already seen: the org a crash interrupted
holds part of the stream and the recall history of the questions asked so far,
so it is not a fresh run. Provision a new org and put its key at that
replicate's index in `COGNOSCENTI_ACHIRAL_API_KEYS`. A log from another stream,
seed, `--top-k` or system is refused. When replicate 0 is already in the log,
the preliminary report is not printed again; it is in the earlier output.

### Which routes the `achiral` adapter asks

A question asked by a user goes to `/v1/memory/retrieve`. A question asked by an
agent goes to that route and to `/v1/memory/agents/<slug>/retrieve`, because the
agent route returns agent-scope notes only while the fact an agent needs may be
org-scope. The two result lists are merged (an id keeps its best score), sorted
by score, and cut to top-k. Scores from the two routes are not calibrated against
each other. Leak and recall are scored on the merged list. Results from runs
before this change asked the agent route alone and are not comparable on the
`agent` category.

### Choosing the retrieval mode and params

An evaluation deployment of achiral.ai (`INFERENCE_EVAL_MODE=true`) lets one run
pick how retrieval works per request. Two variables on the `achiral` adapter send
that choice on every retrieve (org, user and agent routes):

| Variable | Header sent | Value |
|---|---|---|
| `COGNOSCENTI_ACHIRAL_RETRIEVAL_MODE` | `x-achiral-retrieval-mode` | one of `rerank`, `aggr`, `graph`, `dense`, `hybrid` |
| `COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS` | `x-achiral-retrieval-params` | a JSON object, for example `{"activationWeightScale":0.25}` |

```bash
COGNOSCENTI_ACHIRAL_RETRIEVAL_MODE=rerank \
COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS='{"activationWeightScale":0.25}' \
cognoscenti --replay-stream dataset.jsonl --systems achiral --skip-user-scope \
  --label "activation scale 0.25" --results-json results/scale-0.25.json
```

Unset (or blank) sends nothing, and the request is exactly what it was before.
A mode outside the list, or params that are not a JSON object, stop the run
before the first event is sent. `aggr-shadow` is an organisation setting the
server ignores as a header, so it is refused. The parameter names and ranges are
the server's (`services/memory/RetrievalParams.js` in achiral.ai).

A server that does not run in evaluation mode ignores both headers. So the
adapter reads the `retrieval` object each retrieve response carries and, on the
first retrieve of every replicate, stops the run if the mode or any requested
param differs from what the server says it used (a value the server clamped, or
a name it does not know, counts as a difference). The echo from replicate 0 is
kept.

The mode, the params and the echo go into the results file under
`run.retrieval`. The cohort name in the report is the label followed by the
setting, `activation scale 0.25 [rerank · activationWeightScale=0.25]`, so a
tuned cohort never appears under, or is merged with, a plain one; the manifest
table has a Retrieval column. A run without them has no `run.retrieval` and
reads "deployment defaults". Only the `achiral` system takes the setting.

### Rebuilding a results file from a replicate log

A run that dies after some replicates finished, then resumes with `--resume`,
writes a results file that holds only the replicates the resumed process ran:
the earlier ones were loaded from the log, which keeps one pass or fail per
question and nothing else. `rebuild-results` makes a complete cohort file from
that log and the stream:

```bash
cognoscenti rebuild-results \
  --replicate-log run.replicates.log --stream stream.jsonl --skip-user-scope \
  --top-k 3 --seed 0 --label activationNoiseScale=0 \
  --summary-lines run.out run.resume.out \
  --out cohort.results.json
cognoscenti report --results baselines.results.json cohort.results.json
```

What it fills in: per-question recall (the share of replicates that passed the
question), pooled recall@k with a 95% bootstrap interval over questions,
per-category recall with n and interval, and the flip rate across replicates,
all with the seed rule a live run uses. A pass is recall@k = 1.0, so the rebuild
refuses any kept question with more than one gold event, and refuses a log from
another stream, seed, top-k or system, a replicate whose question count differs
from the stream's, and replicates not numbered 0 upward without gaps.
`--summary-lines` takes captured output; its `[replicate N] ...` lines supply
the leak counts per replicate and are checked against the log (same top-k, same
pass count, mean equal to passes / n), and a disagreement stops the rebuild.

What it cannot fill in stays absent, never zero: tokens injected, calibration,
the per-category leak table, and the commit that ran. The file lists the
replicates loaded from the log (`run.from_replicate_log`), the manifest limits
say so, and the report's calibration, tokens and leak sections name the cohort
as "not available for replicates loaded from a replicate log".

### Reports

`--results-json <path>` makes a replay write its numbers as JSON: one file per
run, one cohort, named by `--label`. It holds each system's per-question recall
and tokens, recall by category, leak rates, the flip rate across replicates,
derived calibration (Brier, ECE with its CI, reliability bins), tokens by
cycle, and one summary row per replicate, plus the run info (dataset file
name and SHA-256, seed, top-k, replicate count, commit, trials not run, limits).
Every interval comes from `src/stats.rs` with the run's seed, the same calls the
printed report uses. The file name is kept, never the path, and no key or
endpoint is stored.

`cognoscenti report` draws those files as one page:

```bash
cargo run --release -- --replay-stream datasets/generated/trust-seed-0/stream.jsonl \
  --systems verbatim,bm25,recency,full-context,bounded-window --replicates 3 \
  --skip-user-scope --label baselines --results-json results/run/baselines.results.json
cargo run --release -- report --results results/run/baselines.results.json \
  results/run/noise0.results.json results/run/noise02.results.json
```

It writes `index.html`, `results.json` and `results.csv` into `report/` beside
the first results file, or into `--out <dir>`. The page is self-contained:
inline SVG, no script, font or link, light and dark, and no horizontal scroll at
phone width. Colour carries meaning (the Achiral design standard's grayscale
does not apply to reports): see below. The JSON and CSV hold every plotted number, so a chart can be
redrawn from data.

Colour. Each Achiral cohort gets one saturated hue (blue, gold, magenta, indigo
in turn), kept in every chart. Each local baseline gets a quieter hue of its
own. Ahead is green with a ▲, behind is orange-red with a ▼, no clear gap is
grey with a ●, a leak above zero is red and labelled LEAK, and a category under
10 questions is a grey hatch. Every series also has its own marker shape and is
named in a legend, so colour is never the only cue. The palette is the colour-
vision-safe set tested in `src/report.rs`: every graphic colour keeps 3:1
against the card in light and dark, and the cohort hues stay apart under
protan, deutan and tritan simulation. Each chart opens with one line stating
the result with its n (for example the recall gap to the best baseline and its
CI); losses are drawn as large as wins.

Sections: run manifest and limits; recall by category for every system, with
a table of questions each baseline won; flip rate; calibration; tokens by
cycle (from a `--cycles` run); leaks with the 3/n bound; a metric against a
setting; the replicates. Rate axes start at zero. A category under 10 questions
shows "n too small" and no point.

Each results file is its own cohort. Two files with the same label are
refused: cohorts are never merged. A baseline run is a cohort too, so label it
(`--label baselines`); its rows sit beside the Achiral cohort's on the same
chart. The head-to-head table compares two cohorts only when they ran the same
dataset (same SHA-256), on the questions both ran.

The settings charts are built from labels alone. A label is read as
space- or comma-separated `key=value` words. Cohorts whose labels differ in one
key and agree on every other form a group: a numeric key
(`activationNoiseScale=0`, `=0.2`) draws Achiral's overall recall against that
key, and a text key (`mode=aggr`, `mode=rerank`) draws one row per value.

The report refuses to write if its text holds `http`, `://`, `acm_`, `Bearer`,
`localhost`, an adapter env-variable name, or the value of
`COGNOSCENTI_ACHIRAL_API_KEY(S)` or `_BASE_URL`.

### Skipping user-scope trials

`--skip-user-scope` drops every generated trial that holds a user-scope event and
prints how many it dropped, by category. Use it against a deployment whose
per-member routes are disabled: a refused write is not evidence that nothing
leaks, so those trials are reported as "not run", never scored. Hand-made
datasets are never touched. On the default generated stream it keeps 544 of 860
questions (drops 316: 229 `scope`, 52 `owner`, 35 `individual`).

### Generated streams

What it's for: enough questions for a CI that means something. The nine
hand-made datasets hold 22 questions between them. `--generate-stream` writes a
synthetic stream from fixed word lists and the `--seed`, with no model in the
loop, and lints it before exiting:

```bash
cargo run --release -- --generate-stream datasets/generated/trust-seed-0/stream.jsonl --seed 0
```

```
Generated datasets/generated/trust-seed-0/stream.jsonl: 3002 event(s), 860 question(s), seed 0
sha256 2fc8d6c2ba4e9cff4979945653264ddcc79cec1e86e7038c8a0feccf7f5693e2
Dataset lint: clean (datasets/generated/trust-seed-0/stream.jsonl)
```

The same seed gives the same bytes (and the same checksum). `--scope-trials`
(default 300) sets the scope trials; `--items-per-category` (default 70) sets
every other category. Question ids encode the category:

- `scope-NNN-q`: a user's or agent's private note, then a different asker
  (sometimes an agent) asks about it. Even trials have no org-visible answer, so
  any returned memory is a leak. Odd trials also have an org-visible answer
  that the private note competes with.
- `owner-NNN-q`: the asker owns the private note (a user's or an agent's) and
  asks for it back; gold is that note. This is the authorized counterpart of
  `scope`: a system that refuses everything scores zero leaks and zero recall.
- `individual-NNN-q`: one actor and their own scope, no second party. Half are
  a person's notes, half an agent's task state (like `sam-01` and `scout-01`).
  A fact is stated and possibly changed up to twice, with two filler facts
  about the same subject; every second question rephrases the slot.
- `agent-NNN-q`: agent A posts a fact to the org and revises it; A also keeps a
  private scratchpad. Agent B asks. Gold is A's last post; the scratchpad is a
  leak if it comes back.
- `supersede-NNN-q`: a fact decided and changed two or three times; gold is the
  last event. Every third chain also holds an informal aside that was never
  adopted.
- `retrieve-NNN-q`: one org-visible fact among five other facts about the same
  project and one decoy for another project. Every second question rephrases the
  slot, so only the project name is guaranteed to overlap.
- `paraphrase-NNN-q` and `rephrase-NNN-q`: one fact and three same-project
  distractors, asked twice by the same asker in different words. The second
  wording shares only the project name with the event. Replay scores how much
  of the returned evidence the two wordings share and how often only one found
  the answer.
- `stable-NNN-q`: a fact stated once and never changed, followed by 4 to 20
  later updates to other facts about the same project. Replay reports accuracy
  separately for up to 8 later updates and for more than 8.

Replaying it prints a line per category, each with its own n and CI:

```bash
cargo run --release -- --replay-stream datasets/generated/trust-seed-0/stream.jsonl \
  --systems bm25,recency --top-k 3 --replicates 3 --seed 0
```

```
=== bm25 ===
  mean recall@3: 0.9093 (95% CI 0.8895-0.9279, n=860)
  mean tokens injected: 37.1419 (95% CI 36.4616-37.8605, n=860)
  category paraphrase: recall@3 1.0000 (n=70, no variance observed: CI undefined)
  category rephrase: recall@3 0.9143 (95% CI 0.8429-0.9714, n=70)
  category scope: recall@3 1.0000 (n=300, no variance observed: CI undefined)
  category individual: recall@3 0.8286 (95% CI 0.7429-0.9143, n=70)
  category retrieve: recall@3 0.7857 (95% CI 0.6857-0.8857, n=70)
  category stable: recall@3 0.7714 (95% CI 0.6714-0.8714, n=70)
  category agent: recall@3 1.0000 (n=70, no variance observed: CI undefined)
  category owner: recall@3 1.0000 (n=70, no variance observed: CI undefined)
  category supersede: recall@3 0.5857 (95% CI 0.4714-0.7000, n=70)
  paraphrase pairs: evidence agreement (Jaccard of returned memories) 0.4129 (95% CI 0.3443-0.4829, n=70)
  paraphrase pairs where only one wording found the answer: 6/70 events; 0.0857 (95% CI 0.0286-0.1571, n=70)
  unchanged fact with up to 8 later updates: accuracy 0.9286 (95% CI 0.8214-1.0000, n=28)
  unchanged fact with more than 8 later updates: accuracy 0.6667 (95% CI 0.5238-0.8095, n=42)
  scope: trials with a leak: 297/300 events; 0.9900 (95% CI 0.9767-1.0000, n=300) (leaked returns per returned memory: 0.5189)
  agent: trials with a leak: 66/70 events; 0.9429 (95% CI 0.8857-0.9857, n=70) (leaked returns per returned memory: 0.3143)
  derived brier score = 0.1566; derived expected calibration error = 0.0000 (95% CI 0.0000-0.0000, n=860)
  questions that flipped across 3 replicate(s): 0/860 events; 95% upper bound 0.0035 (3/n)
=== recency ===
  mean recall@3: 0.6744 (95% CI 0.6430-0.7047, n=860)
  mean tokens injected: 34.9547 (95% CI 34.3988-35.5442, n=860)
  category paraphrase: recall@3 0.0000 (n=70, no variance observed: CI undefined)
  category rephrase: recall@3 0.0000 (n=70, no variance observed: CI undefined)
  category scope: recall@3 1.0000 (n=300, no variance observed: CI undefined)
  category individual: recall@3 1.0000 (n=70, no variance observed: CI undefined)
  category retrieve: recall@3 0.0000 (n=70, no variance observed: CI undefined)
  category stable: recall@3 0.0000 (n=70, no variance observed: CI undefined)
  category agent: recall@3 1.0000 (n=70, no variance observed: CI undefined)
  category owner: recall@3 1.0000 (n=70, no variance observed: CI undefined)
  category supersede: recall@3 1.0000 (n=70, no variance observed: CI undefined)
  paraphrase pairs: evidence agreement (Jaccard of returned memories) 1.0000 (n=70, no variance observed: CI undefined)
  paraphrase pairs where only one wording found the answer: 0/70 events; 95% upper bound 0.0429 (3/n)
  unchanged fact with up to 8 later updates: accuracy 0.0000 (n=28, no variance observed: CI undefined)
  unchanged fact with more than 8 later updates: accuracy 0.0000 (n=42, no variance observed: CI undefined)
  scope: trials with a leak: 300/300 events; 1.0000 (n=300, no variance observed: CI undefined) (leaked returns per returned memory: 0.4922)
  agent: trials with a leak: 70/70 events; 1.0000 (n=70, no variance observed: CI undefined) (leaked returns per returned memory: 0.3333)
  derived brier score = 0.1151; derived expected calibration error = 0.0000 (95% CI 0.0000-0.0000, n=860)
  questions that flipped across 3 replicate(s): 0/860 events; 95% upper bound 0.0035 (3/n)
```

(Real output, per-question lines omitted. bm25 and recency ignore scope, so
they leak on most `scope` and `agent` trials; that is the baseline a
scope-aware system is compared against. `recency` scores 1.0 wherever the
answer is the newest event, which is a ceiling, not skill.) The derived ECE is zero because derived probabilities are
fitted to the same ranks they are scored on; calibration against a system's
own probabilities is where ECE carries information.

# Pre-registration: Track A, Achiral trust and tokens (November 2026)

Status: written before any Achiral run. Plan 40 Stage 2 item 2. Anything
that changes after this commit is recorded in the Deviations section at
the end, with its date and reason, in a later commit. Nothing above that
section is edited in place.

## What is being measured

Achiral alone, over HTTP, in `rerank` mode, against a deployment with
`INFERENCE_EVAL_MODE=true`, beside Cognoscenti's local baselines (`verbatim`,
`bm25`, `recency`, `full-context`, `bounded-window`) on the same streams. No vendor
system, no reader model, no judge model. Every metric is retrieval-level.

The claim this run can support is narrow: how Achiral's retrieval behaves on a
seeded synthetic stream on three axes (calibration, stability, scope) and on
tokens injected. It supports no claim about answer quality, no head-to-head
claim, and no graph-retrieval claim.

## Dataset

| Field | Value |
|---|---|
| File | `datasets/generated/trust-seed-0/stream.jsonl` |
| sha256 | `2fc8d6c2ba4e9cff4979945653264ddcc79cec1e86e7038c8a0feccf7f5693e2` |
| Generator | `cognoscenti --generate-stream <path> --seed 0` |
| Generator commit | `91023ff` (the file was first generated at `39a4223`; `91023ff` regenerates identical bytes) |
| Size | 3002 events, 860 questions |
| Public copy | `achiral-ai/cognoscenti` on Hugging Face, `generated/trust-seed-0/stream.jsonl`, same sha256 |

Questions by category (the question id encodes the category):

| Category | n | What it tests |
|---|---|---|
| `scope` | 300 | A private note, then a different asker. Any returned private memory is a leak. |
| `owner` | 70 | The asker owns the private note and asks for it back (authorized counterpart of `scope`). |
| `individual` | 70 | One actor and their own scope (35 user, 35 agent). |
| `agent` | 70 | Agent-to-agent handoff over org scope, with a private scratchpad behind it. |
| `supersede` | 70 | A fact changed two to four times; gold is the last event. |
| `retrieve` | 70 | One fact among same-project distractors and a decoy project. |
| `paraphrase` / `rephrase` | 70 pairs | The same question in two wordings. |
| `stable` | 70 | A fact that never changes, under 4 to 20 later updates to other facts. |

Hand-made regression datasets (`datasets/`, unchanged) are replayed separately and
reported separately. They are never pooled with the generated stream.

### The subset Achiral can run today

As of `achiral.ai` `feat/benchmarks` (2026-11-16), every per-member route
(`/v1/memory/users/:userId/{events,retrieve,recall,search}`) returns 403
`user_scope_unavailable` (Plan 41). A user-scope write cannot reach Achiral, and the
adapter stops the run on an ingest failure. So the first Achiral run covers only
questions whose events Achiral can store:

| Included | n |
|---|---|
| `supersede`, `retrieve`, `paraphrase`, `rephrase`, `stable` (org scope only) | 350 |
| `scope` where the private note is agent-scoped (asker is a user) | 71 |
| `individual` with an agent actor | 35 |
| `owner` where the private note is agent-scoped | 18 |
| `agent` (see the open question below) | 70 |

| Excluded, not testable through the API | n |
|---|---|
| `scope` with a user-scoped private note | 229 |
| `owner` with a user-scoped private note | 52 |
| `individual` with a user actor | 35 |

The excluded trials are reported as "not run: user scope unavailable (Plan 41)". They
are never counted as passed, never counted as zero leaks, and never dropped from the
denominator silently. A refused write is not evidence that nothing leaks.

Open question, to be settled by a one-org smoke check before the run and recorded as a
deviation if it changes anything: whether `/v1/memory/agents/{slug}/retrieve` returns
org-scope memory. The `agent` category asks agent B about a fact agent A posted to the
org. If the agent route returns only that agent's own memory, `agent` recall is zero by
the API's design and is reported as such, not as a retrieval failure.

## Systems and cohorts

| Cohort | Orgs | `activationNoiseScale` | Mode | Label |
|---|---|---|---|---|
| s = 0 | at least 5, one org per replicate | 0 | `rerank` | `--label activationNoiseScale=0` |
| noise 0.2 | at least 5, separate orgs | 0.2 | `rerank` | `--label activationNoiseScale=0.2` |

- The two cohorts use the same streams, are run as separate processes, are labelled in
  every table, and are never merged. A row is either "s = 0" or "noise 0.2".
- 0.2 is the low end of ACT-R's published `:ans` range [0.2, 0.8]
  (`docs/act-r-reference-fixtures.md`) and the value of the 2026-11-13 `atlas-01` cohort.
- Orgs are provisioned fresh for each cohort, with the mode and noise scale set at
  creation. Each org's id, mode and scale are recorded in the run manifest notes.
- The scale 0.2 is registered here. No other scale is run in this batch.
- The `activationNoiseScale` is the system's setting, not the dataset's. The dataset is
  identical for both cohorts.

## Parameters

| Parameter | Value |
|---|---|
| `--top-k` | 3 |
| `--replicates` | 5 per cohort (one org each) |
| `--seed` | 0 (bootstrap resampling and anything else Cognoscenti randomises) |
| Bootstrap | 95% percentile interval, 2000 resamples, the question as the unit |
| Zero-event results | Reported as the 3/n upper bound, never as 0% |
| Cycle checkpoints | 10 and 100 (`--cycles`), on one org per cohort. 1000 only if ingestion cost allows. |
| Token count | Whitespace-word approximation (`src/tokens.rs`), read path only |
| Paired tests | Exact McNemar, library only (no CLI flag), applied to baseline-versus-Achiral on the same questions and reported with the discordant counts |

A hit means recall@k above 0. For single-gold questions that is the same as recall 1.

## Metrics, one table per axis

Each reported figure carries n and a 95% CI. Each category is reported on its own line.
There is no pooled headline.

- **Calibration:** Brier and ECE if and only if Achiral returns its own confidence
  scores. For the local baselines the probabilities are derived from rank and ECE is 0
  by construction, so it is not reported as a result. If Achiral returns no native
  score, calibration is reported as "not measured", not as a derived number.
- **Stability:**
  - flip rate across the 5 replicates, with the 3/n bound when no question flips;
  - paraphrase agreement (Jaccard of the returned memories) over `paraphrase` /
    `rephrase` pairs, and the share of pairs where only one wording found the answer;
  - unchanged-fact accuracy for histories of up to 8 later updates versus more than 8
    (`stable`).
  At s = 0 the pipeline is deterministic, so flip rate there is expected to be zero and
  says nothing about noise. The noise 0.2 cohort is where it can move.
- **Scope:** trials with a leak out of n, per leak-bearing category (`scope`, `agent`),
  with the 3/n bound when there is none; recall on `owner` as the check that refusing
  everything does not score well.
- **Tokens:** mean tokens injected per question, with CI, and by cycle at 10 and 100.

## What counts as a claim

1. A figure is a claim only with its n and 95% CI beside it, from the committed manifest.
2. A "<x%" leak claim states the trials actually run. With 71 agent-scoped `scope`
   trials the 3/n bound is about 4.2%. A "<1%" claim needs about 300 trials and is not
   available from this batch. User-scope leakage is not claimed at all.
3. A leak found in any scope trial blocks publication of the scope axis until it is
   fixed and the batch is re-run.
4. Stability under noise is claimed as "stability under noise" and nothing more. No
   claim that noise helps.
5. A category where Achiral loses to a local baseline is reported in the same table.
6. Each category, and each of the two cohorts, is read on its own. A category whose CI is
   wider than 22 points is labelled as such.
7. Wording: "non-deterministic, probabilistic, ACT-R-style". No "Bayesian" claim. No
   graph-retrieval claim. Models Achiral runs are not named.
8. Anything not listed here is exploratory and is labelled exploratory.

## Open limitations, stated in the report

- Derived ECE is 0 by construction for every baseline.
- McNemar has no CLI flag, so paired tests are run from the library and recorded in the
  bundle.
- Recovery time after a change is not measured.
- Tokens are read path only. Write-path tokens (ingestion LLM calls) are not measured by
  Cognoscenti.
- User-scope behaviour is untested until Plan 41 provides an audited route. Three
  generated categories lose most of their trials to this.
- The generated stream is synthetic and lexically simple. Results do not transfer to real
  conversations without further evidence.
- `rerank` mode only. `aggr` is Track B.
- The first-batch stream is one seed. Seed variation is not measured.

## Run record

The manifest of each run (`RunManifest`: dataset path and sha256, seed, Cognoscenti
commit, systems, top-k) is committed beside its results, together with the `--label`, the
org ids and the settle time used. Keys and tokens are never written to the bundle.

## Deviations

Recorded 2026-11-17, before the first cohort run. Each is a change from the text above.

1. **System under test.** `achiral.ai` commit `a2cd16a0` on `feat/benchmarks`, served by
   `development-api-2` (`INFERENCE_EVAL_MODE=true`, `MEMORY_WRITE_INTERVAL=10 seconds`).
   That commit carries a fix, made after this pre-registration was written, that keeps
   agent-scoped memory out of organization-scope recall. A pre-run probe found the old
   behaviour returning an agent's private note to an org-scope asker. The leak fix is part
   of the system being measured. Results say nothing about earlier builds.
2. **User-scope trials dropped (316 of 860 questions).** `scope` 229, `owner` 52,
   `individual` 35. The per-member routes return 403 `user_scope_unavailable` (Plan 41).
   Replay uses `--skip-user-scope`, which drops every generated trial holding a user-scope
   event. It keeps 544 questions, the subset named in "The subset Achiral can run today". A
   test pins the 544 / 316 split. The dropped trials are reported as not run.
3. **The 70 `agent` questions are expected to score 0 by API design.** A smoke check on a
   separate org showed `/v1/memory/agents/{slug}/retrieve` returns agent-scope memory only
   and no org-scope memory, so agent B cannot see agent A's org post. This resolves the open
   question in the subset section. The category is still run and reported, labelled "zero by
   API design, not a retrieval result", and is excluded from any claim about retrieval
   quality. The `agent` leak line still counts: it tests whether A's scratchpad reaches B.
4. **Cycles not run.** The 10 and 100 checkpoints are deferred. Re-ingesting about 2,300 org
   events per cycle through the real write path at about 7 seconds per settled question is
   estimated at days for 100 cycles. A cycle-10 run may follow on a separate, later
   approval and will be recorded here.
5. **Adapter fix before the run.** The adapter previously treated an HTTP error body such as
   `agent_not_found` as a successful write (commit `8471f4e`, `main`). It now stops on any
   API error. Earlier smoke results on orgs `ct-a-s0-1` and `ct-run-smoke` came from a
   seed-99 smoke stream that is not part of the registered dataset, and none of them is
   reported.
6. **Execution.** Each cohort is one process over five fresh orgs (`ct-run-s0-1..5` at
   `activationNoiseScale` 0, `ct-run-s02-1..5` at 0.2, both `rerank`), run in parallel with
   `COGNOSCENTI_ACHIRAL_SETTLE_MS=15000`. The five local baselines run once, separately, on
   the same 544 questions, because they are deterministic and do not depend on the cohort.
   Cognoscenti commit for the run: `8471f4e`.
7. **First start aborted, orgs discarded.** About three minutes into the first start, both
   cohorts logged `settle(): 4 event(s) not confirmed written within 15000ms` (the adapter
   warned and went on, so those events were absent when the next question was scored). I
   stopped both processes. Orgs `ct-run-s0-1` and `ct-run-s02-1` held partial data and are
   discarded; their output is kept under `results/track-a-2026-11/aborted-1/` and none of
   it is reported. The adapter now stops the run on an unconfirmed write instead of
   warning (this commit), and the poll budget is set apart from the blind wait with
   `COGNOSCENTI_ACHIRAL_CONFIRM_MS`. The restart uses a replacement org for each discarded
   one. `COGNOSCENTI_ACHIRAL_SETTLE_MS` in item 6 applies to the blind wait only.
8. **Second dataset variant, `natural-names`, registered 2026-11-17. Not yet run against
   Achiral.** The registered dataset (sha256 `2fc8d6c2…93e2`, unchanged and byte-identical, a
   test pins it) gives every trial a unique coined project name and uses one wording per kind of
   note. Replicate 0 showed `bm25` far ahead of Achiral on categories where the question names
   the project (`bm25` 1.00 on `paraphrase`, Achiral 0.61), and the diagnosis reads that as the
   coined token acting as a lexical giveaway to a lexical system and as a rare unseen token to a
   dense one. The variant keeps the categories, counts, seeding and determinism but changes two
   things: project names are two natural words (36 adjectives by 36 nouns, each word shared by
   about 25 projects, only the pair unique), and each kind of note has several wordings, with
   events using a slot's name or its alias at random.

   | Field | Value |
   |---|---|
   | File | `datasets/generated/trust-seed-0-natural-names/stream.jsonl` |
   | sha256 | `b3e7ba723ddf28a62a9a36613f80f617b2c8bcb9578baeb8c9243be14c72881c` |
   | Generator | `cognoscenti --generate-stream <path> --seed 0 --generate-profile natural-names` |
   | Size | 2984 events, 860 questions (same category counts as the registered dataset) |
   | With `--skip-user-scope` | 543 questions kept, 317 trials dropped (`scope` 232, `owner` 55, `individual` 30). The random draws differ from the registered dataset, so the split differs by one question. |

   The variant is a separate dataset with its own rows. It is never pooled with the registered
   dataset's results and is not a replacement for it. A comparison on it will be a labelled
   cohort set, announced as its own deviation before it runs. Local baselines were run on the
   variant first: `bm25` recall@3 on the 543 kept questions is 0.694 (0.870 on the registered
   subset), so the gap to a lexical baseline narrows by design.
9. **Retry on transient failures, a per-replicate log and `--resume`, 2026-11-17.** Both
   cohorts died on 2026-11-17 (s0 at 19:10Z "connection refused", s02 at 19:17Z HTTP 502)
   because the dev API containers were recreated; the replay panicked on the first failed
   request and lost replicates 1 to 4 (replicate 0 had finished with no leaks). The adapter now
   retries connection errors, HTTP 502/503/504 and non-JSON error pages up to 8 times with
   capped exponential backoff and jitter (0.5 s base, 30 s cap, upper-half jitter, about 92 s
   in all before jitter), then stops with an error; real API error bodies and 4xx still stop at
   once, and a failed request is never scored. Writes are retried because they carry an
   `Idempotency-Key` and achiral.ai dedupes on it, so a retry cannot duplicate an event. This
   changes no query, dataset, metric or scoring rule. Each replicate now prints a summary line
   and is appended to a log as it ends. The crashed runs' data is not reused: the cohorts
   restart on freshly provisioned orgs, and `--resume` refuses any org the log has already
   started. Replicates from the crashed runs are discarded, not merged.

10. **HTML report from machine-readable results, 2026-11-18.** The replay output was text only,
   so a replay can now write `--results-json` and `cognoscenti report` draws it. The report
   states the per-question counts and 95% CIs the printed report states, from the same
   `stats.rs` calls and seed, and adds no metric, dataset or scoring rule. A category of fewer
   than 10 questions is shown as "n too small" without an interval. The runs already finished
   before this change (the crashed cohorts) printed text only and have no results file; the
   report reads files written from now on.


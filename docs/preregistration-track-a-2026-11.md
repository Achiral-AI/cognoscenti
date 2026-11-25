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

| Field            | Value                                                                                        |
| ---------------- | -------------------------------------------------------------------------------------------- |
| File             | `datasets/generated/trust-seed-0/stream.jsonl`                                               |
| sha256           | `2fc8d6c2ba4e9cff4979945653264ddcc79cec1e86e7038c8a0feccf7f5693e2`                           |
| Generator        | `cognoscenti --generate-stream <path> --seed 0`                                              |
| Generator commit | `91023ff` (the file was first generated at `39a4223`; `91023ff` regenerates identical bytes) |
| Size             | 3002 events, 860 questions                                                                   |
| Public copy      | `achiral-ai/cognoscenti` on Hugging Face, `generated/trust-seed-0/stream.jsonl`, same sha256 |

Questions by category (the question id encodes the category):

| Category                  | n        | What it tests                                                                             |
| ------------------------- | -------- | ----------------------------------------------------------------------------------------- |
| `scope`                   | 300      | A private note, then a different asker. Any returned private memory is a leak.            |
| `owner`                   | 70       | The asker owns the private note and asks for it back (authorized counterpart of `scope`). |
| `individual`              | 70       | One actor and their own scope (35 user, 35 agent).                                        |
| `agent`                   | 70       | Agent-to-agent handoff over org scope, with a private scratchpad behind it.               |
| `supersede`               | 70       | A fact changed two to four times; gold is the last event.                                 |
| `retrieve`                | 70       | One fact among same-project distractors and a decoy project.                              |
| `paraphrase` / `rephrase` | 70 pairs | The same question in two wordings.                                                        |
| `stable`                  | 70       | A fact that never changes, under 4 to 20 later updates to other facts.                    |

Hand-made regression datasets (`datasets/`, unchanged) are replayed separately and
reported separately. They are never pooled with the generated stream.

### The subset Achiral can run today

As of `achiral.ai` `feat/benchmarks` (2026-11-16), every per-member route
(`/v1/memory/users/:userId/{events,retrieve,recall,search}`) returns 403
`user_scope_unavailable` (Plan 41). A user-scope write cannot reach Achiral, and the
adapter stops the run on an ingest failure. So the first Achiral run covers only
questions whose events Achiral can store:

| Included                                                                     | n   |
| ---------------------------------------------------------------------------- | --- |
| `supersede`, `retrieve`, `paraphrase`, `rephrase`, `stable` (org scope only) | 350 |
| `scope` where the private note is agent-scoped (asker is a user)             | 71  |
| `individual` with an agent actor                                             | 35  |
| `owner` where the private note is agent-scoped                               | 18  |
| `agent` (see the open question below)                                        | 70  |

| Excluded, not testable through the API  | n   |
| --------------------------------------- | --- |
| `scope` with a user-scoped private note | 229 |
| `owner` with a user-scoped private note | 52  |
| `individual` with a user actor          | 35  |

The excluded trials are reported as "not run: user scope unavailable (Plan 41)". They
are never counted as passed, never counted as zero leaks, and never dropped from the
denominator silently. A refused write is not evidence that nothing leaks.

Open question, to be settled by a one-org smoke check before the run and recorded as a
deviation if it changes anything: whether `/v1/memory/agents/{slug}/retrieve` returns
org-scope memory. The `agent` category asks agent B about a fact agent A posted to the
org. If the agent route returns only that agent's own memory, `agent` recall is zero by
the API's design and is reported as such, not as a retrieval failure.

## Systems and cohorts

| Cohort    | Orgs                              | `activationNoiseScale` | Mode     | Label                              |
| --------- | --------------------------------- | ---------------------- | -------- | ---------------------------------- |
| s = 0     | at least 5, one org per replicate | 0                      | `rerank` | `--label activationNoiseScale=0`   |
| noise 0.2 | at least 5, separate orgs         | 0.2                    | `rerank` | `--label activationNoiseScale=0.2` |

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

| Parameter          | Value                                                                                                                                       |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `--top-k`          | 3                                                                                                                                           |
| `--replicates`     | 5 per cohort (one org each)                                                                                                                 |
| `--seed`           | 0 (bootstrap resampling and anything else Cognoscenti randomises)                                                                           |
| Bootstrap          | 95% percentile interval, 2000 resamples, the question as the unit                                                                           |
| Zero-event results | Reported as the 3/n upper bound, never as 0%                                                                                                |
| Cycle checkpoints  | 10 and 100 (`--cycles`), on one org per cohort. 1000 only if ingestion cost allows.                                                         |
| Token count        | Whitespace-word approximation (`src/tokens.rs`), read path only                                                                             |
| Paired tests       | Exact McNemar, library only (no CLI flag), applied to baseline-versus-Achiral on the same questions and reported with the discordant counts |

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

   | Field                    | Value                                                                                                                                                                         |
   | ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
   | File                     | `datasets/generated/trust-seed-0-natural-names/stream.jsonl`                                                                                                                  |
   | sha256                   | `b3e7ba723ddf28a62a9a36613f80f617b2c8bcb9578baeb8c9243be14c72881c`                                                                                                            |
   | Generator                | `cognoscenti --generate-stream <path> --seed 0 --generate-profile natural-names`                                                                                              |
   | Size                     | 2984 events, 860 questions (same category counts as the registered dataset)                                                                                                   |
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

11. **Second start of the registered cohorts, 2026-11-18, and the natural-names cohorts
    announced.** Both registered cohorts (`activationNoiseScale=0` and `activationNoiseScale=0.2`,
    5 replicates each, registered dataset sha256 `2fc8d6c2…93e2`, `--skip-user-scope`, top-k 3,
    seed 0) restarted at 2026-11-18T23:46:54Z on freshly provisioned orgs `ct2-s0-1` to `-5` and
    `ct2-s02-1` to `-5` (`rerank` mode, agents `report-agent`, `scribe-agent`, `triage-agent` and
    `ops-agent`), with Cognoscenti `7a6d238` (retry, per-replicate log, results JSON). The
    achiral.ai build under test is `feat/benchmarks` at `b1269043` plus uncommitted dev-only changes
    (compose port mapping for the container Ollama). It carries what the first start did not:
    the org-recall pre-filter that stops agent and client notes from displacing org results
    (`a77ab8e7`, `817beef6`), member-path activation noise (`ca0a1134`) and the Node 26.13.1 image.
    The dev API's model backend is now a host-side Ollama; retrieval in this run uses no model.
    The earlier cohort data (the aborted and crashed starts) is discarded and not merged.
    **Announced before it runs:** the `natural-names` variant (item 8, sha256 `b3e7ba72…881c`)
    will run as its own labelled cohort set, 5 replicates at each of `activationNoiseScale=0` and
    `activationNoiseScale=0.2` on further fresh orgs, started only after the registered cohorts
    finish, with the same flags. Its rows are reported beside, never pooled with, the registered
    dataset's rows.
12. **The second start failed within 35 seconds; third start, 2026-11-18T23:54:13Z.** The
    second start (item 11, orgs `ct2-*`) stopped on its first agent-scope write with
    `agent_memory_write_failed`. Cause: achiral.ai's `AgentMemoryStore` looked up an existing
    write by idempotency key with a plain `{field: value}` filter map, which the Weaviate client
    rejects (`Unsupported filter path`). The path never ran before, because the adapter sent no
    idempotency key until the retry change (deviation 9). The fix builds the lookup from
    `andFilter(equalFilter(...))` and is test-first (`AgentMemoryStore.test.js`, now run through
    the real filter converter). It changes no retrieval, ranking, dataset or scoring rule. The
    `ct2-*` orgs and their partial data are discarded (kept under `aborted-2/`, none reported);
    the cohorts restarted on fresh orgs `ct3-s0-1` to `-5` and `ct3-s02-1` to `-5` with the same
    flags and Cognoscenti `7a6d238`. The announcement in item 11 still holds: `natural-names`
    cohorts start only after these finish.
13. **Blind settle wait lowered for the natural-names cohorts, 2026-11-19.** Replicate 0 of the
    registered cohorts took 1 h 49 min (started 2026-11-18T23:54:13Z, finished 2026-11-19T01:43:21Z).
    `settle()` runs before every question; it polls for confirmation when org-scope writes are
    pending and otherwise sleeps `COGNOSCENTI_ACHIRAL_SETTLE_MS` (15 s). In the 860-question stream
    142 settles are blind, about 35 minutes at 15 s. A probe on a fresh org (20 distinct agent notes,
    each retrieved right after its write) found every note stored and, where the note ranked in the
    top 3, retrievable within 0.5 s of the write. The `natural-names` cohorts therefore run with
    `COGNOSCENTI_ACHIRAL_SETTLE_MS=2000`. The registered cohorts keep 15000. Org-scope confirmation
    polling is unchanged. The setting is recorded per cohort and the two datasets are never pooled.
14. **Retrieval-mode and params headers added to the adapter, 2026-11-19.** The `achiral`
    adapter can now send `x-achiral-retrieval-mode` and `x-achiral-retrieval-params` on every
    retrieve (`COGNOSCENTI_ACHIRAL_RETRIEVAL_MODE`, `COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS`), for
    the activation-weight sweeps. It changes nothing for a run that sets neither variable: the
    request is byte for byte what it was, and the registered cohorts (items 11 to 13) run without
    them. A run that sets them stops unless the server echoes the same mode and params on the
    first retrieve of each replicate, records the requested setting and the echo in the results
    file, and carries the setting in its cohort name, so it is reported beside, never pooled
    with, a cohort without one. Valid modes are the server's `rerank`, `aggr`, `graph`, `dense`
    and `hybrid`; `aggr-shadow` is not sent as a header.
15. **Third start interrupted at replicate 1; resumed, 2026-11-19.** Replicate 0 finished in both
    registered cohorts (2026-11-19T01:43:21Z, 1 h 49 min; recall@3 0.559 at `s = 0`, 0.544 at
    `s = 0.2`). Both processes stopped at about 02:22:55Z during replicate 1 (`settle(): ... not
confirmed written within 120000ms` after HTTP 502 retries). Cause: at 02:17:31Z the operator
    edited achiral.ai source in the checkout the dev API runs from; the API restarts on source
    changes (`node --watch`) and was down for about 40 s with writes in flight, and a heavy test run
    shared the machine. Replicate 1's orgs (`ct3-s0-2`, `ct3-s02-2`) hold partial data and are
    discarded (outputs kept under `run3/crashed-1/`, none reported). The cohorts resumed at
    2026-11-19T02:47Z with `--resume`: replicate 0 comes from the replicate log, replicates 1 to 4
    run on fresh orgs `ct5-s0-1` to `-4` and `ct5-s02-1` to `-4`. Same binary (`7a6d238`), dataset,
    flags and 15 s settle. No achiral.ai source is edited while cohorts run.
16. **The registered cohorts' report is rebuilt from the replicate log, 2026-11-19.** Replicate 0
    of each registered cohort ran in a process that crashed in replicate 1 before writing results;
    the resumed process loaded replicate 0 from the replicate log and ran replicates 1 to 4, so its
    results files hold only per-replicate summary rows and the flip rate. The cohorts' report is
    therefore built with `cognoscenti rebuild-results` from each replicate log, the registered
    stream (sha256 `2fc8d6c2…93e2`, `--skip-user-scope`, top-k 3, seed 0) and the printed summary
    lines. It gives per-question recall (the share of the five replicates that passed it), pooled
    recall@3 with a 95% bootstrap interval over the 544 kept questions (equal to the mean of the
    five per-replicate recalls, because every kept question has one gold event or none), per-category
    recall, and the flip rate. It lacks tokens injected, calibration, the per-category leak table and
    the run's commit; leak counts per replicate are taken from the printed lines (0 of 141 trials in
    every replicate shown) and are not reproducible from the log. The rebuilt file lists replicates
    0 to 4 as loaded from the log, and the report says so in each section it cannot fill. Nothing in
    the cohort run, its logs or its orgs was changed.
17. **Agent questions ask the org route as well, 2026-11-19.** In every Achiral run so far the
    `agent` category scored 0 of 70. Its gold fact is an org-scope event (for example
    `agent-000-e2`, written by `report-agent`) and the question is asked by another agent
    (`ops-agent`) through `/v1/memory/agents/<slug>/retrieve`, which returns agent-scope notes only;
    a live check returned 1 result from the org route and 0 from the agent route. The 0 was
    structural, not a measurement, and so was the category's leak count of 0: that route could not
    return an org event or another agent's note to the asker. From this change the adapter asks an
    agent's question on both routes, merges the lists by score (an id keeps its best score),
    and cuts to top-k; scores from the two routes are not calibrated against each other. Leak
    accounting is unchanged and runs on the merged list. Agent-category results from the registered
    cohorts (items 11 to 15) and earlier runs are not comparable with runs that include this change;
    the other categories are unaffected, because only agent-asked questions change. Those results are
    kept and reported as run, with this note.
18. **Natural-names cohorts interrupted at replicate 4; resumed, 2026-11-19.** Replicates 0 to 3
    finished in both natural-names cohorts (09:37Z start, 2 s settle, ~1 h 18 min each). Both
    processes stopped at about 14:34Z on the first writes of replicate 4 (`settle(): ... not
confirmed written within 120000ms`). Cause: host overload. A concurrent cargo test and mutation
    run on the same machine drove the load average to about 150 on 12 cores; MongoDB and Weaviate
    timed out (Weaviate 30 s request timeouts, MongoDB connection loss) while the API stayed up.
    The replicate-4 orgs (`ct4-s0-5`, `ct4-s02-5`) hold partial data and are discarded (outputs
    under `run3/crashed-natural-1/`, none reported). One first resume attempt failed at startup
    with a key-list error and wrote no replicate record. Replicate 4 resumed with `--resume` at about
    14:50Z on fresh orgs `ct7-s0-1` and `ct7-s02-1`, once the load was back under 10. Same binary
    (`7a6d238`), dataset, flags and 2 s settle. No other CPU-heavy work runs beside cohorts.
19. **Activation-weight ablation: starts and the confirm budget, 2026-11-19.** The labelled
    ablation (`rerank`, `activationWeightScale` 0.25 and 0, natural-names dataset, 543 questions, 3
    replicates each on fresh orgs, 2 s settle, `--skip-user-scope`, top-k 3) started three times.
    Start 1 (16:00Z, cognoscenti `7815bbe`) was stopped at about 16:03Z when the adapter flaw in
    deviation 17 was found; its files are under `ablation/stopped-old-adapter/`. Start 2 (16:03Z,
    `4240735`, 120 s confirm budget) stopped at 17:13Z, about 70 minutes into replicate 0, on
    `settle(): ... not confirmed written within 120000ms`; the achiral.ai candidate writer had
    stalled for 221 s (its usual gap is under 30 s), overlapping conversation-summarization jobs
    that were calling the model; the files are under `ablation/crashed-2/` and none is reported. Start
    3 (17:17Z, `4240735`) differs only in `COGNOSCENTI_ACHIRAL_CONFIRM_MS=600000`, which only changes
    how long a write may stay unconfirmed before the run stops; it changes no query, dataset,
    metric or scoring rule. The cause of the writer stall is not established.
20. **The agent-asker merge becomes a rank-by-rank interleave, 2026-11-20.** Deviation 17 merged
    the agent route and the org route by score. The control cohort of the natural-names dataset
    (shipped weight, `4240735`, `ablation/natural-control.results.json`) then scored 0.00 on
    `individual` and on `owner`, against 0.25 and 0.51 with the agent route alone, and overall recall
    fell from 0.270 to 0.221. The scored `individual` (40) and `owner` (15) questions are all asked by
    agents, and their gold is a note private to that agent; the dense org pool outscored it and
    pushed it out of the top 3. The two routes' scores are not on one scale, so a merge by score
    was the wrong rule. From this change an agent's question takes agent #1, org #1, agent #2,
    org #2, and so on until top-k, skipping an id already taken, and each item keeps its own
    route's score. The user-asked path is unchanged, and leak accounting runs on the interleaved
    list. `natural-control` and the activation-weight ablation cohorts (item 19) ran on the
    score-merge adapter (`4240735`); they are labelled as such, reported as run, and not pooled with
    runs on this change. Earlier runs on the agent route alone, and runs on the score merge, are not
    comparable with later ones on the `agent`, `individual` and `owner` categories.
21. **Natural-names cohorts on the interleaved adapter, started 2026-11-21T18:53:08Z.** The
    natural-names dataset (543 kept questions, sha256 `b3e7ba72…881c`, `--skip-user-scope`, top-k 3,
    seed 0) runs as two labelled cohorts, `interleave activationNoiseScale=0` and
    `interleave activationNoiseScale=0.2`, 5 replicates each, on fresh orgs `ct9-i0-1` to `-5` and
    `ct9-i02-1` to `-5` (`rerank`, agents `report-agent`, `scribe-agent`, `triage-agent` and
    `ops-agent`), with Cognoscenti `3a76bf1` (deviation 20: an agent's question interleaves the
    agent and org routes rank by rank), 2 s blind settle, 10 min confirm budget
    (`COGNOSCENTI_ACHIRAL_CONFIRM_MS=600000`), no retrieval-mode or params headers, per-replicate log,
    results JSON. The achiral.ai build under test is `feat/benchmarks` at `42326dc8` (includes main's
    PR #228 squash), working tree clean; its later commit `e9a6eb65` is not in the running API, on
    purpose. The earlier natural-names cohorts (score-merge `4240735`, `natural-control` and the
    ablation) and the registered-dataset cohorts are reported as run and are never pooled with these.
22. **Plan 40 B2 cohorts on the natural-names dataset, run 2026-11-22 (recorded after the fact).**
    Two labelled cohorts, `b2 range=0.25` and `b2 range=1`, sent `x-achiral-retrieval-params`
    `{"activationRangeWeight":W}` (deviation 14), 5 replicates, seed 0, `--skip-user-scope`, top-k 3,
    2 s settle, 10 min confirm budget, Cognoscenti `3a76bf1`, achiral.ai `e9a6eb65`, fresh orgs
    `ct9-b2*`, no retrieval-mode header, no noise. A first launch died 21 s in on `agent_not_found`
    (the orgs lacked the four memory agents) and was discarded (`b2/failed-launch/`); the orgs were
    re-provisioned with the agents and the cohorts relaunched. Results: 0.5562 (95% CI 0.5157-0.5985)
    at 0.25 and 0.5285 (0.4880-0.5727) at 1, 0 leaks. Reported beside, not pooled with, the
    interleave cohorts of item 21 (which ran without the header and on `42326dc8`).
23. **B2 range 0.25 on the registered stream, announced 2026-11-22 before it runs.** One labelled cohort,
    `registered b2 range=0.25`, on the registered dataset (sha256 `2fc8d6c2…93e2`, 544 kept questions,
    `--skip-user-scope`, top-k 3, seed 0), 5 replicates, fresh orgs `ct10-b2reg-1` to `-5` (`rerank`,
    noise 0, the four memory agents), header `{"activationRangeWeight":0.25}` (deviation 14), 15 s
    settle as the registered cohorts, 10 min confirm budget, no retrieval-mode header. It differs from
    the registered cohorts (item 11 to 16) in three ways, all stated: the adapter is `3a76bf1`
    (deviation 20 interleave, not `7a6d238`), the achiral.ai build is `feat/benchmarks` at `15db2d24`
    (adds `e9a6eb65`, the Plan 44 productions behind a flag that is off, and the reinforce fix), and
    the params header is sent. It is reported beside the registered cohorts (0.559 at `s = 0`),
    never pooled with them, and does not replace them. No achiral.ai source is edited while it runs.
24. **Interleave-only control on the registered stream, announced 2026-11-23 before it runs.** One
    labelled cohort, `registered interleave`, identical to deviation 23 (registered dataset, 544 kept
    questions, `--skip-user-scope`, top-k 3, seed 0, 5 replicates, 15 s settle, 10 min confirm budget,
    Cognoscenti `3a76bf1`, achiral.ai `15db2d24`, fresh orgs `ct10-ilreg-1` to `-5` with `rerank`, noise 0
    and the four memory agents) except that no params header is sent. Its purpose is attribution: deviation
    23 changed the adapter, the build and the header together (recall 0.7353 against 0.5581 for the
    registered cohort), so this cohort separates the interleave and build from `activationRangeWeight`.
    Reported beside deviations 23 and the registered cohorts, never pooled. No achiral.ai source is edited
    while it runs.

# Memory Trustworthiness: score definition (2026-11-23)

Recorded before any run on the frozen serving checkout. Changing anything below needs a new dated deviation and a new label; the old label then stops being used.

## Definition

Memory Trustworthiness is the unweighted mean of four component rates, each in [0, 1]. Higher is better.

| Component | What it measures | Rate | Pool on the registered stream (user-scope trials not run) |
|---|---|---|---|
| Access | Private notes stay private | 1 minus the share of leak trials in which a note the asker may not see was returned in the top 3 | `scope` 71 and `agent` 70 leak trials, 141 |
| Currency | The current fact beats the superseded one | recall@3 | `supersede`, 70 questions |
| Recall | Right note found | recall@3 | `retrieve`, `paraphrase`, `rephrase`, `stable` pooled, 280 questions |
| Private memory | An agent finds its own private notes | recall@3 | `agent`, `owner`, `individual` pooled, 123 questions |

Score = (Access + Currency + Recall + Private memory) / 4. Weights are equal and fixed. No component may be dropped, re-pooled or re-weighted for a run that is labelled Memory Trustworthiness.

## Run settings

- Stream `trust-seed-0`, sha256 `2fc8d6c2...93e2`, `--skip-user-scope`, top-k 3, seed 0, 5 replicates, fresh org per replicate, `rerank`, noise 0, the four memory agents, adapter `3a76bf1` (interleave).
- A question's rate is its mean over replicates, as the harness reports it. Leak trials are scored the way the harness scores them.
- Interval: 95% percentile bootstrap. Resample questions (leak trials, for Access) with replacement within each component, recompute the four rates, take the mean; 10,000 draws. Report the 2.5th and 97.5th percentiles.
- Baselines (`verbatim`, `bm25`, `recency`, `full-context`, `bounded-window`) are scored on the same stream with the same formula, and shown beside Achiral wherever the score is.

## Disclosure

- This definition was set after the registered B2 run and its interleave-only control had been read at component level (B2 0.25: Access 1.00, Currency 0.86, Recall 0.88; control: 1.00, 0.49, 0.66; Private memory about 0.18 for B2). Those two runs are not confirmatory for the score. The first confirmatory run is the registered run on the frozen serving checkout, still to be launched.
- The score covers organization and agent scope only. User-scope trials (316) are not run (Plan 41) and are not in the score.
- Recall alone does not separate Achiral from `verbatim` (0.90) and `bm25` (0.87). Wherever the score is shown, its four components, their sizes and intervals are shown with it.
- Track C (provenance, lineage, leaver) is not part of this score. It can join later as a fifth component or a separate team sub-score through a new dated deviation.

## Display rule

Any page, README or product surface that shows the score also shows: the four components with n and 95% intervals, the baselines scored the same way, the run date, the serving commit and the stream hash. A score without these is not published.

## Result note: arm b2def (added 2026-11-24, after the run)

First confirmatory run under this definition. No setting in this file changed after it started.

- Run: 2026-11-23T19:41Z to 2026-11-24T03:47Z. Serving commit `5a11cad76aecc4badcb8a7de5bff86c24fec6da4` (B2 default, no retrieval header), container `bench-api`, `RestartCount` 0 and the same `StartedAt` at the end, serving checkout unchanged. Binary `cognoscenti-3a76bf1` (sha256 `b4da5ba6...`); the manifest's `cognoscenti_commit` (`75a340d3`) is the harness checkout's HEAD at launch, not the binary's build. Stream `trust-seed-0`, sha256 `2fc8d6c2...93e2`, 5 replicates, seed 0, top-k 3, `--skip-user-scope`. Exit 0.
- Score: 0.80 (95% interval 0.77 to 0.83).

| Component | n | Achiral | 95% interval |
|---|---|---|---|
| Access | 141 | 1.00 | 0 leaks; the bootstrap is degenerate, so the leak rate is below 2.1% (3 / 141) |
| Currency | 70 | 0.86 | 0.77 to 0.93 |
| Recall | 280 | 0.88 | 0.84 to 0.91 |
| Private memory | 123 | 0.46 | 0.37 to 0.55 |

- Private memory by category: agent 0.31 (70), owner 1.00 (18), individual 0.49 (35, the agent-scope half; the user-scope half is not run). The earlier B2 run, which had the 0.25 band only through a header, scored 0.20 on this component (agent 0.31, owner 0.06, individual 0.03).
- Baselines on the same stream and formula, from a 3-replicate run (Achiral used 5): `bm25` 0.64 (Access 0.12), `verbatim` 0.48 (Access 0.60), `recency` 0.50 (Access 0.00), `full-context` and `bounded-window` 0.00. A baseline that returns everything scores high on Private memory only because it leaks; Access is the component that shows it.
- Not yet done: the baselines have not been rerun at 5 replicates. Arm b3kw (keyword candidates in rerank) is running and is reported separately.

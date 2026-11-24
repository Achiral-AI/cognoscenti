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

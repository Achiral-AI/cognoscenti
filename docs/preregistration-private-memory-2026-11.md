# Preregistration: the private-memory fixes on a held-out stream (Plan 45)

Written 2026-11-25, before any arm in this file has started. Status: draft until
the first arm launches; it is then frozen and changes are dated deviations at
the end.

## Question

Memory Trustworthiness for Achiral was 0.88 (95% interval 0.85 to 0.90) on the
registered stream `trust-seed-0` (arm b3kw). Private memory, the weakest
component, was 0.61 (0.52 to 0.69). Three changes were built after that result
and have not been measured:

1. `achiral.ai` `f372db4c` (merged as `0250eebb`): agent notes take an event time
   and a `supersedes` link, and agent recall drops a replaced note and refills
   the slot.
2. Adapter opt-in `COGNOSCENTI_ACHIRAL_AGENT_WRITE_TIME=1` (cognoscenti `1dd2c43`):
   sends `occurredAt` and `supersedes` for agent notes.
3. Adapter opt-in `COGNOSCENTI_ACHIRAL_ORG_ROUTE_FIRST=1` (cognoscenti `1dd2c43`):
   queries the organization route before the agent route.

Failures on `trust-seed-0` shaped all three, so that stream cannot confirm them.
The `keywordCandidates` setting behind 0.88 was also chosen after arm b2def on
that stream. This run asks two questions on a stream nobody tuned against:
does 0.88 hold, and do the fixes help.

## Stream

`datasets/generated/trust-seed-1/stream.jsonl`, generated with
`cognoscenti-3a76bf1 --generate-stream ... --seed 1` on 2026-11-25. sha256
`773b1896a15a067e171e4551b0a040e9cfe940f20d72677f6799497230642985`. 2,978 events,
860 questions, lint clean. With `--skip-user-scope`, 532 questions are kept and
328 trials are not run (scope 232, individual 42, owner 54). Component sizes:
Access 138, Currency 70, Recall 280, Private memory 114.

`trust-seed-2` (sha256 `4ca2f2e44f0f41a82bf4366ede2b1fcd9215a4f06607050338265aabddbaaa38`)
is reserved and not run. It is used only if a later change is tuned on seed 1.

"Held-out" means unseen questions from the same generator. It is not a
different distribution and not real team data.

## Arms

All arms: stream `trust-seed-1`, 5 replicates, seed 0, top-3, `--skip-user-scope`,
15 s settle, 10 min confirm budget, five fresh organizations per arm (`rerank`,
noise 0, the four memory agents), `keywordCandidates` on. Run one after another
against one frozen serving container. A restart of the container voids the arm.

| Arm | Adapter setting |
| --- | --- |
| P0 control | no opt-ins |
| P2 treatment | `AGENT_WRITE_TIME=1` and `ORG_ROUTE_FIRST=1` |
| P1 attribution | `AGENT_WRITE_TIME=1` only. Runs only if P2 meets H1 |

Order: P0, P2, then P1 if needed.

## Hypotheses

- **H1 (primary).** Private memory in P2 exceeds P0 by at least 0.05, and the paired
  bootstrap 95% interval (over questions matched by id, 10,000 draws) is above 0.
- **H2 (safety).** No private note is returned to the wrong actor in any of the 138
  scope and agent trials of P0 or P2, in any replicate, reported with the 3/n bound.
- **H3 (no harm).** Recall and Currency in P2 are each no more than 0.02 below P0.
- **P0 reading (descriptive).** Memory Trustworthiness and the four components with
  intervals, beside arm b3kw. No threshold.

Computed with `docs/reports/memory-trustworthiness-2026-11/mt_compare.py`.

## Decision rules

- H1, H2 and H3 hold: report P2 as the first held-out Memory Trustworthiness number
  for the fixed system, and run P1 for attribution.
- H1 fails, H2 and H3 hold: report no effect large enough to see. Do not tune on
  seed 1.
- H2 or H3 fails: report the failure. The opt-ins stay off by default.
- A change made after seeing seed 1 results is measured on seed 2, never again on
  seed 1.

## Baselines

The five local baselines on this stream, 5 replicates, same binary (`3a76bf1`):

| System | Memory Trustworthiness | Access | Currency | Recall | Private memory |
| --- | --- | --- | --- | --- | --- |
| bm25 | 0.67 (0.64 to 0.70) | 0.07 | 0.77 | 0.87 | 0.96 |
| recency | 0.50 | 0.00 | 1.00 | 0.00 | 1.00 |
| verbatim | 0.48 (0.45 to 0.51) | 0.61 | 0.03 | 0.89 | 0.39 |
| full-context, bounded-window | 0.00 | 0.00 | 0.01 | 0.00 | 0.00 |

bm25 scores higher here than on seed 0 (0.63) because Currency is 0.77 instead of
0.60. Any comparison to bm25 uses the seed-1 figure.

## Limits

- Per-member (user-scope) trials are not run.
- One generator and one seed family. No real data.
- A paired effect under about 0.05 will not be detected with about 114 Private memory
  trials.
- Calibration is derived from ranking.
- The host may sleep; sleep periods from `pmset -g log` are reported with each arm.

## Provenance to record at launch

Serving commit and container (name, `StartedAt`, `RestartCount`), adapter binary
sha256, stream hash, keys file name (never its contents), start and end times.

## Deviations

None yet.

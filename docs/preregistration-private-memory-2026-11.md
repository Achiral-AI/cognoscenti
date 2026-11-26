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

1. 2026-11-25. First P2 attempt (organizations `pm-p2-1` to `pm-p2-5`, started 18:02:04Z)
   aborted at 18:31:56Z in replicate 0: `settle()` reported 5 events not confirmed
   written within 600000 ms, exit 101. No P2 result was produced and none was read.
   Cause not established. The container was unchanged (`RestartCount` 0). P2 was
   rerun from scratch on five fresh organizations (`pm-p2b-1` to `pm-p2b-5`, started
   22:49:03Z) on the same container, with the same settings. Only the rerun is
   reported. If it fails the same way, that is reported as a P2 failure.

## Result: arm P0 (control), 2026-11-25

Run 2026-11-25 10:18:04Z to 18:00:27Z. Serving commit `6ad36ac9`
(`6ad36ac999fd75873b024e755dce52f87f9d1dbf`), container `bench-api-pm`
`StartedAt` 2026-11-25T10:17:24.935669218Z, `RestartCount` 0 at launch and at end.
Adapter binary `cognoscenti-1dd2c43` (sha256 `80218e07094c5b64`), stream sha256
`773b1896…2985`, five organizations `pm-p0-1` to `pm-p0-5`, keys file `keys-p0`.
The harness exited 0. `pmset -g log` shows no sleep or wake event in the run window
(`caffeinate -i -s -m` held the host awake), so the run took 7 h 42 min of wall time.

| Component | n | P0, `trust-seed-1` | b3kw, `trust-seed-0` |
| --- | --- | --- | --- |
| Memory Trustworthiness | | 0.85 (0.82 to 0.87) | 0.88 (0.85 to 0.90) |
| Access | 138 | 1.00 (0 leaks; 3/n bound 2.2%) | 1.00 |
| Currency | 70 | 0.93 (0.86 to 0.99) | 1.00 |
| Recall | 280 | 0.90 (0.86 to 0.93) | 0.90 (0.86 to 0.93) |
| Private memory | 114 | 0.56 (0.47 to 0.65) | 0.61 (0.52 to 0.69) |

Mean recall@3 by replicate: 0.8402, 0.8609, 0.8609, 0.8515, 0.8534. No trial with a
leak in any replicate (0/138 each).

Reading, descriptive only. The score on an unseen stream is 0.03 lower than on the
stream the setting was chosen against, and the two intervals overlap. Recall matches.
Currency is the largest drop (1.00 to 0.93), which fits `keywordCandidates` having
been chosen after seeing seed 0. On this stream the five-replicate bm25 baseline is
0.67, so the gap to the best local baseline is about 0.18, against 0.25 on seed 0.
P2 has not finished; H1 to H3 are not evaluated.

## Result: arm P2 (treatment), 2026-11-26

Run 2026-11-25 22:49:03Z to 2026-11-26 09:53:01Z (attempt 2; see Deviation 1).
Same serving commit `6ad36ac9`, same container (`StartedAt` 2026-11-25T10:17:24.935669218Z,
`RestartCount` 0 at launch and at end), same binary and stream as P0. Organizations
`pm-p2b-1` to `pm-p2b-5`, keys file `keys-p2b`. Settings: `AGENT_WRITE_TIME=1`,
`ORG_ROUTE_FIRST=1`. The harness exited 0.

Sleep: `pmset -g log` shows the host in system sleep (lid closed, on battery) from
2026-11-26 01:20:47Z to 01:31:15Z and from 01:39:13Z to 04:04:21Z, about 2 h 35 min
in all, inside replicate 1. The harness survived and no settle timeout fired, so
the run took 11 h 4 min of wall time, about 8 h 30 min awake. The sleep did not
change the stream or the serving container. A Mongo election on the host at about
04:49Z (reported by another session) fell inside replicate 1 as well and caused no
failure here.

| Component | n | P0 | P2 | Paired P2 − P0 (95% interval) |
| --- | --- | --- | --- | --- |
| Memory Trustworthiness | | 0.85 (0.82 to 0.87) | 0.91 (0.89 to 0.93) | |
| Access | 138 | 1.00 | 1.00 | 0 |
| Currency | 70 | 0.93 | 1.00 | +0.07 (+0.01 to +0.14) |
| Recall | 280 | 0.90 | 0.89 | −0.01 (−0.03 to 0.00) |
| Private memory | 114 | 0.56 (0.47 to 0.65) | 0.75 (0.68 to 0.83) | +0.19 (+0.11 to +0.28) |

Mean recall@3 by replicate: 0.8872, 0.8891, 0.8891, 0.8910, 0.8910. No leak in any
replicate in either arm: scope 0/68 and agent 0/70 events, 3/n bound 4.4% and 4.3%.

- **H1 met.** Private memory rose by 0.19, above the 0.05 threshold, with an
  interval above 0.
- **H2 holds.** No leaks in P0 or P2.
- **H3 holds.** Recall fell by 0.01 and Currency rose; neither fell more than 0.02.

Decision rule: report P2 as the first held-out Memory Trustworthiness number for the
fixed system (0.91, 0.89 to 0.93) and run P1 for attribution. P1 has not been run.

Limits: held-out means unseen questions from the same generator; one seed family;
user-scope trials not run; the P2 result pairs two arms that differ in two opt-ins,
so P1 is needed before crediting either one. Two opt-ins that are off by default
produced this score; the shipped default is P0 (0.85).

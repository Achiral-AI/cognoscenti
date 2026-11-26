# Preregistration: the shipped default configuration on a fresh stream (Plan 45 follow-up)

Written 2026-11-26, before the arm in this file has started. Frozen when it
launches; later changes are dated deviations at the end.

## Question

On the held-out stream `trust-seed-1`, three settings raised Memory
Trustworthiness from 0.85 (0.82 to 0.87, arm P0) to 0.91 (0.89 to 0.93, arm P2),
with Private memory up 0.19 (paired interval 0.11 to 0.28) and no leaks. All
three were opt-ins. They are now the defaults:

1. `achiral.ai` `5a225eb9` (branch `feat/retrieval-defaults`, off `6ad36ac9`, not
   pushed or merged): `keywordCandidates` is true unless a request sets it to false.
   This changes the retrieval call for every `rerank` search, not only the
   benchmark's organization route.
2. Cognoscenti `cf37ceb`: the adapter sends agent write time and supersession
   unless `COGNOSCENTI_ACHIRAL_AGENT_WRITE_TIME=0`.
3. Cognoscenti `cf37ceb`: the adapter queries the organization route first for an
   agent asker unless `COGNOSCENTI_ACHIRAL_ORG_ROUTE_FIRST=0`.

Does the plain default configuration, with nothing set, score as P2 did on a stream
nobody has looked at? Seed 1 informed the decision to ship these, so it cannot
answer that.

## Stream

`datasets/generated/trust-seed-2/stream.jsonl`, generated with
`cognoscenti-3a76bf1 --generate-stream ... --seed 2` on 2026-11-26. sha256
`4ca2f2e44f0f41a82bf4366ede2b1fcd9215a4f06607050338265aabddbaaa38` (reserved in
the Plan 45 preregistration before any arm ran). 2,976 events, 860 questions, lint
clean. With `--skip-user-scope`, 541 questions are kept and 319 trials are not run
(scope 229, individual 36, owner 54). Component sizes: Access 141, Currency 70,
Recall 280, Private memory 120.

## Arm

One arm, `default`. Stream `trust-seed-2`, 5 replicates, seed 0, top-3,
`--skip-user-scope`, 15 s settle, 10 min confirm budget, five fresh organizations
(`rerank`, noise 0, the four memory agents). No `COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS`,
no adapter opt-ins. Run script `run-default.sh`.

Frozen serving: container `bench-api-dflt` on `127.0.0.1:7095`, image
`achiralai/achiral-ai:dev-api`, worktree `tmp/achiral.ai-default` at `5a225eb9`.
Adapter binary `cognoscenti-cf37ceb` (sha256 `ad1dc1584bdafb62…`). A restart of the
container voids the arm. The server's echoed retrieval object is recorded to show
that keyword candidates were on.

## Hypotheses

- **H1 (primary).** Memory Trustworthiness point estimate is at least 0.88, the
  number published for the unfixed system.
- **H2 (safety).** No private note is returned to the wrong actor in any of the 141
  scope and agent trials, in any replicate, reported with the 3/n bound.
- **H3.** Private memory point estimate is above 0.56, the held-out value without
  the fixes (P0).

Descriptive, no threshold: the four components with intervals, beside the baselines
below.

## Decision rules

- H1, H2 and H3 hold: report the default configuration as the current Memory
  Trustworthiness number, with seed 1 results as supporting evidence. Public pages
  change only after wording is agreed in chat.
- H1 or H3 fails: report the failure and the interval. Do not tune on seed 2.
- H2 fails: report the failure and treat the default as unsafe to ship.
- A change made after seeing seed 2 results is measured on `trust-seed-3`, never
  again on seed 2.

## Baselines

Five local baselines on this stream, 5 replicates, binary `3a76bf1`:

| System | Memory Trustworthiness | Access | Currency | Recall | Private memory |
| --- | --- | --- | --- | --- | --- |
| bm25 | 0.66 (0.63 to 0.69) | 0.11 | 0.71 | 0.89 | 0.93 |
| recency | 0.50 | 0.00 | 1.00 | 0.00 | 1.00 |
| verbatim | 0.46 (0.43 to 0.49) | 0.53 | 0.00 | 0.91 | 0.40 |
| full-context, bounded-window | 0.00 | 0.00 | 0.00 | 0.00 | 0.00 |

## Limits

- Per-member (user-scope) trials are not run.
- One generator, and three seeds from it. No real data.
- Unseen questions from the same generator are not a different distribution.
- The default also changes retrieval for collections the benchmark does not
  exercise (for example domain knowledge). This arm does not measure them.
- The host may sleep; sleep periods from `pmset -g log` are reported with the arm.
- A running P1 arm shares the host; its load can slow this arm but not change what
  it scores.

## Provenance to record at launch

Serving commit and container (name, `StartedAt`, `RestartCount`), adapter binary
sha256, stream hash, keys file name (never its contents), start and end times.

## Deviations

None yet.

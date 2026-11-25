# Track C, Team Memory: first Achiral run (2026-11-25)

**Status: unregistered, unreviewed.** The pre-registration
(`preregistration-track-c-2026-11.md`) is still a draft with open questions, none of the 64
items has been reviewed, and the `recency` control arm was not run. Nothing below is a claim
under that document's decision rule. These are the numbers, with n and intervals, for the
first batch only.

## What ran

| | |
|---|---|
| Stream | `datasets/generated/team-seed-0/stream.jsonl`, sha256 `234b7205...9749`, 64 items, 199 events (191 org, 8 user scope), 20 actors, seed 0 |
| Arms | `achiral` (org setting `rerank`, AGGR off, activation noise 0.25), `bm25`, `verbatim`; top-k 5 |
| Binary | `results/track-c-2026-11/cognoscenti-c7716cd`, sha256 `f51744a871ee792b75af0033063cd3ddeae951d7a69eb4597abe1f4ae56f1d6d`, built from `feat/team-track-c-run` (`97ad7ec` + `e442bdb` precision@k merge + `c7716cd` adapter change); `cargo test` passed (442 unit tests and the integration tests) before the build. All three arms used this one binary |
| Serving | achiral.ai `c81b488139a85e9914b313dc0ac783f2146a5db5` (`origin/feat/benchmarks`), clean detached worktree `tmp/achiral.ai-serve-tc`, container `bench-api-tc` on 127.0.0.1:7094, image `achiralai/achiral-ai:dev-api`, no `--watch`. StartedAt `2026-11-25T20:27:39Z`, RestartCount 0 at start and at the end of every run; the checkout was unchanged after each run |
| Orgs | Six fresh orgs from `scripts/provision-cognoscenti-benchmark-org.js` (`tc-team-2026-11-25-r0..r5`), mode `rerank`, noise 0.25 (the shipped value; Track A's earlier runs used 0). Six leaver members per org plus a compliance key bound to each. Keys are in a private folder outside the repo |

## Runs

1. `achiral-5rep`: Achiral, 5 replicates, orgs r0-r4 (`achiral-5rep.*`). All five replicates finished.
   The process then stopped on the `bm25` system because one `--replicate-log` holds one
   system, so no results file was written for this run; its printed report (replicate 0) and
   the replicate log survive. All five replicates recalled 47/64 questions fully with a
   mean recall@5 of 0.8187 and no leak; 0/64 questions flipped (3/n bound 0.047).
2. `achiral-1rep`: Achiral, one replicate on a fresh org (r5), written to results JSON, CSV
   and the report. It reproduces replicate 0 of run 1 on every printed number. Its cohort label
   was renamed in the JSON from `track-c team-seed-0` to `track-c team-seed-0 achiral` so the
   report can hold three cohorts; nothing else was edited.
3. `bm25` and `verbatim`: 5 replicates each, offline. All replicates are identical (the
   baselines are deterministic).

Team metrics and precision@k are scored on replicate 0 only, as the replay does for every
system; replicates 1-4 record pass or fail per question. Report: `report/index.html`,
`results.json`, `results.csv`.

## Results (replicate 0; 95% bootstrap intervals over items; n is items)

| Metric | Achiral | bm25 | verbatim |
|---|---|---|---|
| Supersession correctness (`sup`, n=16) | **16/16 = 1.00** | 5/16 = 0.31 (0.13-0.56) | 3/16 = 0.19 (0.00-0.38) |
| Control, older fact stands (`keep`, n=8) | 7/8 = 0.88 (0.63-1.00) | 8/8 | 8/8 |
| Control, point in time (`asof`, n=8) | **0/8** (3/n bound 0.38) | 6/8 = 0.75 (0.38-1.00) | 7/8 |
| Provenance accuracy (`prov`, n=14) | 7/14 = 0.50 (0.21-0.79) | 7/14 = 0.50 (0.21-0.79) | 6/14 = 0.43 (0.14-0.71) |
| Lineage edge recall (`why`, 30 edges) | 9/30 = 0.30 (0.13-0.47) | 11/30 = 0.37 (0.20-0.53) | 8/30 = 0.27 (0.13-0.43) |
| Lineage chain complete (n=10) | 1/10 = 0.10 (0.00-0.30) | 2/10 = 0.20 (0.00-0.50) | 0/10 |
| Lineage decoy hit (n=10) | 1/10 = 0.10 (0.00-0.30) | 0/10 | 0/10 |
| Leaver recall (n=8) | 8/8 | 8/8 | 7/8 |
| Leaver leak (n=8) | **0/8** (3/n bound 0.38) | 8/8 | 8/8 |
| Leak, all 64 questions | **0/64** (3/n bound 0.047) | 19/64 = 0.30 (0.19-0.41) | 20/64 = 0.31 (0.20-0.44) |
| Precision@5 (n=64) | 0.2250 (0.1906-0.2688) | 0.2344 (0.2156-0.2594) | 0.1844 (0.1563-0.2125) |
| Mean recall@5 (n=64) | 0.8187 (0.7333-0.8958) | 0.9323 (0.8833-0.9740) | 0.7135 (0.6146-0.8125) |
| Tokens injected per question | 55.3 (53.8-57.0) | 55.9 (53.5-58.6) | 59.6 (57.1-62.2) |

Where an interval is missing, the rate had no variance (all or none) and the report gives a
3/n bound instead, or the cell is not in the report. All values are in `report/results.csv`.

Paired exact McNemar, Achiral against bm25 on supersession: Achiral is right on all 16, so the
11 items bm25 misses are all discordant, p = 2/2^11 = 0.001. The other paired tests in the
draft rule were not computed (the replay does not keep per-item team outcomes).

## What the numbers say

- Supersession: Achiral returns the current value on every item and outranks the replaced
  ones. bm25 does so on 5 of 16. Largest effect in the batch.
- Leaver privacy: no private note came back for an asker who may not see it (0/64 trials).
  The baselines ingest everything and ignore scope, so their 8/8 is by construction. This
  confirms the live path only for these 8 items.
- Point in time: 0/8, the weakest result. The serving code accepts an `asOf` field on
  retrieve (`services/memory/PublicMemoryService.js:686`) but the adapter does not send one,
  because the stream question carries the date only inside its text and the gold validity
  window, and deriving `asOf` from the gold would leak it. So the 0/8 is "no `asOf` sent",
  not a measurement of the server's point-in-time retrieval. Not yet tested.
- Provenance is level with bm25, and lineage is below it: edge recall 9/30 against 11/30, with
  overlapping intervals. Achiral returns the newest value and drops the earlier steps of a
  chain at k=5.
- Precision@5 is about 0.2 for all three arms: gold sets are 1-5 events and k is 5, so the
  ceiling is low. It is a floor on noise, not a quality score.

## Limits

- 64 items, one seed (0), templated wording, one org shape. Per-category n is 8-16, so
  intervals are wide and 0 or 8/8 tell little beyond "no failure seen".
- Draft registration; items unreviewed. The reviewer estimate is unchanged (about 1.5 hours
  for these 64, about 15 hours for 500 for one reviewer; an estimate, not measured). Nothing
  here reduces it.
- Retrieval only; no answer is graded. No latency, cost, write-path time or concurrency.
- Team metrics come from one replicate per system (see above); the five-replicate result
  covers recall and flips only.
- The write path that moves ingested events into the store is a leader-elected job. During
  these runs `bench-api-tc` was not the leader (log: "Not leader, lock held by" the
  `bench-api-pm` container), so candidates from these orgs were written by the leader's
  code (`achiral.ai-serve2`, commit `6ad36ac9`), not by `c81b4881`. The retrieval path
  under test is `c81b4881`.
- User-scope ingest needs a member-bound compliance credential on this commit
  (`middleware/complianceMemoryAccess.js`), which the adapter did not support. Commit
  `c7716cd` adds `COGNOSCENTI_ACHIRAL_USER_KEYS_FILE` (JSON array, one actor-to-key map per
  replicate). The org key is still used for every other scope and for all retrieval.
- `recency`, `full-context` and `bounded-window` were not run, so the draft rule's control
  comparison against `recency` cannot be made.
- The host carried `bench-api-pm` (a long arm) and `bench-api-ta` (another session) while this
  ran; one extra serving container was added.

## Open

- Human review of all 64 items; at least 500 reviewed items; private held-out split.
- Registration sign-off (draft open questions 1-8), including whether to run the report seed 4242.
- Send `asOf` for point-in-time questions (needs a decision on where the date comes from) and
  rerun `asof`.
- Team change, agent handoff, postmortems, commitments, procedural, contradiction, noise,
  scale scenarios; latency, cost and concurrency; the `achiral-aggr` arm.
- Whether the replay should keep per-item team outcomes for paired tests, and accept several
  systems with one replicate log.

# Pre-registration: the use-learning stream

Status: written 2026-11-21, before the generator exists and before any run on this stream.
Plan 40 Stage 5 item 1. The stream is specified in `use-learning-stream.md`. Anything changed
after the first report-seed run is recorded below as a dated deviation, not edited in place.

## Goal

Real numbers that show where the existing ACT-R memory earns its place, or that it does not.
Both results are published as measured.

## Arms

Every arm replays the same stream on fresh orgs (`rerank` mode, noise 0, a new org per replicate),
5 replicates, `--top-k 3`, 2 s blind settle, `--skip-user-scope` not needed (org scope only).

| Arm | System | Setting | Explicit use feedback |
|---|---|---|---|
| `off` | `achiral` | `x-achiral-retrieval-params` `{"activationWeightScale":0}` | `--feedback gold` |
| `shipped` | `achiral` | no header (shipped activation) | `--feedback gold` |
| `shipped-nofb` | `achiral` | no header | none (feedback-off control) |
| `b2` | `achiral` | `{"activationRangeWeight":1.0}` | `--feedback gold` |
| `bm25` | `bm25` | the local baseline | n/a |
| `usecount` | `usecount` | `bm25`'s top 20, then among those within 10% of the top bm25 score, the one used more often first | n/a |

`off` still records reinforcement but the activation term has no effect on ranking, so it is the
same store and clock with ACT-R switched out of the ranking. `shipped-nofb` isolates what explicit
feedback adds: `recall` itself still reinforces what it returns in `rerank`, so it is "no explicit
reinforce call", not "no reinforcement" (stated beside the arm in the report). `b2` needs an
achiral.ai build with `activationRangeWeight` (`feat/benchmarks` at `e9a6eb65` or later); the
adapter's echo check stops the run if the server did not use the setting. The single value 1.0 is
registered here; no other value is run on the report seed. `usecount` is a new local baseline
with no free parameter except the registered 10% window and the registered top 20.

## Seeds

- Generator seed for the tuning stream: 17. Used only to check the stream is valid (lint) and
  that the baselines behave sanely. No arm result on seed 17 may lead to a change of the design.
- Generator seed for the report stream: 4242. First generated after the generator is committed
  and frozen. Different from the tuning seed.
- Bootstrap and report seed: 0, as in the live runs (`stats.rs`).

If the generator is changed after the first run on seed 4242, the change is a registered
deviation and the report-seed runs are redone in full.

## Not tuned toward a win

Sizes, kinds, counts, the Zipf exponent, the wording count, the bins, the arms and the values
above are fixed by this document and `use-learning-stream.md`. Nothing is adjusted after seeing
any arm's result. A run is not repeated because its result was unwelcome.

## Metrics

Per question: recall@3 (binary here, one gold fact per question) and top-1 hit, tokens injected.
Each reported with n and a 95% percentile bootstrap interval over questions (`stats.rs`),
per arm, by kind, by use bin, and for original against reworded wording. A bin under 10
questions is "n too small". Flip rate across the 5 replicates with its interval. Leaks are not
measured here (no private scope in this stream).

Paired comparison. For two arms, the same question in the same replicate order is a pair. The
reported difference is the mean per-question recall difference with a bootstrap 95% interval; the
paired test is the exact McNemar test (`mcnemar_exact`) on replicate-0 outcomes (replicates 1 to
4 are reported as a check, not pooled into one test).

## The decision rule (written before any run)

Primary comparison, the only one that can support a claim of improvement: `pop` questions in the
highest use bin with n of at least 30, `shipped` against `off`. If that bin has fewer than 30
questions the primary comparison is not run and the result is reported as underpowered.

"Recall improves with use" is claimed for an arm only if the curves cross, meaning all of:

1. in the first-ask bin (use 0) the arm's recall minus `off`'s has a 95% interval that is not
   entirely above zero (so any gain is not just a different starting point);
2. in the highest use bin the arm's recall minus `off`'s is above zero with its 95% interval above
   zero and a paired exact test p below 0.05;
3. the arm's own recall in the highest bin is above its own recall in the first-ask bin, with
   non-overlapping intervals.

Otherwise no claim of improvement with use is made, and the result is published as measured,
with the curves.

The same test applies to `b2` against `off`, and separately to `shipped` and `b2` against
`usecount`. There is one primary test; the others are secondary and are reported with their
intervals, not as claims.

Harms are reported with equal weight. On `rare` (C behind a popular D) and `change` (the current
fact against the used old one), any arm whose recall minus `off`'s has a 95% interval entirely
below zero is reported as a loss from use, whatever `pop` shows. A statement that ACT-R "earns
its place" needs the `pop` gain and no such loss; with a `pop` gain and a loss the result is
stated as both. A `pop` gain only against `usecount` in the end is "no better than counting",
and is stated so.

No claim is made about ACT-R memory in general, only about this stream, org scope, and these
arms. `shipped-nofb` against `shipped` is reported as the effect of explicit feedback on top of
the automatic reinforcement of `recall`.

## Deviations

None yet.

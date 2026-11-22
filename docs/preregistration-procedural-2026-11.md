# Pre-registration: the procedural track

Status: written 2026-11-21, before any run on this stream and before the system arm exists
(no runner command, no productions adapter). Plan 44 Phase 5. The stream and its scoring are in
`procedural-track.md` and `src/procedural.rs`. Anything changed after the first report-seed run
is recorded below as a dated deviation, not edited in place.

## Goal

Find out whether a system that holds the team's procedures as productions applies them more
often than one that holds them only as memories, without applying them where they do not
belong. Both outcomes are published as measured. The result gates only the proposal to turn
`aiConfig.proceduralMemory` on for customers; it does not turn it on.

## Arms

Every arm replays the same stream on a fresh org per replicate, `rerank` mode, noise 0, 5
replicates, `--top-k 1`, 2 s blind settle, org and user scope both on.

| Arm | System | What it holds |
|---|---|---|
| `declarative` | `achiral` | each procedure event as an ordinary memory; `aiConfig.proceduralMemory` off |
| `procedural` | `achiral` | the same memories, plus each procedure authored as a production, `aiConfig.proceduralMemory` on |
| `bm25` | `bm25` | the local baseline, for a sanity check only |

**How productions are authored (fixed here).** From the stream, not by hand. For each procedure
event: `condition.text` is the situation phrase ("an annual-plan customer raises billing"),
`condition.cues` are its words longer than three letters, lowercased, `action.text` is the action
phrase, `evidence` cites the procedure event. An org-wide procedure is authored by an admin
(active at once, org audience). A replacement event is authored after the original with the same
cues, so supersession retires the original. A private procedure is authored by its member,
member-scoped. No outcomes are recorded and no utility is learned in this stream (there are no
conversations), so every production keeps its neutral utility: this track tests matching, scoping
and supersession, not Phase 3 learning.

**How the `procedural` arm ranks (fixed here).** The adapter asks the productions API's selection
(`selectProductions`: threshold 0.5, at most three, noise scale 0.15, seed = the replicate index)
for the asker, puts each selected production's first evidence event first, in selection order,
then the declarative results, without duplicates. At `--top-k 1` a question's answer is the
first of that list. A production counts as retrieving the event it cites; nothing else about it
is scored.

The constants above are the ones in `services/procedural/Production.js` at the commit recorded
with the first run. They are not changed during the run.

## Seeds

- Tuning stream, seed 17: used only to check the stream is valid and that the local `bm25`
  baseline behaves sanely. No result on seed 17 from an Achiral arm may change the design.
- Report streams, seeds 4242 and 4243. Generated after the generator is committed and frozen;
  the commit is recorded with the first run. The two are independent streams, so their
  questions are pooled for the paired tests below (replicate 0 of each).
- Bootstrap and report seed: 0.

If the generator changes after the first run on a report seed, the change is a registered
deviation and the report-seed runs are redone in full.

## Not tuned toward a win

Sizes, the situation vocabulary, wording variants, thresholds, arms and the rules below are
fixed by this document. Nothing is adjusted after seeing any arm's result. A run is not repeated
because its result was unwelcome.

## Metrics

Per arm: adherence, near-miss firing, superseded-rule rate, scope leaks, tokens per question
(definitions in `procedural-track.md`). Each rate with n and a 95% percentile bootstrap interval
over questions (`stats.rs`); a rate under 10 questions is "n too small". Flip rate over the 5
replicates with its interval. Paired comparison: the same question in the same replicate is a
pair; the difference `procedural` minus `declarative` is reported with a bootstrap 95% interval,
and the paired test is the exact McNemar test on replicate 0 of each report seed, pooled
(replicates 1 to 4 are a check, not pooled).

## The decision rule (written before any run)

"Procedural memory beats declarative memory alone" is claimed only if all of these hold:

1. Adherence: `procedural` minus `declarative` is above zero with its 95% interval above zero
   and an exact McNemar p below 0.05 (n = 120 follow pairs).
2. Near-miss firing: the upper end of the 95% interval of `procedural` minus `declarative` is at
   or below +0.10 (n = 80 pairs). The margin is registered here. A difference that is not
   distinguishable from zero but whose interval is wider than the margin fails this condition
   and is reported as "not shown to hold".
3. Superseded-rule rate: the 95% interval of `procedural` minus `declarative` is not entirely
   above zero.
4. Scope leaks: zero in the `procedural` arm across all replicates and both seeds. One leak
   fails the claim whatever the rest shows. Zero observed leaks over 40 trials support "under
   0.075 at 95% (3/n)", not "zero"; the invariant itself is proven by the unit tests in
   `ProductionStore.test.js` and `Production.test.js`, and this run only checks it on the live
   path.

If any condition fails, no claim is made and the result is published with every number. Tokens
per question is reported for both arms and is not part of the rule; no claim of efficiency is
made from it.

Equal weight for harms: if `procedural` is worse than `declarative` on near-miss firing or
superseded-rule rate with an interval entirely on the wrong side of zero, that is stated next to
any adherence gain.

## Limits, stated before the run

- The stream varies the framing of a question, not the words naming the situation: every
  wording of a situation contains the same qualifier and topic. Adherence here measures
  matching with the situation words present. It makes no claim about situations described in
  other words.
- Near-miss situations share the qualifier or the topic with a procedure, so they test partial
  cue overlap, which the matcher grades (a half overlap scores 0.5 against a threshold of 0.5).
  This was not simulated before registering. A high near-miss firing rate is an acceptable
  result; it is published, and the threshold is not adjusted.
- Retrieval only. No model answers and no answer is graded.
- Org and member scope, one organisation shape, one seed pair.
- Utility learning, compilation and induction (Phases 3 and 4) are not exercised.

## Before the first run

Nothing is run until each of these exists and is committed: a command that generates the
stream; a replay and scoring command for it; the productions adapter described above; the
report section. This document is not edited when they land. A difference between what they do
and what is written here is a deviation.

## Deviations

- 2026-11-21, before any run: `scoring-standard.md` was written to state what counts as
  retrieving an event for a memory, a chunk and a production. It adds no metric and changes no
  threshold or rule above. It records that tokens per question, as the replay counts them today,
  use the cited event's full content and so are not comparable between `declarative` and
  `procedural` until the adapter reports the injected text. Tokens are not in the decision rule,
  so no claim here depends on them.
- 2026-11-22, before any run: the adapter contract gained `retrieve_items`
  (`RetrievedItem` with `sources` and `text`) and the replay resolves items into distinct events
  and injected tokens (`scoring-standard.md`). For an adapter that reports no text, which is every
  adapter today, results are unchanged. The `procedural` arm's adapter is to report each
  production's evidence events as `sources` and its rendered condition and action as `text`, so
  tokens per question for the two arms will be comparable once it exists. This changes no rule
  above.

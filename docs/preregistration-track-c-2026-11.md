# Pre-registration: Track C, Team Memory

**Status: DRAFT, written 2026-11-23. Not registered.** Nothing here binds a run until Marvin
signs off the open questions below and the document is committed with the frozen generator
commit. No Achiral run has been made on any item. Plan 40 Stage 4. Code: `src/team/`, tooling:
`examples/team_track.rs`. Offline only; nothing in this track contacts a deployment.

## Goal

Measure four things a team buyer asks, with gold evidence and no model in the loop:

1. Who can see what: does a leaver's private note stay private while their org-visible decision
   stays findable?
2. Which fact is current: after a value changes, does the new one outrank the old, and does the
   system still return the old one when the question is about the past?
3. Where did a fact come from: does the originating statement outrank its restatements?
4. Why was a decision made: are the events behind it, and the edges between them, returned?

## Datasets

One item is one question about one project, with its own events (unique project name per
item), so a reviewer can keep, change or drop an item without touching another. The generator
is seeded and uses no model, so there is no generating-model family to separate from the
reader, judge or vendors' extraction models. Wording comes from templates (5 statement, 4
revision, 4 restatement and 5 question wordings; 12 attributes; 900 project names), so items
are not look-alike but are plainly templated. See open question 2.

| Scenario (id code) | Question it answers | First batch | Full (500) | Gold | Hard negatives |
|---|---|---:|---:|---|---|
| `sup` | which fact is current | 16 | 120 | newest of a 2-4 link chain | the replaced values |
| `keep` (control) | same, newest-wins trap | 8 | 60 | an older fact nothing replaced | later events: same project other attribute, other project same attribute |
| `asof` (control) | same, point in time | 8 | 60 | the value valid on the date asked, with a validity window | its replacement |
| `prov` | where it came from | 14 | 110 | the first statement, in a source of record | 1-2 restatements by others in chat or email |
| `why` | why a decision was made | 10 | 90 | 3 events (5 when later reversed) and 2 or 4 gold edges | another project's decision on the same problem |
| `leave` | who can see what | 8 | 60 | the leaver's org-visible decision | none; any event the asker may not see is a leak |

Not built: team change (a member moves between teams) needs a team scope, and the stream
schema has only org, user and agent scope. Also not built: agent handoff, incident
postmortems, customer commitments, procedural next action, contradiction detection, forgetting
and noise, the `atlas-03` fan shape, and the scale scenarios (5-50 people, thousands to
millions of events).

Files, in `datasets/generated/team-seed-0/` (first batch, seed 0, 64 items, 199 events):
`stream.jsonl` (passes `--lint-stream`), `items.jsonl` (the gold the scorer reads),
`review.csv` (what reviewers open), `CHECKSUMS.txt` (sha256 of each). Regenerate with
`cargo run --example team_track -- generate --seed 0 --size first --out-dir <dir>`; the
checksums must match.

Seeds, once registered: tuning seed 17 (checks that the stream is valid and the baselines
behave; no Achiral result on it may change the design); public report seed 4242; a private
held-out seed that is never committed, generated at `--size full` and held by whoever owns the
held-out split (open question 5). Bootstrap seed 0. A generator change after the first report-seed run is
a registered deviation and the runs are redone.

Precision@k is a descriptive retrieval metric. Count distinct gold evidence IDs in the first k returned positions and divide by k, including when fewer than k are returned. Report the mean across scored questions and bootstrap over those questions. An empty response scores zero. A skipped question is excluded from n. The registered decision rule remains a draft.

## Arms

Same stream, a fresh org per replicate, 5 replicates, top-k 5 (open question 4), org and user
scope both on.

| Arm | What |
|---|---|
| `achiral-rerank` | Achiral, `rerank`, AGGR off: the "before" |
| `achiral-aggr` | Achiral, `aggr`, the "after", only when Stage 5's pairing runs |
| `verbatim`, `bm25`, `recency`, `full-context`, `bounded-window` | local baselines, same streams |

`recency` is the control for the controls: it returns the newest event, so it must score
about 100% on `sup`, 0% on `keep` and `asof`, and 0% on `prov`. The unit tests pin that (and
the opposite for an oldest-first retriever); a scorer that did not show it would be wrong.

## Metrics

All are retrieval-level, scored from event ids and ranks by `src/team/score.rs`.

- **Supersession correctness** (`sup`): the gold event is returned and outranks every replaced
  value that was also returned. A missing gold event is wrong.
- **Hard-negative control accuracy** (`keep`, `asof`, reported separately and pooled): the same
  rule on the controls. A supersession score is read only next to this one.
- **Provenance accuracy** (`prov`): the originating event is returned and outranks every
  restatement. This checks which event comes back, not that a reader names the right person; the
  gold answer ("ada in contract") is recorded for review and for a later reader-graded run.
- **Lineage edge recall** (`why`): an edge counts when both its events are returned. Also the
  share of items whose whole chain and every edge came back, and the decoy hit rate (the other
  project's decision returned).
- **Leaver recall and leaks** (`leave`): the org-visible decision is returned; any returned
  event the asker may not see (`is_authorized`) is a leak. Leaks are reported as a rate with the
  3/n bound when zero.
- Tokens injected per question, from the existing replay. `missing` counts items with no result
  so a run cannot look better by skipping items.

Each rate: n and a 95% percentile bootstrap interval over items (`stats.rs`); fewer than 10
items is "n too small". Paired comparisons use the same item in the same replicate; exact
McNemar on replicate 0, replicates 1-4 as a check. Flip rate over replicates as in Track A. Not
in this draft: precision@k, distractor activation rate, latency, cost, write-path time,
accuracy-vs-cost Pareto and concurrency (Stage 4 item 2); they need the replay to record them
(open question 7).

## Decision rule (draft; numbers need sign-off before any run)

A claim is made only when every condition for it holds, on the report seed, with the intervals
above. Otherwise the result is published with every number and no claim.

1. **"Tracks current truth".** Control accuracy beats `recency` and supersession correctness
   beats `bm25`, each by paired exact McNemar p below 0.05. Both rates are reported together
   with their intervals, so neither can carry the other.
2. **"Traces provenance".** Provenance accuracy beats the better of `bm25` and `recency` by
   paired exact McNemar p below 0.05.
3. **"Explains decisions".** Per-item edge recall beats `bm25` by a paired bootstrap interval
   above zero. Chain completeness is reported, not claimed on.
4. **"Respects who can see what" (leaver only).** Zero leaks, stated as "under 3/n at 95%"
   (n = 60 leaver trials in the full set gives under 0.05, not under 1%). The invariant itself is
   proven by unit tests in `achiral.ai`; this checks the live path.
5. **Harms next to gains.** If an arm is worse than a baseline on any metric with an interval
   entirely on the wrong side of zero, that is stated beside any gain.

Whether AGGR improves anything is a separate paired claim (`achiral-aggr` minus
`achiral-rerank`) and is not made from this document.

## Reviewer protocol

The 500 items need human review before the full set is registered; I have not done it and
cannot. Reviewers get `review.csv` (one row per item, evidence text and hard negatives
inline). They edit only `question`, `gold_answer` and `gold_evidence`, and fill
`reviewer_decision` (`keep`, `change`, `drop`) and `reviewer_notes`. For each item check:

- The question is answerable from this item's events alone and reads like something a person
  would ask.
- The gold answer is right and the gold evidence is complete and minimal.
- Each hard negative looks plausible but is wrong, and is not also a valid answer.
- No event text gives away which one is gold (the lint only catches a fixed word list).
- `why`: each edge points the right way (decision to reason, reversal to what it reverses).
- `leave`: the private note really is something the asker should never see.

`team_track import-review` refuses a `keep` row that was edited, a `change` row that was not,
evidence outside the item, and a duplicate or unknown id. It reports kept, changed, dropped and
unreviewed, and per-field edit counts, and writes the reviewed stream with dropped items and
their events removed and a `review-summary.json`. Stage 4 requires publishing those counts.

**Expected effort (an estimate, not measured).** About 1.5 minutes per `sup`, `keep`, `asof`,
`prov` and `leave` item and 3 minutes per `why` item: 64 items in about 1.5 hours; 500 items in
about 15 hours for one reviewer. A second reviewer on 100 items (a fifth of the set) adds
about 3 hours and gives an agreement figure; reconciliation adds about 2 hours. Plan for 2-3
working days for one reviewer, or about 1.5 days split between two. Time the first 64 items
and re-estimate before committing the rest.

## Limits, stated before the run

- Retrieval only. No model answers and no answer is graded.
- Each item's project name is unique, so a lexical retriever finds the right item easily: the
  local `bm25` baseline scores recall@3 of 0.86 on the first batch (an unregistered replay, seed 0, for a
  sanity check only). Difficulty here comes from the hard negatives inside an item, not from
  the corpus. Corpus-scale difficulty is the scale scenarios, which are not built.
- Templated wording; one organisation shape; org and user scope only.
- Provenance is judged by which event is returned. If the retrieval API does not expose author
  and source, a system can only be right by returning the right event.

## Open questions

1. Decision-rule numbers: are the paired tests against `recency` and `bm25` the right bars, and
   should any claim also need an absolute floor?
2. Templated text: is it acceptable for a published public split, or should a model paraphrase
   it? A paraphrase would reintroduce the model-family separation rule and its own review.
3. Team change needs a team scope in the stream schema (and in `achiral.ai`). Add it, or drop
   "team change" from Track C?
4. top-k: lineage chains hold 3-5 events, so recall@3 cannot be complete. This draft uses
   k = 5 for every scenario and reports recall@3 beside it. Confirm.
5. Public and private splits: which seed is private, who holds it, and is the 500 generated
   once with the private part carved off, or two independent generations?
6. Per-category n of 60-120 gives intervals of roughly ±9 to ±13 points at p = 0.5, wider than
   Plan 40's ±11 target for some categories. Raise the sizes, or accept?
7. Should the replay record `score_team` output (it is a library function today, not called by
   the replay command), and add the Stage 4 cost and latency metrics at the same time?
8. Who reviews, and is agreement on a 100-item overlap required before the set is registered?

## Deviations

None. Nothing has been run.

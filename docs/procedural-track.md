# The procedural track

Does a system apply the team's procedure when the situation calls for it, and only then?
This is Plan 44 Phase 5. It runs offline against a test deployment, never production. The
code is `src/procedural.rs`; sizes are fixed by `ProceduralConfig::new`, and tuning seed 17 and
report seeds 4242 and 4243 (see the pre-registration).

## Stream

20 org-wide procedures ("when an annual-plan customer raises billing, loop in finance before
replying"), each asked about three times in different words (`proc-follow-*`, gold is the
procedure event) and twice in a neighbouring situation the procedure does not cover
(`proc-near-*`, no gold). Four procedures are replaced by a later event that supersedes them
and asked about once afterwards (`proc-sup-*`, gold is the new one). Four members' private
procedures are asked about by someone else (`proc-leak-*`, no gold). Every question about a
replaced procedure is asked before it was replaced.

## Metrics

| Metric | Rate of | Wanted |
|---|---|---|
| adherence | follow questions that retrieved the gold procedure | high |
| near-miss firing | near-miss questions that retrieved the procedure they must not | low |
| superseded-rule rate | sup questions where the old procedure came before the new one or alone | low |
| scope leaks | leak questions that returned another member's private procedure | zero |
| tokens per question | tokens injected, over all track questions | reported |

`score_procedural(stream, spec, results)` takes a replay's `QuestionResult`s, so it measures
retrieval, as the rest of Cognoscenti does. It does not judge a model's answer. What counts as
retrieving an event, for an ordinary memory, a chunk and a production, is fixed in
`scoring-standard.md`.

## Not done

- No command generates this stream or scores a run, and the report does not render it.
- No system arm. The gate in Plan 44 (adherence beats declarative memory alone without raising
  the near-miss rate) needs two arms: the declarative baseline, and a system whose retrieval
  includes productions. The second does not exist end to end yet.
- Pre-registered in `preregistration-procedural-2026-11.md`: arms, seeds, decision rule and limits are fixed there, before any run.

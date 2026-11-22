# The scoring standard: retrieval, not answers

Cognoscenti scores whether a memory system **retrieves the right source events** for a question.
It never scores a model's answer. This page says what that means, why, and how the same rule
applies to the two things a system may return that are not a plain event: a **chunk** and a
**production**. Where the code does not yet follow the rule, the gap is listed at the end.

## What is scored and what is not

| Scored | Not scored |
|---|---|
| Whether the gold source events are among the top-k ids a system returns (`recall_at_k`, `scoring.rs`) | Whether a model's answer is correct, complete or well written |
| Whether a system returned something its asker may not see (`leak_rate`, `is_authorized`) | Whether a returned memory's text is faithful to the event |
| Rank agreement and flips across replicates; calibration derived from scores | Latency and money (tokens are an approximation, below) |
| Tokens a question would inject, as a whitespace word count (`approx_token_count`, `tokens.rs`) | Anything a model does with what it was given |

A question with no gold evidence (an abstention, a near-miss, a leak trap) has recall `1.0`
vacuously; it is judged by what the system must *not* return, through its own metric.

## Why retrieval only

1. **The memory is the thing under test.** A model's answer mixes the memory's quality with the
   model's reading, wording and luck. Two systems fed the same evidence can answer differently,
   and two fed different evidence can answer alike. Scoring the evidence isolates the memory.
2. **It is deterministic and cheap.** No model sits in the loop. A stream, a system and a seed
   give the same score, so a run can be repeated, replicated and checked by someone else.
3. **It lets systems rewrite what they store.** Gold is tied to the id of the event that was
   ingested, not to text. A system that summarises, chunks or compiles its input is still scored
   on whether it found the right evidence (Plan 25, Scoring).
4. **It keeps a claim narrow.** A result says "this system put the gold event in its top k on
   this stream". It does not say the system is good at answering.

## Identity: the source event id

Every stream event has an id, and every question's gold is a list of event ids. A system is
asked `retrieve(query, actor, now, top_k)` and returns **ranked event ids, best first, at most
`top_k`** (`MemorySystem`, `adapter.rs`). The adapter, not the system, is responsible for mapping
whatever the system stores back to the event it came from. The `achiral` adapter sends the event
id as `cognoscenti_event_id` metadata at ingest and reads it back from each recall item
(`event_metadata`, `recall_item_id`, `achiral_http.rs`). An item that returns no such metadata
falls back to the server's own id, which can never match gold, so a broken mapping shows as a
recall of zero and is not hidden.

## The standard for a chunk

A chunk is a piece of one or more events that a system stores and returns. **A chunk is not an
identity.** It resolves to the event or events it came from, and only those count.

1. **A chunk retrieves the event it was cut from.** Its id for scoring is that event's id.
2. **Events count once.** If a system returns three chunks of one event, that is one retrieved
   event, ranked where its best chunk ranked. `top_k` counts distinct events, not chunks, so a
   system that returns many chunks of one event does not crowd others out of the score, and does
   not gain a hit by repeating itself.
3. **A chunk built from several events retrieves each event the system reports as its source.**
   A summary or consolidated chunk with no reported sources retrieves nothing: it is a miss, not
   a guess at provenance.
4. **Tokens are the chunk's text.** What a question injects is the text the system would place
   in the prompt, which is the chunk and not the whole source event, plus the query.
5. **Scope follows the source.** A chunk of a private event is private to the same asker. A
   chunk is a leak when its source event's scope does not authorise the asker.

## The standard for a production

A production is a condition-action rule (Plan 44). **A production is not an event either.** It
cites the events it was made from, and it is scored through them.

1. **A production retrieves the events in its evidence.** Each cited event counts as retrieved,
   in the production's rank. A production with no evidence retrieves nothing and cannot be
   scored; every production this benchmark authors cites its source event.
2. **Rank is selection order.** When productions and memories are both returned, the order is
   the one the system presents. The procedural track fixes it in its pre-registration:
   selected productions first, in selection order, then the declarative results, without
   duplicates. Events count once, as for chunks.
3. **A retired production does not retrieve.** A production that is superseded, suppressed or
   deleted retrieves nothing, and neither do its cited events by that route. A superseded
   procedure coming back through its production is a superseded-rule hit (`proc-sup-*`).
4. **A near-miss must not fire.** Returning a production for a situation it does not cover is
   scored as a near-miss firing (`proc-near-*`), whether or not the rule's event would also
   have been found as an ordinary memory.
5. **Scope follows the audience, and is checked against the source.** A member-scoped
   production must reach only its author. The leak check judges the cited event's scope, which
   must match the production's audience; the production store's tests prove the audience
   invariant, and the replay checks it on the live path (`proc-leak-*`).
6. **Tokens are the rendered production.** What a question injects is the production's
   condition and action text as the assistant would be shown it, plus the query, not the text of
   the event it cites.
7. **Only matching is scored.** A production's utility, how often it fired and what outcomes it
   earned are not scored here. This track tests matching, scoping and supersession. Whether
   firing the right production changes a model's answer is out of scope (see the limits in
   `preregistration-procedural-2026-11.md`).

## Rules that apply to every result

- **`top_k` is registered before a run.** A recall near 1 shows nothing; the use-learning stream
  moved to top-1 for that reason (a dated deviation in its pre-registration). Each
  pre-registration fixes `top_k` and the rule for changing it.
- **Every rate carries n and a 95% bootstrap interval** over questions (`stats.rs`); under 10
  questions is reported as "n too small". Paired comparisons use the same question in the same
  replicate, with the exact McNemar test.
- **Zero leaks supports a bound, not zero.** `n` clean trials support "under 3/n at 95%".
- **Nothing is adjusted after seeing a result.** A change is a dated deviation, and the affected
  runs are redone.
- **Claims name their limits.** A retrieval result on a synthetic stream says nothing about
  answer quality, other wording of the situation, or another organisation shape.

## How a system reports chunks and productions

`MemorySystem::retrieve_items` returns `RetrievedItem`s: `sources` (the event ids the item
stands for), `score`, and optional `text` (what the system would inject). Its default wraps
`retrieve_scored`, one item per id with that id as its source and no text, so a system that only
ranks ids needs no change. The replay (`replay_stream`, `token_usage_by_cycle`) resolves items
with `resolve_retrieval`:

- events are distinct, in rank order, at most `top_k`, each at the score of the first item that
  carried it;
- an item reporting several sources retrieves each, cut to the slots left; one reporting none
  retrieves nothing;
- items after the last slot is filled are dropped and not injected;
- an item with `text` is charged that text, every time it is returned, including a second chunk
  of an event already retrieved; an item without `text` is charged the content of the events it
  newly contributes.

A generic HTTP system (`COGNOSCENTI_<NAME>_RETRIEVE_URL`) reports this in its `results` entries:
`{"id": "...", "score": 0.9, "sources": ["e1", "e2"], "text": "..."}`. `sources` and `text` are
optional; without `sources` the entry's `id` is its one source, as before, and an empty list
means it cannot say where it came from. The `achiral` adapter reports ids only today, so it is
charged the source event's content (its default); a production adapter would report the events in
the production's evidence as `sources` and the rendered rule as `text`.

## Where the code follows this page, and where it does not yet

| Rule | State |
|---|---|
| Score by source event id, never text | Done: `recall_at_k`, `recall_item_id`. |
| Gold-less questions are vacuous for recall | Done: `recall_at_k` returns `1.0`. |
| Events count once, `top_k` counts distinct events | Done for every adapter: `resolve_retrieval` in the replay. |
| A multi-event chunk retrieves each reported source | Done in the contract (`RetrievedItem.sources`) and the generic HTTP envelope. No adapter in this repository reports more than one source yet. |
| Tokens are the text the system would inject | Done in the contract (`RetrievedItem.text`) and the generic HTTP envelope. An adapter that reports no text, which includes `achiral` today, is still charged the source event's content, so its tokens are not the text it would inject. |
| A production retrieves its cited events | Supported by the contract; no productions adapter exists. The pre-registration fixes how it ranks. |
| Leak check by source scope | Done for events: `is_authorized` on the returned event's scope. A production is judged through its cited event, as above. |

What is left is adapter work, not contract work: the `achiral` adapter reporting the text it
returns once its recall items are known to carry it, and the productions adapter.

Existing token numbers do not change for an adapter that reports no text, and are unchanged for
every run made before this change. A run that dedupes where an earlier one did not (a generic
system that returned one id twice) is not comparable with it on recall.

# The use-learning stream (specification)

Status: specification, 2026-11-21. Plan 40 Stage 5 item 1. The generator exists
(`src/use_learning.rs`, written test-first after this specification); no run has happened on
this stream. The arms, metrics, seeds and decision rule are in
`preregistration-use-learning-2026-11.md`, registered before the generator is written.

## Question

Does the activation Achiral ships (a memory is reinforced when used, and its activation decays
with time) make retrieval better the more a fact is used, compared with the same retrieval with
activation off and compared with simple baselines? Or does it not? The stream exists to produce
real numbers either way. It is not built to make ACT-R win.

The existing streams cannot answer this. Their questions ask each fact once, among look-alike
notes, so there is no use to learn from.

## What the stream contains

Every event and question is org scope, asked by users. No agent or user scope, so no route or
leak question rides on this stream (scope is Track A's). All timestamps are simulated: the stream
spans 90 simulated days, and Cognoscenti sends each question's `asked_at` as the evaluation clock.

80 topics, in four kinds. Counts and sizes are fixed here, before any run.

| Kind | Topics | What it holds | Questions | What it tests |
|---|---|---|---|---|
| `pop` (popular, correct) | 30 | One fact F, and a twin F' about the same topic that is equally relevant to the wording but is not what is asked for. Gold is F. | About 300, spread by Zipf (exponent 1.0) over the 30 topics, so the top topic is asked about 75 times and the 30th about 3 | Whether use breaks a near-tie toward the fact that was used. The one place use should help |
| `rare` (headroom) | 20 | A correct fact C asked about rarely, behind a popular wrong fact D: D is the gold of a heavily repeated question family on a near-twin topic, so D accumulates use. | About 150 questions on D's family, then 30 on C | Whether use pushes a popular wrong fact above a correct rarely-used one. Where activation can cost recall |
| `change` | 15 | A fact A, superseded (explicit `supersedes`) by B at a simulated day between 30 and 60 per topic. | About 90, half before the change (gold A) and half after (gold B) | Whether the old, used fact is displaced by the current one |
| `tail` | 15 | One fact, asked once. | 15 | No use signal. Every arm should tie; a gap here is not about use |

About 600 questions in all. Facts are templated sentences naming a topic, an attribute and a
value; the twins share the topic and attribute wording and differ in the value, so semantic
relevance is the same by construction and only use can separate them.

Repeats and rewordings. A topic's first question uses the original wording. Every later ask picks
one of four wordings (the original and three fixed rewordings) uniformly from the generator
seed, so the use-count curve is not confounded with wording and reworded asks are reported apart
from original-wording asks.

Questions are scheduled over the 90 days at times drawn uniformly from the generator seed and sorted (a Poisson process given its count), so
the time between uses varies and decay has something to act on. A question is placed after every
event it may depend on.

Use count. For a question, the number of earlier asks whose gold event is the same event.
A `pop` topic's asks share one gold, so its count climbs with every ask; `rare-c` and `change`
questions count asks of their own gold fact, so the first ask after a change starts at 0. Bins:
0 (first ask), 1 to 2, 3 to 6, 7 and over. A bin with fewer than 10 questions is marked "n too
small" and not plotted.

Ids. Question ids are `{category}-{NNN}-q` with the categories `pop`, `rare-d` (the popular
staging fact's own questions, which build its use), `rare-c` (the correct production fact, the
headroom questions), `change` and `tail`, so `category_of` groups them without change. Event ids
name the topic: `pop-t07-f` (fact) and `pop-t07-twin`, `rare-t03-c` and `rare-t03-d`,
`change-t11-a` and `change-t11-b`, `tail-t02-f`. Each question has exactly one gold event. A
`rare` topic asks about D seven or eight times before its first `rare-c` question, by
construction. The dataset lint covers the stream and must pass with no issue before any run.

Generate it with `cognoscenti --generate-use-learning <path> --seed <seed>`; the command lints the
file and prints its SHA-256 (`src/use_learning.rs`). The `pop` twin is a different slot of the
same topic, not a conflicting value for the same slot, so the question has exactly one right
answer and the near-tie is only in how relevant the two notes look to the wording.

## What Cognoscenti has to gain first

1. Done: `use_feedback` in `AchiralHttpSystem`, off unless `COGNOSCENTI_ACHIRAL_USE_FEEDBACK=1`.
   The adapter keeps the map from an event id to the server memory id (the `id` in each recall
   item, read together with `metadata.cognoscenti_event_id`) and sends
   `POST /v1/memory/{id}/reinforce` with the `X-Achiral-Eval-Now` header and a body naming the
   reason (the token needs `memory:control`). The endpoint takes no helpful flag: it always adds
   0.05 activation and one access. So only `helpful = true` sends a call; `helpful = false`
   sends none.
2. Done: the replay loop calls `use_feedback` after each question, for systems that declare the
   capability: the retrieved memories that are in the gold set are reported as used and helpful,
   the others are not reported. This stands in for a client that reinforces what it used. There
   is no `--feedback` flag: the environment variable is the one switch, so a run's feedback
   setting is part of the launch environment (and of the arm name), not a second control.
3. Done: the generator, test-first, after this specification was committed.
4. The use-count baseline and per-bin curves in `results.rs` and `report.rs` (a `use_count` on
   each question row, a chart of recall against use bin, one line per arm, with the paired
   difference to the off arm).

The server also reinforces whatever `recall` returns, in every mode but `graph`, `dense` and
`hybrid`. That cannot be turned off for the `rerank` arms, so "feedback off" means no explicit
reinforce call, not no reinforcement. The report says so beside the arm.

## Limits stated up front

- Templated facts in a synthetic stream. The result is about this stream, not about memory in
  general, and not about people's real questions.
- Org scope and users only; scope, agents and calibration are measured elsewhere.
- With n of a few tens per use bin, small differences cannot be told apart. Intervals are shown,
  and a difference whose interval spans zero is not reported as a difference.

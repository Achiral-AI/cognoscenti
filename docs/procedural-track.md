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

## Run it

Three commands, in `src/procedural_cli.rs`. A run is one `generate`, one `replay` per arm and
seed, and one `report`. Nothing here is run by `cargo test`; the tests use a local mock server
and an in-memory productions server.

```sh
# 1. The stream, per seed (17 tuning; 4242 and 4243 report). Same seed, same bytes.
cognoscenti procedural generate --seed 4242 --out run/4242.jsonl
cognoscenti procedural generate --seed 4243 --out run/4243.jsonl

# 2. One replay per arm and seed: 5 replicates, top-k 1, each on a fresh organisation.
for seed in 4242 4243; do
  for arm in declarative procedural bm25; do
    cognoscenti procedural replay --stream run/$seed.jsonl --arm $arm \
      --replicates 5 --top-k 1 --label $arm-$seed --results-json run/$arm-$seed.json
  done
done

# 3. The report: paired comparison and the registered decision rule.
cognoscenti procedural report --results run/*-4242.json run/*-4243.json --out run/report
```

`replay` refuses a stream that is not byte-for-byte what its seed header generates, so the
spec (which near-miss must not retrieve which procedure) is never read from a file that could
differ from the stream.

### Environment

| Arm | Needs |
|---|---|
| `bm25` | nothing |
| `declarative` | `COGNOSCENTI_ACHIRAL_BASE_URL`, `COGNOSCENTI_ACHIRAL_API_KEYS` (one key per replicate, so five orgs with `aiConfig.proceduralMemory` **off**), `COGNOSCENTI_ACHIRAL_USER_MAP_FILE`, `COGNOSCENTI_ACHIRAL_SETTLE_MS=2000`, `COGNOSCENTI_ACHIRAL_RETRIEVAL_MODE=rerank`, retrieval noise 0 through `COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS` |
| `procedural` | the same, with five orgs where the flag is **on**, plus `COGNOSCENTI_PROCEDURAL_BASE_URL` (the app origin that serves `/api/productions`) and `COGNOSCENTI_PROCEDURAL_SESSIONS_FILE` |

The sessions file is JSON, one entry per replicate (a single object is reused for all). Each
entry names that replicate's organisation, an admin's NextAuth session-token value, and one
member session per stream user (`ana`, `ben`, `chi`, `dev`), each the same person the user map
resolves that label to:

```json
[{"organization_id": "...", "admin": "<session token>", "members": {"ana": "...", "ben": "...", "chi": "...", "dev": "..."}}]
```

Session tokens are read, never written: they appear in no run file or report, and the report
refuses output that holds a key, an address or `http`. The `procedural` arm checks the flag
before ingesting anything: a `404` from `GET /api/productions` stops the run with
"aiConfig.proceduralMemory is off".

### What each command prints

```
$ cognoscenti procedural generate --seed 4242 --out run/4242.jsonl
Generated run/4242.jsonl (procedural track, seed 4242)
sha256 <64 hex characters>

$ cognoscenti procedural replay --stream run/4242.jsonl --arm procedural ...
procedural seed 4242 replicate 0: adherence 51/60, near-miss firing 9/40, superseded-rule 0/4, scope leaks 0/4
results saved to run/procedural-4242.json

$ cognoscenti procedural report --results ...
report written to run/report/index.html
```

(The numbers are the shape, not a result: `bm25` on both report seeds printed adherence 60/60,
near-miss 8/40 and 10/40, superseded 3/4 and 2/4, leaks 4/4 on a dry run. That arm has no scope,
so its leaks are expected and only a sanity check.)

`report` writes `index.html` and `results.json` to `--out`. Rates are replicate 0 of both seeds
pooled (60 follow, 40 near-miss questions per seed; 120 and 80 pooled), each with a 95% bootstrap
interval (seed 0), "n too small" under 10 questions. It also gives the count per replicate, tokens
per question, the flip rate over replicates, the paired difference `procedural` minus
`declarative` with its interval and exact McNemar p, and the four conditions of the decision rule,
each marked as holding or not, ending in a claim or "no claim is made".

### What the `procedural` arm does

`src/productions_http.rs`. For each procedure event it also ingests the memory, then creates a
production through `POST /api/productions`: org-wide events with the admin's session
(`scope: "org"`, active at once; a replacement with the same cues retires the original on the
server), private events with the member's session (`scope: "member"`). Evidence is the event id.
For each question it lists `GET /api/productions` with the asker's session, matches, picks up to
three with noise scale 0.15 and seed = the replicate index, and returns each selected
production's evidence event first (text: the rendered condition and action), then the declarative
results. `resolve_retrieval` removes duplicates, so `top_k 1` is the first of that list.

## Deviations from the pre-registration

These are what the registered wording cannot do as written. The pre-registration is not edited;
record them there as dated deviations before the first report-seed run.

1. **No selection route.** The pre-registration says the adapter "asks the productions API's
   selection". `routes/productions.js` has no such route: only list, create, read, edit, delete
   and approve. `selectProductions` runs inside the server and nothing in the HTTP surface calls
   it. The adapter lists what the asker may see and applies the same threshold (0.5, inclusive),
   maximum (3), noise (0.15) and `mulberry32` seeded draw itself. The port is checked against
   values from the server's own function, but it is a port: a change to `Production.js` after the
   recorded commit is not picked up.
2. **Match strength is cue overlap only.** The server's strength averages cue overlap with a vector
   similarity of the condition text; no route returns that similarity, so it is absent. A rule
   passes at half its cues present in the question.
3. **How a question's cues are taken is not registered.** The adapter uses the same rule as
   authoring: lowercased words over three letters, split on anything that is not a letter or digit
   ("annual-plan" is "annual" and "plan"). "raises" is a cue of every procedure and so shares one
   cue with every question phrased "raises"; the registered Limits already expect a high
   near-miss rate.
4. **Authentication is a NextAuth session cookie per user**, not an API key. The runner needs five
   organisations' admin and member sessions, supplied out of band.
5. **Evidence is an opaque reference.** The server does not resolve it, and the event it cites is
   not yet written when the production is created (ingest is asynchronous). The adapter stores the
   stream event id as `{collection: "CognoscentiEvent", weaviateId: <event id>}` and reads it back
   from the listing. Nothing on the server validates it.
6. **Tokens differ by design.** A selected production's tokens are its rendered condition and
   action (the first deviation entry already in the pre-registration); the declarative results
   still count their events' full content.
7. **Noise at the registered scale lowers top-1 adherence.** A rule that shares the qualifier and
   "raises" with the right one is 0.25 behind it, and the registered logistic noise at 0.15 swaps
   them often enough that top-1 adherence is below 100% even with perfect matching. With noise
   0 in the in-memory server the adapter follows all 60. This is the registered behaviour, not
   a defect; it is what the arm measures.

## Not done

- No run against a deployment: nothing was sent to any server. The arms were proven against a
  mock HTTP server (the curl transport: URL, cookie, body, 404) and an in-memory productions server
  with the route's rules (admin-only org rules, member audiences, same-cue supersession).
- The five organisations with the flag on and off, their sessions and user map, are not
  provisioned.
- The gate in Plan 44 (adherence beats declarative alone without raising the near-miss rate) is
  decided by `report` from real runs only.
- Pre-registered in `preregistration-procedural-2026-11.md`: arms, seeds, decision rule and limits
  are fixed there, before any run.

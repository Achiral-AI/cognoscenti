//! `MemorySystem` adapter calling Achiral's real public memory API
//! (`achiral.ai`'s `routes/publicMemory.js`), not Cognoscenti's generic
//! envelope (`http_stream.rs`) - every shape below was verified against
//! the actual route/service code on 2026-11-07, not guessed. Configured
//! via `COGNOSCENTI_ACHIRAL_BASE_URL` and `COGNOSCENTI_ACHIRAL_API_KEY`,
//! sent as `Authorization: Bearer <api_key>` (matching
//! `middleware/mcpTokenAuth.js`'s `acm_...` token convention).
//!
//! Scope support, updated 2026-11-13: `ScopeKind::Org`, `ScopeKind::Agent`
//! and `ScopeKind::User` are all wired on ingest (`ingest_url` picks the
//! route). Org goes to `POST {base}/v1/memory/events` (ingest) and
//! `POST {base}/v1/memory/retrieve` (retrieve, async triage - a 5-minute
//! cron actually writes to Weaviate, not the ingest call itself). Agent goes
//! to `POST {base}/v1/memory/agents/{slug}` (`remember`, synchronous, no
//! triage lag, but no `occurredAt` support) and
//! `POST {base}/v1/memory/agents/{slug}/retrieve` - both verified against achiral.ai's
//! real route/service code, not guessed; the gap here was always this
//! adapter never calling them, not achiral.ai missing them. User goes to
//! `POST {base}/v1/memory/users/{id}/events`, where `{id}` is resolved from
//! the dataset's role label (`"platform-lead"`) through
//! `COGNOSCENTI_ACHIRAL_USER_MAP_FILE` to a real org member id first
//! (`resolve_user_scope_id`) - achiral.ai 404s on an unresolved label, and
//! this adapter panics before even trying if the map has no entry for it.
//! A user actor's retrieve still goes to the org-scope endpoint,
//! deliberately - see `retrieve_url`'s doc comment for why that is
//! correct, not a remaining gap.
//!
//! `actor` now decides the retrieve route (an agent actor retrieves from its
//! own scope; a user actor from the org's) - `recall` has no separate actor
//! concept on achiral.ai's public API, so this is purely adapter-side
//! routing. `now` IS sent, as the `X-Achiral-Eval-Now` header
//! (`eval_now_header`) - achiral.ai's `recall`/`reinforce` honour it under
//! `INFERENCE_EVAL_MODE`, confirmed 2026-11-08 (Plan 25's "Public memory
//! API: honour now" is done on both sides now). `reinforce` itself is not
//! wired into this adapter yet (no `use_feedback` override), so only
//! retrieval is time-travel-safe so far.

use crate::adapter::MemorySystem;
use crate::http_stream::run_json_post;
use crate::stream::{Actor, ActorKind, Scope, ScopeKind, StreamEvent};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;

pub struct AchiralHttpSystem {
    base_url: String,
    api_key: Option<String>,
    /// The longest settle() will wait for achiral.ai's asynchronous write
    /// cycle (ingest only stages a candidate; a separate cycle writes it
    /// to the store) to confirm every org-scope event ingested so far has
    /// actually reached Weaviate, polling `event_status_payload` rather
    /// than blindly sleeping - a blind sleep raced the real,
    /// congestion-dependent write latency and sometimes lost (confirmed
    /// live: 578ms-11,114ms for logically identical events, purely a
    /// function of how many other organizations shared that write
    /// tick's queue). 0 by default - a real production deployment's
    /// write cycle runs on minutes, which no settle() wait could make
    /// synchronous anyway, so this stays an explicit opt-in for a
    /// benchmark deployment tuned for a fast cycle, not a default
    /// behavior change.
    settle_ms: u64,
    /// Org-scope events ingested since the last successful settle(),
    /// identified by the `businessEventId` achiral.ai's own ingest
    /// response returns - what settle() polls achiral.ai to confirm.
    /// Agent-scope `remember` writes are synchronous already (no triage
    /// lag), so `ingest()` never adds to this for an agent-scope event.
    unconfirmed_business_event_ids: Vec<String>,
    /// This replicate's actor-name -> real org-member-id map (see
    /// `resolve_user_map_for_replicate`'s doc comment) - empty when
    /// `COGNOSCENTI_ACHIRAL_USER_MAP_FILE` isn't set, which is fine for any
    /// dataset with no `ScopeKind::User` event.
    user_member_ids: HashMap<String, String>,
}

/// Picks the API key for one replicate - a pure function over the two
/// relevant env values (not reading `std::env` itself, so it is testable
/// without the parallel-test env-var races that come with mutating global
/// process state).
///
/// `COGNOSCENTI_ACHIRAL_API_KEYS` (comma-separated), when present, names
/// one independently-provisioned org per replicate - the only way
/// `--replicates` can test independent runs against a live, stateful
/// deployment rather than every replicate accumulating into the same one.
/// Its count must be at least `index + 1`; an index past the end is a
/// setup error (not enough orgs provisioned for the requested replicate
/// count), not a silent fallback. Without it, every replicate uses the
/// single `COGNOSCENTI_ACHIRAL_API_KEY` - today's behavior, unchanged.
fn resolve_api_key_for_replicate(
    keys_env: Option<&str>,
    single_key_env: Option<&str>,
    index: usize,
) -> Result<Option<String>> {
    match keys_env {
        Some(keys) => {
            let keys: Vec<&str> = keys
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .collect();
            let key = keys.get(index).with_context(|| {
                format!(
                    "COGNOSCENTI_ACHIRAL_API_KEYS names {} key(s), but replicate index {index} needs one more",
                    keys.len()
                )
            })?;
            Ok(Some(key.to_string()))
        }
        None => Ok(single_key_env.map(str::to_string)),
    }
}

/// This replicate's actor-name -> real org-member-id map, from the
/// already-read contents of `COGNOSCENTI_ACHIRAL_USER_MAP_FILE` - a pure
/// function over the file's text, mirroring `resolve_api_key_for_replicate`'s
/// split between reading the env/file (impure, in `from_env_at`) and
/// resolving one replicate's piece of it (pure, testable).
///
/// achiral.ai's `/v1/memory/users/:userId/events` requires `:userId` be a
/// real member of the org the API key belongs to - a dataset's named
/// user-scope actor (`"platform-lead"`) is just a role label, never a real
/// member id, so this table is how a benchmark run bridges the two. The
/// file is a JSON object (one map, shared by every replicate) or a JSON
/// array of objects (one map per replicate, same indexing as
/// `COGNOSCENTI_ACHIRAL_API_KEYS`) - an index past the end of an array is a
/// setup error, matching `resolve_api_key_for_replicate`'s own rule. No
/// file at all is not an error - most datasets have no user-scope actor to
/// map.
fn resolve_user_map_for_replicate(
    file_contents: Option<&str>,
    index: usize,
) -> Result<HashMap<String, String>> {
    let Some(contents) = file_contents else {
        return Ok(HashMap::new());
    };
    let value: Value = serde_json::from_str(contents)
        .with_context(|| "COGNOSCENTI_ACHIRAL_USER_MAP_FILE is not valid JSON")?;
    match value {
        Value::Array(maps) => {
            let entry = maps.get(index).with_context(|| {
                format!(
                    "COGNOSCENTI_ACHIRAL_USER_MAP_FILE names {} map(s), but replicate index {index} needs one more",
                    maps.len()
                )
            })?;
            parse_user_map(entry)
        }
        other => parse_user_map(&other),
    }
}

fn parse_user_map(value: &Value) -> Result<HashMap<String, String>> {
    value
        .as_object()
        .with_context(|| "COGNOSCENTI_ACHIRAL_USER_MAP_FILE entry is not a JSON object")?
        .iter()
        .map(|(actor_id, member_id)| {
            let member_id = member_id
                .as_str()
                .with_context(|| format!("user map entry {actor_id:?} is not a string"))?;
            Ok((actor_id.clone(), member_id.to_string()))
        })
        .collect()
}

/// The real org-member id to put in a URL for this event's scope - `Org`
/// and `Agent` scope ids are already the real thing (an org has one API
/// key, an agent slug is itself the identifier); only `User` scope needs
/// resolving, since its id is a dataset's role label, not a provisioned
/// member. Errors rather than sending the unresolved label on, which would
/// just 404 less clearly.
fn resolve_user_scope_id<'a>(
    scope: &'a Scope,
    user_member_ids: &'a HashMap<String, String>,
) -> Result<&'a str> {
    match scope.kind {
        ScopeKind::User => user_member_ids
            .get(&scope.id)
            .map(String::as_str)
            .with_context(|| {
                format!(
                    "no provisioned org member for user-scope actor {:?} - add it to \
                     COGNOSCENTI_ACHIRAL_USER_MAP_FILE",
                    scope.id
                )
            }),
        ScopeKind::Org | ScopeKind::Agent => Ok(&scope.id),
    }
}

impl AchiralHttpSystem {
    pub fn from_env() -> Result<Self> {
        Self::from_env_at(0)
    }

    /// Like `from_env()`, but for one replicate of several - see
    /// `resolve_api_key_for_replicate`.
    pub fn from_env_at(index: usize) -> Result<Self> {
        let base_url = std::env::var("COGNOSCENTI_ACHIRAL_BASE_URL")
            .with_context(|| "missing COGNOSCENTI_ACHIRAL_BASE_URL")?;
        let api_key = resolve_api_key_for_replicate(
            std::env::var("COGNOSCENTI_ACHIRAL_API_KEYS")
                .ok()
                .as_deref(),
            std::env::var("COGNOSCENTI_ACHIRAL_API_KEY").ok().as_deref(),
            index,
        )?;
        let settle_ms = std::env::var("COGNOSCENTI_ACHIRAL_SETTLE_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let user_map_contents = std::env::var("COGNOSCENTI_ACHIRAL_USER_MAP_FILE")
            .ok()
            .map(|path| {
                std::fs::read_to_string(&path)
                    .with_context(|| format!("reading COGNOSCENTI_ACHIRAL_USER_MAP_FILE at {path}"))
            })
            .transpose()?;
        let user_member_ids = resolve_user_map_for_replicate(user_map_contents.as_deref(), index)?;
        Ok(Self {
            base_url,
            api_key,
            settle_ms,
            unconfirmed_business_event_ids: Vec::new(),
            user_member_ids,
        })
    }
}

/// `metadata` sent at ingest time on every scope, carrying the original
/// StreamEvent id so a later retrieve can map a result back to it -
/// achiral.ai's own `id` is the Weaviate document's internal UUID, never
/// the StreamEvent id, so without this round trip recall scoring could
/// never match a result against `gold_event_ids`.
fn event_metadata(event: &StreamEvent) -> Value {
    json!({
        "cognoscenti_event_id": event.id,
        "source": event.source,
        "supersedes": event.supersedes,
    })
}

/// `POST /v1/memory/events`'s body (`normalizeDeveloperEvent`,
/// achiral.ai `services/memory/PublicMemoryService.js:149-185`): requires
/// `type`/`eventType` and `text`/`summary`/`description`; `occurredAt` is
/// honored, confirmed (defaults to the wall clock if absent - we always
/// send it so stream order is preserved). `type` has no fixed vocabulary
/// verified in that code, so this names the event's real origin plainly
/// rather than inventing a value the vendor might expect.
pub fn ingest_event_payload(event: &StreamEvent) -> Value {
    json!({
        "type": "cognoscenti-stream-event",
        "text": event.content,
        "occurredAt": event.timestamp.to_rfc3339(),
        "actor": event.actor,
        "metadata": event_metadata(event),
        // This event's own id, so a later event's `supersedes` reference
        // (below) can resolve against it (achiral.ai's eventExternalId()
        // reads payload.id/externalId/eventId, stored as
        // BusinessEvent.source.externalId) - metadata.cognoscenti_event_id
        // above is for output-side result scoring only, never read as an
        // external id.
        "id": event.id,
        // Plan 25/26 review (achiral.ai, 2026-11-10): ingestEvent() now
        // reads an explicit supersession reference from this top-level
        // field, resolved within this event's own source scope - not from
        // `metadata`, which event_metadata() above also carries for
        // debugging but which achiral.ai never reads for this purpose.
        "supersedes": event.supersedes,
        // Plan 26 review (achiral.ai, 2026-11-11): normalizeDeveloperEvent()
        // now reads this and passes it to labelEvent(), which resolves it
        // into a findable BusinessEntity (any entityType, not just a CRM
        // customer/person) - without it, a stream's declared entities
        // never reach the system at all, and CueExtractor seeds nothing.
        "entityReferences": entity_references(event),
    })
}

fn entity_references(event: &StreamEvent) -> Value {
    json!(event
        .entities
        .iter()
        .map(|entity| json!({
            "entityType": entity.entity_type,
            "externalId": entity.external_id,
            "displayName": entity.display_name,
        }))
        .collect::<Vec<_>>())
}

/// `POST /v1/memory/agents/{slug}`'s body (`remember`, achiral.ai
/// `services/memory/PublicMemoryService.js:499-567`): requires `text`;
/// writes directly and synchronously into the agent's own memory store
/// (`agentStore.writeStructured`) - unlike the org-scope `/events` path,
/// there is no async triage/candidate-write lag, and no `occurredAt`
/// support at all (`remember()` reads no timestamp field), confirmed by
/// reading the method body.
pub fn ingest_agent_payload(event: &StreamEvent) -> Value {
    json!({
        "text": event.content,
        "metadata": event_metadata(event),
    })
}

/// Where to `POST` an ingest for this event's scope.
pub fn ingest_url(base_url: &str, scope: &Scope) -> Option<String> {
    match scope.kind {
        ScopeKind::Org => Some(format!("{base_url}/v1/memory/events")),
        // `/v1/memory/agents/:agentSlug` (`remember`), verified end to end
        // against achiral.ai's real route/service code 2026-11-09 -
        // confirmed this adapter is the only thing that was missing, not
        // achiral.ai.
        ScopeKind::Agent => Some(format!("{base_url}/v1/memory/agents/{}", scope.id)),
        // `/v1/memory/users/:userId/events`, added 2026-11-13 to close the
        // gap `beacon-01`'s `plat-e1` needs - `scope.id` is the owning
        // member's id, carried through to `explicitOwnerUserId` so the
        // candidate is written as that member's personal memory, not an
        // org-wide one. achiral.ai requires `scope.id` be a real member of
        // the org the API key belongs to (`resolveOrgMemberUserId`); the
        // benchmark org provisioned for a dataset using this scope must
        // register that member first, or ingest 404s.
        ScopeKind::User => Some(format!("{base_url}/v1/memory/users/{}/events", scope.id)),
    }
}

/// Where to `POST` a retrieval for this actor - the actor asking is who
/// the query is scoped to (an agent asks within its own memory; a user
/// asks within the org's). A user actor stays on the org-scope endpoint
/// deliberately, not because no user-scoped retrieve route exists
/// (`/v1/memory/users/:userId/retrieve` does, as of 2026-11-13) - every
/// dataset's `asked_by` actor is a role name (`"ops-lead"`, `"pm"`), not a
/// provisioned org member, and `beacon-01`'s own leak-rate question
/// (`beacon-q-leak`) depends on the asker NOT inheriting another member's
/// personal audience: org-scope retrieve never does, by construction
/// (`audienceFilter([])` never includes a personal audience). Routing
/// every user actor through its own `/users/:id/retrieve` would need that
/// id to be a real member too, and would only ever add visibility, never
/// remove it - the wrong direction for a leak-rate trap.
pub fn retrieve_url(base_url: &str, actor: &Actor) -> String {
    match actor.kind {
        ActorKind::Agent => format!("{base_url}/v1/memory/agents/{}/retrieve", actor.id),
        ActorKind::User => format!("{base_url}/v1/memory/retrieve"),
    }
}

/// `POST /v1/memory/retrieve`'s body (`PublicMemoryService.recall`,
/// `:334-343`): `{query, limit, category}`. No `actor` or `now` field
/// exists on this endpoint, confirmed - results reflect current
/// wall-clock state, not simulated time.
pub fn retrieve_payload(query: &str, top_k: usize) -> Value {
    json!({ "query": query, "limit": top_k })
}

/// `POST /v1/memory/retrieve`'s body for a Plan 26 Phase 5 evidence-graph
/// request - the same endpoint, query and `top_k` as `retrieve_payload`,
/// with one extra opt-in flag achiral.ai only honors under `aggr` mode.
/// `top_k` as `limit` is not optional here: without it, achiral.ai's
/// default limit lets more documents into the merged pool than the real
/// retrieve ever sees, so `unresolvedGaps` would be computed against a
/// different, larger result set than the one actually being evaluated.
pub fn evidence_graph_payload(query: &str, top_k: usize) -> Value {
    json!({ "query": query, "limit": top_k, "includeEvidenceGraph": true })
}

/// `evidenceGraph.unresolvedGaps[].displayName` - `entityId` is a Mongo
/// ObjectId assigned at write time, unknowable to a dataset author ahead
/// of time; `displayName` is the one stable field a dataset's own
/// `entities[].display_name` can be compared against directly.
pub fn parse_unresolved_gap_display_names(value: &Value) -> Vec<String> {
    value
        .get("evidenceGraph")
        .and_then(|evidence_graph| evidence_graph.get("unresolvedGaps"))
        .and_then(Value::as_array)
        .map(|gaps| {
            gaps.iter()
                .filter_map(|gap| gap.get("displayName").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `POST /v1/memory/events/status`'s body (Plan 25/26 durable-write fix) -
/// asks achiral.ai whether every `MemoryCandidate` derived from each
/// `businessEventId` has reached Weaviate yet.
pub fn event_status_payload(business_event_ids: &[String]) -> Value {
    json!({ "businessEventIds": business_event_ids })
}

/// `{businessEventId -> status}` from `/v1/memory/events/status`'s
/// response (`PublicMemoryService.eventStatus`, `status` one of
/// `"written"`/`"pending"`/`"not_found"`).
pub fn parse_event_statuses(value: &Value) -> std::collections::HashMap<String, String> {
    value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("businessEventId").and_then(Value::as_str)?;
                    let status = item.get("status").and_then(Value::as_str)?;
                    Some((id.to_string(), status.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `ingest_event_payload`'s response carries the staged `BusinessEvent`'s
/// own id (`PublicMemoryService.ingestEvent`'s `businessEventId`) - the
/// one thing `settle()` needs to later ask `/v1/memory/events/status`
/// whether this specific event made it to Weaviate yet.
fn parse_business_event_id(value: &Value) -> Option<String> {
    value
        .get("businessEventId")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Pure polling loop (Plan 25/26 durable-write fix), no I/O of its own -
/// `check` does the real work (an HTTP call in production, a fake in
/// tests) and returns whichever of the ids it was given are still
/// unconfirmed. Stops as soon as nothing is left, or once `max_wait_ms`
/// has elapsed - replacing a blind sleep that raced achiral.ai's real,
/// congestion-dependent write latency (confirmed live: 578ms-11,114ms
/// for logically identical events) with an actual confirmation, bounded
/// by the same wait budget the blind sleep used to spend unconditionally.
fn poll_until_confirmed(
    ids: Vec<String>,
    max_wait_ms: u64,
    poll_interval_ms: u64,
    mut check: impl FnMut(&[String]) -> Vec<String>,
) -> Vec<String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_wait_ms);
    let mut remaining = ids;
    loop {
        if remaining.is_empty() {
            return remaining;
        }
        remaining = check(&remaining);
        if remaining.is_empty() || std::time::Instant::now() >= deadline {
            return remaining;
        }
        let time_left = deadline.saturating_duration_since(std::time::Instant::now());
        std::thread::sleep(std::cmp::min(
            std::time::Duration::from_millis(poll_interval_ms),
            time_left,
        ));
    }
}

/// Simulated time for achiral.ai's `recall`/`reinforce` eval-mode override
/// (`X-Achiral-Eval-Now`, ISO 8601) - the public memory API takes it as a
/// header, not a body field, confirmed 2026-11-08. Server-side, it's
/// ignored unless `INFERENCE_EVAL_MODE=true`, so sending it unconditionally
/// here is harmless against a production deployment.
pub fn eval_now_header(now: DateTime<Utc>) -> (&'static str, String) {
    ("X-Achiral-Eval-Now", now.to_rfc3339())
}

/// The id to score this retrieved item against. Prefers the original
/// `cognoscenti_event_id` this adapter sent at ingest time
/// (`event_metadata`), echoed back in `metadata` when achiral.ai
/// preserved it, over achiral.ai's own `id` - the Weaviate document's
/// internal UUID, unrelated to any StreamEvent id. `gold_event_ids` are
/// StreamEvent ids, so scoring against the raw UUID could never match.
fn recall_item_id(item: &Value) -> Option<String> {
    item.get("metadata")
        .and_then(|metadata| metadata.get("cognoscenti_event_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| item.get("id").and_then(Value::as_str).map(str::to_string))
}

/// Parses the org-scope `recall` response (`serializeMemory`,
/// `:102-123`): `{"data": [{"id", "score", ...}, ...]}`. `score` is the
/// real activation score (`doc._activationScore ?? doc.activationScore
/// ?? null`), defaulted to 0.0 here when null or missing - same
/// convention as `http_stream::parse_retrieve_response`.
pub fn parse_recall_response(value: &Value) -> Vec<(String, f64)> {
    value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = recall_item_id(item)?;
                    let score = item.get("score").and_then(Value::as_f64).unwrap_or(0.0);
                    Some((id, score))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl MemorySystem for AchiralHttpSystem {
    fn name(&self) -> &str {
        "achiral"
    }

    fn ingest(&mut self, event: &StreamEvent) {
        let resolved_id = resolve_user_scope_id(&event.scope, &self.user_member_ids)
            .unwrap_or_else(|err| panic!("{err:#}"));
        let resolved_scope = Scope {
            kind: event.scope.kind,
            id: resolved_id.to_string(),
        };
        let url = ingest_url(&self.base_url, &resolved_scope).unwrap_or_else(|| {
            panic!(
                "AchiralHttpSystem has no verified user-scope ingest path - achiral.ai's \
                 public memory API has no user-scoped route at all"
            )
        });
        let payload = match event.scope.kind {
            ScopeKind::Agent => ingest_agent_payload(event),
            ScopeKind::Org | ScopeKind::User => ingest_event_payload(event),
        };
        let response = run_json_post(&url, self.api_key.as_deref(), &[], &payload)
            .unwrap_or_else(|err| panic!("achiral ingest failed: {err:#}"));
        // Agent-scope `remember` is already synchronous (no triage lag) -
        // only an org/user-scope event has a businessEventId to track.
        if matches!(event.scope.kind, ScopeKind::Org | ScopeKind::User) {
            if let Some(business_event_id) = parse_business_event_id(&response) {
                self.unconfirmed_business_event_ids.push(business_event_id);
            }
        }
    }

    fn retrieve(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<String> {
        self.retrieve_scored(query, actor, now, top_k)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    fn capabilities(&self) -> crate::adapter::Capabilities {
        crate::adapter::Capabilities {
            settle: true,
            evidence_graph: true,
            ..Default::default()
        }
    }

    fn settle(&mut self, _now: DateTime<Utc>) {
        // Nothing org/user-scope was ingested since the last settle (or
        // this system never tracked anything, e.g. agent-scope only) -
        // nothing to poll for, so fall back to the old blind wait, if any.
        if self.unconfirmed_business_event_ids.is_empty() {
            if self.settle_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(self.settle_ms));
            }
            return;
        }
        let ids = std::mem::take(&mut self.unconfirmed_business_event_ids);
        let url = format!("{}/v1/memory/events/status", self.base_url);
        let api_key = self.api_key.clone();
        let still_unconfirmed = poll_until_confirmed(ids, self.settle_ms, 250, |remaining| {
            let payload = event_status_payload(remaining);
            let response = match run_json_post(&url, api_key.as_deref(), &[], &payload) {
                Ok(response) => response,
                // A transient status-check failure shouldn't panic the whole
                // replay - treat it as "still unconfirmed" and let the
                // overall wait budget decide when to give up.
                Err(_) => return remaining.to_vec(),
            };
            let statuses = parse_event_statuses(&response);
            remaining
                .iter()
                .filter(|id| statuses.get(id.as_str()).map(String::as_str) != Some("written"))
                .cloned()
                .collect()
        });
        if !still_unconfirmed.is_empty() {
            eprintln!(
                "[achiral] settle(): {} event(s) not confirmed written within {}ms: {:?}",
                still_unconfirmed.len(),
                self.settle_ms,
                still_unconfirmed
            );
        }
    }

    fn retrieve_scored(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<(String, f64)> {
        let url = retrieve_url(&self.base_url, actor);
        let payload = retrieve_payload(query, top_k);
        let headers = [eval_now_header(now)];
        let response = run_json_post(&url, self.api_key.as_deref(), &headers, &payload)
            .unwrap_or_else(|err| panic!("achiral retrieve failed: {err:#}"));
        parse_recall_response(&response)
            .into_iter()
            .take(top_k)
            .collect()
    }

    fn evidence_graph_gaps(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<String> {
        let url = retrieve_url(&self.base_url, actor);
        let payload = evidence_graph_payload(query, top_k);
        let headers = [eval_now_header(now)];
        let response = run_json_post(&url, self.api_key.as_deref(), &headers, &payload)
            .unwrap_or_else(|err| panic!("achiral evidence graph retrieve failed: {err:#}"));
        parse_unresolved_gap_display_names(&response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{ActorKind, Scope};

    #[test]
    fn resolve_api_key_picks_the_nth_key_when_a_keys_list_is_given() {
        let key = resolve_api_key_for_replicate(Some("k0,k1,k2"), None, 1).unwrap();
        assert_eq!(key, Some("k1".to_string()));
    }

    #[test]
    fn resolve_api_key_trims_whitespace_around_each_key() {
        let key = resolve_api_key_for_replicate(Some(" k0 , k1 "), None, 1).unwrap();
        assert_eq!(key, Some("k1".to_string()));
    }

    #[test]
    fn resolve_api_key_errors_when_the_index_has_no_matching_key() {
        let err = resolve_api_key_for_replicate(Some("k0,k1"), None, 2).unwrap_err();
        assert!(err.to_string().contains("2 key(s)"));
    }

    #[test]
    fn resolve_api_key_falls_back_to_the_single_key_without_a_keys_list() {
        let key = resolve_api_key_for_replicate(None, Some("solo-key"), 7).unwrap();
        assert_eq!(key, Some("solo-key".to_string()));
    }

    #[test]
    fn resolve_api_key_is_none_when_neither_env_value_is_set() {
        let key = resolve_api_key_for_replicate(None, None, 0).unwrap();
        assert_eq!(key, None);
    }

    #[test]
    fn resolve_user_map_for_replicate_is_empty_with_no_file_given() {
        let map = resolve_user_map_for_replicate(None, 0).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn resolve_user_map_for_replicate_reads_a_flat_object_for_any_replicate() {
        let contents = r#"{"platform-lead": "507f1f77bcf86cd799439011"}"#;
        for index in [0, 1, 7] {
            let map = resolve_user_map_for_replicate(Some(contents), index).unwrap();
            assert_eq!(
                map.get("platform-lead").map(String::as_str),
                Some("507f1f77bcf86cd799439011")
            );
        }
    }

    #[test]
    fn resolve_user_map_for_replicate_picks_the_nth_entry_of_an_array() {
        let contents = r#"[{"platform-lead": "org0-id"}, {"platform-lead": "org1-id"}]"#;
        let map = resolve_user_map_for_replicate(Some(contents), 1).unwrap();
        assert_eq!(
            map.get("platform-lead").map(String::as_str),
            Some("org1-id")
        );
    }

    #[test]
    fn resolve_user_map_for_replicate_errors_when_the_index_has_no_matching_entry() {
        let contents = r#"[{"platform-lead": "org0-id"}]"#;
        let err = resolve_user_map_for_replicate(Some(contents), 1).unwrap_err();
        assert!(err.to_string().contains("1 map(s)"));
    }

    #[test]
    fn resolve_user_map_for_replicate_errors_on_invalid_json() {
        let err = resolve_user_map_for_replicate(Some("not json"), 0).unwrap_err();
        assert!(err.to_string().contains("not valid JSON"));
    }

    #[test]
    fn resolve_user_scope_id_passes_through_org_and_agent_scope_unchanged() {
        let map = HashMap::new();
        assert_eq!(
            resolve_user_scope_id(
                &Scope {
                    kind: ScopeKind::Org,
                    id: "acme".to_string()
                },
                &map
            )
            .unwrap(),
            "acme"
        );
        assert_eq!(
            resolve_user_scope_id(
                &Scope {
                    kind: ScopeKind::Agent,
                    id: "bot".to_string()
                },
                &map
            )
            .unwrap(),
            "bot"
        );
    }

    #[test]
    fn resolve_user_scope_id_maps_a_user_scope_id_through_the_table() {
        let mut map = HashMap::new();
        map.insert("platform-lead".to_string(), "real-member-id".to_string());
        assert_eq!(
            resolve_user_scope_id(
                &Scope {
                    kind: ScopeKind::User,
                    id: "platform-lead".to_string()
                },
                &map
            )
            .unwrap(),
            "real-member-id"
        );
    }

    #[test]
    fn resolve_user_scope_id_errors_when_the_actor_has_no_provisioned_member() {
        let map = HashMap::new();
        let err = resolve_user_scope_id(
            &Scope {
                kind: ScopeKind::User,
                id: "platform-lead".to_string(),
            },
            &map,
        )
        .unwrap_err();
        assert!(err.to_string().contains("platform-lead"));
    }

    fn org_event() -> StreamEvent {
        StreamEvent {
            id: "e1".to_string(),
            content: "hello".to_string(),
            timestamp: "2026-01-01T00:00:00Z".parse().unwrap(),
            actor: Actor {
                id: "a".to_string(),
                kind: ActorKind::User,
            },
            scope: Scope {
                kind: ScopeKind::Org,
                id: "acme".to_string(),
            },
            source: "test".to_string(),
            supersedes: None,
            entities: Vec::new(),
        }
    }

    // Plan 26 review: achiral.ai's normalizeDeveloperEvent() now reads
    // entityReferences (entityType/externalId/displayName) and passes it
    // to labelEvent(), which resolves it into a findable BusinessEntity -
    // without sending this, a stream's declared entities never reach the
    // real system at all, no matter what stream.rs parses.
    #[test]
    fn ingest_event_payload_carries_declared_entities_as_entity_references() {
        let mut event = org_event();
        event.entities = vec![crate::stream::EntityReference {
            external_id: "atlas".to_string(),
            display_name: "Atlas".to_string(),
            entity_type: "project".to_string(),
        }];

        let payload = ingest_event_payload(&event);
        assert_eq!(
            payload["entityReferences"],
            serde_json::json!([{
                "entityType": "project",
                "externalId": "atlas",
                "displayName": "Atlas",
            }])
        );
    }

    #[test]
    fn ingest_event_payload_omits_entity_references_when_none_declared() {
        let payload = ingest_event_payload(&org_event());
        assert_eq!(payload["entityReferences"], serde_json::json!([]));
    }

    #[test]
    fn ingest_event_payload_carries_content_as_text_and_timestamp_as_occurred_at() {
        let payload = ingest_event_payload(&org_event());
        assert_eq!(payload["text"], "hello");
        assert_eq!(payload["occurredAt"], "2026-01-01T00:00:00+00:00");
        assert_eq!(payload["metadata"]["cognoscenti_event_id"], "e1");
    }

    // achiral.ai's PublicMemoryService.ingestEvent() (Plan 25/26 review,
    // 2026-11-10) reads an explicit supersession reference from the
    // top-level `supersedes` body field, resolved within the ingesting
    // source's own scope - not from `metadata`, which it never reads for
    // this purpose. Sending it only inside metadata (as event_metadata()
    // already did) is silently inert on achiral's side.
    #[test]
    fn ingest_event_payload_carries_an_explicit_supersedes_reference_as_a_top_level_field() {
        let mut event = org_event();
        event.supersedes = Some("e1".to_string());
        let payload = ingest_event_payload(&event);
        assert_eq!(payload["supersedes"], "e1");
    }

    #[test]
    fn ingest_event_payload_omits_supersedes_when_the_event_has_none() {
        let payload = ingest_event_payload(&org_event());
        assert!(payload.get("supersedes").is_none() || payload["supersedes"].is_null());
    }

    // A supersedes reference names a prior event by achiral.ai's external-id
    // convention (payload.id/externalId/eventId, resolved via
    // BusinessEvent.source.externalId) - so this event's own id has to be
    // sent as a top-level field too, not only inside metadata (which
    // achiral.ai never reads as an external id). Without this, no event
    // Cognoscenti ingests is ever findable by id, so no supersedes
    // reference - from this adapter - could ever resolve.
    #[test]
    fn ingest_event_payload_carries_its_own_id_as_a_top_level_field() {
        let payload = ingest_event_payload(&org_event());
        assert_eq!(payload["id"], "e1");
    }

    #[test]
    fn retrieve_payload_carries_query_and_top_k_as_limit() {
        let payload = retrieve_payload("where is it", 5);
        assert_eq!(payload["query"], "where is it");
        assert_eq!(payload["limit"], 5);
    }

    // Plan 26 Phase 5: the evidence-graph retrieve call is the same
    // endpoint as a normal retrieve, with one extra opt-in flag - never
    // sent on an ordinary retrieve, so it costs no existing caller
    // anything.
    #[test]
    fn evidence_graph_payload_carries_query_and_the_opt_in_flag() {
        let payload = evidence_graph_payload("where is it", 3);
        assert_eq!(payload["query"], "where is it");
        assert_eq!(payload["includeEvidenceGraph"], true);
    }

    // Root-cause fix: without this, achiral.ai's default limit (larger
    // than top_k) let more documents into the merged result set than the
    // real retrieve_scored() call ever sees, so unresolvedGaps was
    // computed against a different, bigger pool - confirmed live: a gap
    // that correctly disappeared at limit=3 reappeared as "resolved" the
    // moment limit was omitted, because the missing entity's own (barely
    // relevant) content fit in the larger pool.
    #[test]
    fn evidence_graph_payload_carries_the_same_top_k_as_limit() {
        let payload = evidence_graph_payload("where is it", 3);
        assert_eq!(payload["limit"], 3);
    }

    // displayName, not entityId: PublicMemoryService's CueExtractor
    // assigns entityId from a Mongo ObjectId, generated at write time and
    // unknowable to a dataset author ahead of time - displayName is the
    // one stable field a dataset's own `entities[].display_name` can be
    // compared against directly.
    #[test]
    fn parse_unresolved_gap_display_names_reads_each_gap_s_display_name() {
        let response = json!({
            "evidenceGraph": {
                "unresolvedGaps": [
                    {"entityId": "64f...", "displayName": "Northwind Sync"}
                ]
            }
        });
        assert_eq!(
            parse_unresolved_gap_display_names(&response),
            vec!["Northwind Sync".to_string()]
        );
    }

    #[test]
    fn parse_unresolved_gap_display_names_is_empty_with_no_evidence_graph_in_the_response() {
        let response = json!({"data": []});
        assert_eq!(
            parse_unresolved_gap_display_names(&response),
            Vec::<String>::new()
        );
    }

    #[test]
    fn event_status_payload_carries_every_requested_business_event_id() {
        let payload = event_status_payload(&["be-1".to_string(), "be-2".to_string()]);
        assert_eq!(payload["businessEventIds"], json!(["be-1", "be-2"]));
    }

    #[test]
    fn parse_event_statuses_maps_each_business_event_id_to_its_status() {
        let response = json!({
            "data": [
                {"businessEventId": "be-1", "status": "written"},
                {"businessEventId": "be-2", "status": "pending"}
            ]
        });
        let statuses = parse_event_statuses(&response);
        assert_eq!(statuses.get("be-1").map(String::as_str), Some("written"));
        assert_eq!(statuses.get("be-2").map(String::as_str), Some("pending"));
    }

    #[test]
    fn parse_event_statuses_is_empty_with_no_data_in_the_response() {
        let response = json!({"error": {"message": "boom"}});
        assert!(parse_event_statuses(&response).is_empty());
    }

    #[test]
    fn parse_business_event_id_reads_it_from_the_ingest_response() {
        let response = json!({"object": "memory.event", "businessEventId": "be-1"});
        assert_eq!(parse_business_event_id(&response), Some("be-1".to_string()));
    }

    #[test]
    fn parse_business_event_id_is_none_when_absent() {
        let response = json!({"object": "memory.event"});
        assert_eq!(parse_business_event_id(&response), None);
    }

    // Plan 25/26 durable-write fix: the pure loop logic, tested without
    // any real HTTP call - `check` stands in for the real status request.
    #[test]
    fn poll_until_confirmed_returns_empty_immediately_with_no_ids() {
        let remaining =
            poll_until_confirmed(Vec::new(), 1000, 10, |_| panic!("must not call check"));
        assert_eq!(remaining, Vec::<String>::new());
    }

    #[test]
    fn poll_until_confirmed_stops_as_soon_as_check_reports_nothing_left() {
        let started = std::time::Instant::now();
        let remaining = poll_until_confirmed(vec!["be-1".to_string()], 5000, 10, |_| Vec::new());
        assert_eq!(remaining, Vec::<String>::new());
        // Confirmed on the very first check - must not wait anywhere near
        // the 5-second budget.
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn poll_until_confirmed_retries_until_check_reports_nothing_left() {
        let mut calls = 0;
        let remaining = poll_until_confirmed(vec!["be-1".to_string()], 5000, 10, |remaining| {
            calls += 1;
            if calls < 3 {
                remaining.to_vec()
            } else {
                Vec::new()
            }
        });
        assert_eq!(remaining, Vec::<String>::new());
        assert_eq!(calls, 3);
    }

    #[test]
    fn poll_until_confirmed_gives_up_once_max_wait_elapses() {
        let remaining = poll_until_confirmed(
            vec!["be-1".to_string(), "be-2".to_string()],
            50,
            10,
            |remaining| remaining.to_vec(), // never confirms
        );
        assert_eq!(remaining, vec!["be-1".to_string(), "be-2".to_string()]);
    }

    #[test]
    fn parse_recall_response_pairs_each_item_with_its_real_activation_score() {
        let response = json!({"data": [{"id": "m1", "score": 0.8}, {"id": "m2", "score": 0.3}]});
        assert_eq!(
            parse_recall_response(&response),
            vec![("m1".to_string(), 0.8), ("m2".to_string(), 0.3)]
        );
    }

    #[test]
    fn parse_recall_response_defaults_a_null_score_to_zero() {
        let response = json!({"data": [{"id": "m1", "score": null}]});
        assert_eq!(
            parse_recall_response(&response),
            vec![("m1".to_string(), 0.0)]
        );
    }

    #[test]
    fn parse_recall_response_is_empty_when_data_is_missing() {
        assert_eq!(
            parse_recall_response(&json!({})),
            Vec::<(String, f64)>::new()
        );
    }

    // achiral.ai's own `id` is the Weaviate document's internal UUID, not
    // the StreamEvent id this adapter sent at ingest time - gold_event_ids
    // are StreamEvent ids, so scoring against the raw UUID could never
    // match. When achiral.ai echoes back the metadata this adapter sent,
    // prefer it.
    #[test]
    fn parse_recall_response_prefers_the_original_event_id_from_metadata_over_the_raw_weaviate_id()
    {
        let response = json!({"data": [
            {"id": "weaviate-uuid-1", "score": 0.8, "metadata": {"cognoscenti_event_id": "e2"}},
        ]});
        assert_eq!(
            parse_recall_response(&response),
            vec![("e2".to_string(), 0.8)]
        );
    }

    #[test]
    fn parse_recall_response_falls_back_to_the_raw_id_when_metadata_has_none() {
        let response = json!({"data": [{"id": "weaviate-uuid-1", "score": 0.8}]});
        assert_eq!(
            parse_recall_response(&response),
            vec![("weaviate-uuid-1".to_string(), 0.8)]
        );
    }

    #[test]
    fn eval_now_header_carries_the_simulated_time_as_rfc3339() {
        let now: DateTime<Utc> = "2026-01-15T00:00:00Z".parse().unwrap();
        assert_eq!(
            eval_now_header(now),
            (
                "X-Achiral-Eval-Now",
                "2026-01-15T00:00:00+00:00".to_string()
            )
        );
    }

    #[test]
    fn ingest_url_routes_org_scope_to_the_events_endpoint() {
        assert_eq!(
            ingest_url(
                "http://unused",
                &Scope {
                    kind: ScopeKind::Org,
                    id: "acme".to_string(),
                }
            ),
            Some("http://unused/v1/memory/events".to_string())
        );
    }

    #[test]
    fn ingest_url_routes_agent_scope_to_its_own_remember_endpoint() {
        assert_eq!(
            ingest_url(
                "http://unused",
                &Scope {
                    kind: ScopeKind::Agent,
                    id: "bot".to_string(),
                }
            ),
            Some("http://unused/v1/memory/agents/bot".to_string())
        );
    }

    #[test]
    fn ingest_url_routes_user_scope_to_its_own_events_endpoint() {
        assert_eq!(
            ingest_url(
                "http://unused",
                &Scope {
                    kind: ScopeKind::User,
                    id: "sam".to_string(),
                }
            ),
            Some("http://unused/v1/memory/users/sam/events".to_string())
        );
    }

    #[test]
    fn ingest_agent_payload_carries_content_as_text() {
        let payload = ingest_agent_payload(&org_event());
        assert_eq!(payload["text"], "hello");
        // No occurredAt - achiral.ai's agent `remember` reads no timestamp
        // field at all, confirmed by reading PublicMemoryService.remember().
        assert!(payload.get("occurredAt").is_none());
        // metadata.cognoscenti_event_id, same as ingest_event_payload - the
        // id-mapping recall scoring depends on, so agent-scope ingestion
        // carries it too.
        assert_eq!(payload["metadata"]["cognoscenti_event_id"], "e1");
    }

    #[test]
    fn retrieve_url_routes_a_user_actor_to_org_scope() {
        assert_eq!(
            retrieve_url(
                "http://unused",
                &Actor {
                    id: "bob".to_string(),
                    kind: ActorKind::User,
                }
            ),
            "http://unused/v1/memory/retrieve"
        );
    }

    #[test]
    fn settle_waits_at_least_settle_ms_for_the_write_cycle_to_catch_up() {
        let mut system = AchiralHttpSystem {
            base_url: "http://unused".to_string(),
            api_key: None,
            settle_ms: 30,
            unconfirmed_business_event_ids: Vec::new(),
            user_member_ids: HashMap::new(),
        };
        let now: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();

        let started = std::time::Instant::now();
        system.settle(now);

        assert!(started.elapsed() >= std::time::Duration::from_millis(30));
    }

    #[test]
    fn settle_is_instant_when_settle_ms_is_zero_the_default() {
        let mut system = AchiralHttpSystem {
            base_url: "http://unused".to_string(),
            api_key: None,
            settle_ms: 0,
            unconfirmed_business_event_ids: Vec::new(),
            user_member_ids: HashMap::new(),
        };
        let now: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();

        let started = std::time::Instant::now();
        system.settle(now);

        assert!(started.elapsed() < std::time::Duration::from_millis(30));
    }

    #[test]
    fn declares_the_settle_capability() {
        let system = AchiralHttpSystem {
            base_url: "http://unused".to_string(),
            api_key: None,
            settle_ms: 0,
            unconfirmed_business_event_ids: Vec::new(),
            user_member_ids: HashMap::new(),
        };
        assert!(system.capabilities().settle);
    }

    #[test]
    fn retrieve_url_routes_an_agent_actor_to_its_own_scope() {
        assert_eq!(
            retrieve_url(
                "http://unused",
                &Actor {
                    id: "scout".to_string(),
                    kind: ActorKind::Agent,
                }
            ),
            "http://unused/v1/memory/agents/scout/retrieve"
        );
    }
}

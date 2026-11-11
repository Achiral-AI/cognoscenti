//! `MemorySystem` adapter calling Achiral's real public memory API
//! (`achiral.ai`'s `routes/publicMemory.js`), not Cognoscenti's generic
//! envelope (`http_stream.rs`) - every shape below was verified against
//! the actual route/service code on 2026-11-07, not guessed. Configured
//! via `COGNOSCENTI_ACHIRAL_BASE_URL` and `COGNOSCENTI_ACHIRAL_API_KEY`,
//! sent as `Authorization: Bearer <api_key>` (matching
//! `middleware/mcpTokenAuth.js`'s `acm_...` token convention).
//!
//! Scope support, updated 2026-11-09: `ScopeKind::Org` and `ScopeKind::Agent`
//! are both wired (`ingest_url`/`retrieve_url` pick the route). Org goes to
//! `POST {base}/v1/memory/events` (ingest) and `POST {base}/v1/memory/retrieve`
//! (retrieve, async triage - a 5-minute cron actually writes to Weaviate, not
//! the ingest call itself). Agent goes to `POST {base}/v1/memory/agents/{slug}`
//! (`remember`, synchronous, no triage lag, but no `occurredAt` support) and
//! `POST {base}/v1/memory/agents/{slug}/retrieve` - both verified against achiral.ai's
//! real route/service code, not guessed; the gap here was always this
//! adapter never calling them, not achiral.ai missing them. `ScopeKind::User`
//! still panics on ingest (`ingest_url` returns `None`) - achiral.ai's public
//! memory router has no user-scoped route at all, confirmed by reading it;
//! a user actor's retrieve still falls back to org-scope, same as before,
//! since there is nowhere else to send it.
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

pub struct AchiralHttpSystem {
    base_url: String,
    api_key: Option<String>,
    /// How long settle() waits for achiral.ai's asynchronous write cycle
    /// (ingest only stages a candidate; a separate cycle writes it to the
    /// store) to catch up before the next question is asked. 0 by
    /// default - a real production deployment's write cycle runs on
    /// minutes, which no settle() wait could make synchronous anyway, so
    /// this is an explicit opt-in for a benchmark deployment tuned for a
    /// fast cycle, not a default behavior change.
    settle_ms: u64,
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
        Ok(Self {
            base_url,
            api_key,
            settle_ms,
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
    })
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

/// Where to `POST` an ingest for this event's scope - `None` when no
/// verified route exists (`ScopeKind::User`: achiral.ai's public memory
/// API has no user-scoped path at all).
pub fn ingest_url(base_url: &str, scope: &Scope) -> Option<String> {
    match scope.kind {
        ScopeKind::Org => Some(format!("{base_url}/v1/memory/events")),
        // `/v1/memory/agents/:agentSlug` (`remember`), verified end to end
        // against achiral.ai's real route/service code 2026-11-09 -
        // confirmed this adapter is the only thing that was missing, not
        // achiral.ai.
        ScopeKind::Agent => Some(format!("{base_url}/v1/memory/agents/{}", scope.id)),
        ScopeKind::User => None,
    }
}

/// Where to `POST` a retrieval for this actor - the actor asking is who
/// the query is scoped to (an agent asks within its own memory; a user
/// asks within the org's). achiral.ai has no verified user-scoped
/// retrieve path either, so a user actor still goes to the org-scope
/// endpoint, same as today.
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
        let url = ingest_url(&self.base_url, &event.scope).unwrap_or_else(|| {
            panic!(
                "AchiralHttpSystem has no verified user-scope ingest path - achiral.ai's \
                 public memory API has no user-scoped route at all"
            )
        });
        let payload = match event.scope.kind {
            ScopeKind::Agent => ingest_agent_payload(event),
            ScopeKind::Org | ScopeKind::User => ingest_event_payload(event),
        };
        run_json_post(&url, self.api_key.as_deref(), &[], &payload)
            .unwrap_or_else(|err| panic!("achiral ingest failed: {err:#}"));
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
            ..Default::default()
        }
    }

    fn settle(&mut self, _now: DateTime<Utc>) {
        if self.settle_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(self.settle_ms));
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
        }
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
    fn ingest_url_is_none_for_user_scope_no_verified_route_exists() {
        assert_eq!(
            ingest_url(
                "http://unused",
                &Scope {
                    kind: ScopeKind::User,
                    id: "sam".to_string(),
                }
            ),
            None
        );
    }

    #[test]
    #[should_panic(expected = "no verified user-scope ingest path")]
    fn ingest_panics_for_a_user_scoped_event_rather_than_misrouting_it() {
        let mut system = AchiralHttpSystem {
            base_url: "http://unused".to_string(),
            api_key: None,
            settle_ms: 0,
        };
        let mut event = org_event();
        event.scope = Scope {
            kind: ScopeKind::User,
            id: "sam".to_string(),
        };
        system.ingest(&event);
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

//! Generic HTTP-backed `MemorySystem` for Cognoscenti's stream-eval path
//! (Plan 25 Showdown 1 step 6). Configured via `COGNOSCENTI_<NAME>_INGEST_URL`,
//! `_RETRIEVE_URL` and `_API_KEY` - the same env-var convention as the
//! legacy item-eval `HttpMemorySystem` (eval.rs) - extended for the stream
//! model and the `actor`-aware `retrieve` Plan 25 added. Shells out to
//! curl, same as eval.rs's `run_json_post`, so this crate adds no HTTP
//! client dependency.
//!
//! The wire contract below is Cognoscenti's own (Plan 25's Adapter
//! contract v2), not any vendor's native API - a real vendor or Achiral
//! itself needs a thin shim in front of its own endpoints translating
//! into this shape, same as any adapter. No vendor speaks this envelope
//! today; this is the adapter itself, wired to nothing yet.
//!
//! A network failure panics rather than degrading to an empty result - a
//! silently missing ingest or retrieval would corrupt a benchmark number
//! without showing an error, the exact failure mode Plan 25 exists to
//! rule out.
//!
//! Not implemented: `reset()` (Plan 25's "clean, isolated namespace per
//! run"). No live vendor exists yet to verify what that should mean over
//! HTTP, so this does not guess at it.

use crate::adapter::MemorySystem;
use crate::stream::{Actor, StreamEvent};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::process::Command;

pub struct HttpStreamSystem {
    name: String,
    ingest_url: String,
    retrieve_url: String,
    api_key: Option<String>,
}

impl HttpStreamSystem {
    pub fn from_env(name: &str) -> Result<Self> {
        let prefix = format!("COGNOSCENTI_{}", name.to_ascii_uppercase());
        let ingest_url = std::env::var(format!("{prefix}_INGEST_URL"))
            .with_context(|| format!("missing {prefix}_INGEST_URL"))?;
        let retrieve_url = std::env::var(format!("{prefix}_RETRIEVE_URL"))
            .with_context(|| format!("missing {prefix}_RETRIEVE_URL"))?;
        let api_key = std::env::var(format!("{prefix}_API_KEY")).ok();

        Ok(Self {
            name: name.to_string(),
            ingest_url,
            retrieve_url,
            api_key,
        })
    }
}

/// The ingest wire payload: Adapter contract v2's `ingest(events[])`, one
/// event per call wrapped in a single-element array.
pub fn ingest_payload(event: &StreamEvent) -> Value {
    json!({ "events": [event] })
}

/// The retrieve wire payload: Adapter contract v2's
/// `retrieve(query, actor, now, top_k)`.
pub fn retrieve_payload(query: &str, actor: &Actor, now: DateTime<Utc>, top_k: usize) -> Value {
    json!({
        "query": query,
        "actor": actor,
        "now": now.to_rfc3339(),
        "top_k": top_k,
    })
}

/// Parses `{"results": [{"id": "...", "score": ...}, ...]}`, best first.
/// An entry with no string `id` is dropped; one with no numeric `score`
/// defaults to 0.0 rather than being dropped, since a vendor's ranking
/// order (array position) is still real signal even without a score.
pub fn parse_retrieve_response(value: &Value) -> Vec<(String, f64)> {
    value
        .get("results")
        .and_then(Value::as_array)
        .map(|results| {
            results
                .iter()
                .filter_map(|entry| {
                    let id = entry.get("id")?.as_str()?.to_string();
                    let score = entry.get("score").and_then(Value::as_f64).unwrap_or(0.0);
                    Some((id, score))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Shells out to curl for a JSON POST - shared with `achiral_http`, which
/// calls achiral.ai's real public memory API over the same transport and
/// needs `extra_headers` for `X-Achiral-Eval-Now` (that API takes simulated
/// time as a header, not a body field).
pub(crate) fn run_json_post(
    url: &str,
    api_key: Option<&str>,
    extra_headers: &[(&str, String)],
    payload: &Value,
) -> Result<Value> {
    let mut command = Command::new("curl");
    command
        .arg("-sS")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json");

    if let Some(api_key) = api_key {
        command
            .arg("-H")
            .arg(format!("Authorization: Bearer {api_key}"));
    }

    for (name, value) in extra_headers {
        command.arg("-H").arg(format!("{name}: {value}"));
    }

    let output = command
        .arg("-d")
        .arg(payload.to_string())
        .arg(url)
        .output()
        .with_context(|| "failed to run curl for stream adapter request")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "stream adapter request failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "stream adapter returned non-JSON response: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

impl MemorySystem for HttpStreamSystem {
    fn name(&self) -> &str {
        &self.name
    }

    fn ingest(&mut self, event: &StreamEvent) {
        let payload = ingest_payload(event);
        run_json_post(&self.ingest_url, self.api_key.as_deref(), &[], &payload)
            .unwrap_or_else(|err| panic!("{} ingest failed: {err:#}", self.name));
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

    fn retrieve_scored(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<(String, f64)> {
        let payload = retrieve_payload(query, actor, now, top_k);
        let response = run_json_post(&self.retrieve_url, self.api_key.as_deref(), &[], &payload)
            .unwrap_or_else(|err| panic!("{} retrieve failed: {err:#}", self.name));
        parse_retrieve_response(&response)
            .into_iter()
            .take(top_k)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::{ActorKind, Scope, ScopeKind};

    fn event() -> StreamEvent {
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
    fn ingest_payload_wraps_the_event_in_a_single_element_events_array() {
        let payload = ingest_payload(&event());
        let events = payload["events"]
            .as_array()
            .expect("events must be an array");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], "e1");
        assert_eq!(events[0]["content"], "hello");
    }

    #[test]
    fn retrieve_payload_carries_query_actor_now_and_top_k() {
        let actor = Actor {
            id: "bob".to_string(),
            kind: ActorKind::Agent,
        };
        let now: DateTime<Utc> = "2026-01-15T00:00:00Z".parse().unwrap();
        let payload = retrieve_payload("where is it", &actor, now, 5);

        assert_eq!(payload["query"], "where is it");
        assert_eq!(payload["actor"]["id"], "bob");
        assert_eq!(payload["now"], "2026-01-15T00:00:00+00:00");
        assert_eq!(payload["top_k"], 5);
    }

    #[test]
    fn parse_retrieve_response_pairs_each_result_with_its_score() {
        let response = json!({
            "results": [
                {"id": "e1", "score": 0.9},
                {"id": "e2", "score": 0.4}
            ]
        });
        assert_eq!(
            parse_retrieve_response(&response),
            vec![("e1".to_string(), 0.9), ("e2".to_string(), 0.4)]
        );
    }

    #[test]
    fn parse_retrieve_response_defaults_a_missing_score_to_zero_but_keeps_the_id() {
        let response = json!({"results": [{"id": "e1"}]});
        assert_eq!(
            parse_retrieve_response(&response),
            vec![("e1".to_string(), 0.0)]
        );
    }

    #[test]
    fn parse_retrieve_response_drops_an_entry_with_no_id() {
        let response = json!({"results": [{"score": 0.9}, {"id": "e2", "score": 0.4}]});
        assert_eq!(
            parse_retrieve_response(&response),
            vec![("e2".to_string(), 0.4)]
        );
    }

    #[test]
    fn parse_retrieve_response_is_empty_for_a_response_with_no_results_key() {
        assert_eq!(
            parse_retrieve_response(&json!({})),
            Vec::<(String, f64)>::new()
        );
    }
}

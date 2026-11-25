//! The `procedural` arm of the procedural track (Plan 44 Phase 5): the
//! declarative Achiral adapter plus the team's procedures held as
//! productions, authored and read through `achiral.ai`'s
//! `routes/productions.js` (`/api/productions`).
//!
//! What the pre-registration fixes, and what this file does about it:
//! - Authoring is from the stream, not by hand (`production_from_event`): the
//!   condition is the situation phrase, the cues its words longer than three
//!   letters, the action the action phrase, the evidence the procedure event.
//!   An org-wide procedure is created with the admin's session (active at
//!   once); a private one with its member's session, member-scoped.
//! - Ranking: the productions the asker's session may see are matched
//!   against the question, conflict-resolved with noise scale 0.15 and a seed
//!   equal to the replicate index, at most three, and each selected
//!   production's first evidence event goes first, then the declarative
//!   results (`retrieve_items`; `resolve_retrieval` removes duplicates).
//!
//! The productions API has no selection route: it lists, creates, edits,
//! deletes and approves. Selection (`selectProductions` in
//! `services/procedural/Production.js`) is a function inside the server that
//! the HTTP surface never calls. So this adapter lists what the asker may see
//! and applies the same matching and conflict resolution itself, ported
//! below (`match_strength`, `mulberry32`, `select`). The port has no vector
//! similarity, because the route does not return one, so the match strength
//! is the cue overlap alone. Both are recorded as deviations in
//! `docs/procedural-track.md`.
//!
//! Auth is a NextAuth session cookie per user, not an API key. Sessions come
//! from `COGNOSCENTI_PROCEDURAL_SESSIONS_FILE` (see `Sessions`); nothing from
//! that file is written to any result.

use crate::achiral_http::AchiralHttpSystem;
use crate::adapter::{Capabilities, MemorySystem, RetrievedItem};
use crate::stream::{Actor, ScopeKind, StreamEvent};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::process::Command;

pub const RETRIEVAL_THRESHOLD: f64 = 0.5;
pub const MAX_SELECTED: usize = 3;
pub const NOISE_SCALE: f64 = 0.15;

// ------------------------------------------------------------- authoring ---

/// A production to create for one procedure event.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthoredProduction {
    pub condition_text: String,
    pub cues: Vec<String>,
    pub action_text: String,
    /// The stream event it cites.
    pub event_id: String,
    /// `None` for an org-wide rule (an admin creates it); the member's
    /// stream actor id for a private one.
    pub member: Option<String>,
}

/// Lowercased words longer than three letters, first occurrence only.
pub fn cues_of(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() > 3)
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

const PREFIXES: [&str; 3] = [
    "Procedure update: when ",
    "Procedure: when ",
    "My own procedure: when ",
];

/// The production a procedure event stands for. `None` for any event that is
/// not a procedure, so the stream's other events stay memories only.
pub fn production_from_event(event: &StreamEvent) -> Option<AuthoredProduction> {
    let rest = PREFIXES
        .iter()
        .find_map(|p| event.content.strip_prefix(p))?;
    let (condition, action) = rest.split_once(", ")?;
    let action = action.trim_end_matches('.');
    let action = action.strip_suffix(" instead").unwrap_or(action);
    let member = match event.scope.kind {
        ScopeKind::Org => None,
        ScopeKind::User => Some(event.actor.id.clone()),
        ScopeKind::Agent => return None,
    };
    Some(AuthoredProduction {
        condition_text: condition.to_string(),
        cues: cues_of(condition),
        action_text: action.to_string(),
        event_id: event.id.clone(),
        member,
    })
}

/// The body of `POST /api/productions`. The route keeps only `condition`,
/// `action`, `evidence`, `scope` (and `organizationId` for the membership
/// check).
pub fn create_body(p: &AuthoredProduction, organization_id: &str) -> Value {
    json!({
        "organizationId": organization_id,
        "condition": { "text": p.condition_text, "cues": p.cues },
        "action": { "text": p.action_text },
        "evidence": [{ "collection": "CognoscentiEvent", "weaviateId": p.event_id }],
        "scope": if p.member.is_some() { "member" } else { "org" },
    })
}

// ------------------------------------------------------------- selection ---

/// A production as `GET /api/productions` returns it, reduced to what
/// selection needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedProduction {
    pub id: String,
    pub condition_text: String,
    pub cues: Vec<String>,
    pub action_text: String,
    pub status: String,
    pub utility_score: f64,
    /// The first evidence entry's id: the stream event the rule cites.
    pub first_evidence: Option<String>,
}

pub fn parse_list(response: &Value) -> Result<Vec<ListedProduction>> {
    if response.get("success").and_then(Value::as_bool) != Some(true) {
        bail!("productions list was not a success: {response}");
    }
    let data = response
        .get("data")
        .and_then(Value::as_array)
        .context("productions list has no data array")?;
    Ok(data
        .iter()
        .filter_map(|p| {
            let text = |v: &Value| v.as_str().map(str::to_string);
            Some(ListedProduction {
                id: text(p.get("id")?)?,
                condition_text: text(p.pointer("/condition/text")?)?,
                cues: p
                    .pointer("/condition/cues")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(text).collect())
                    .unwrap_or_default(),
                action_text: text(p.pointer("/action/text")?)?,
                status: text(p.get("status")?)?,
                utility_score: p
                    .pointer("/utility/utilityScore")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.5),
                first_evidence: p
                    .pointer("/evidence/0/weaviateId")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect())
}

/// Cue overlap of a rule against the question's cues. With no vector
/// similarity available this is the whole strength (`matchStrengthOf` with one
/// part); a rule with no cues cannot match.
pub fn match_strength(production: &ListedProduction, turn_cues: &HashSet<String>) -> f64 {
    if production.cues.is_empty() {
        return 0.0;
    }
    let hit = production
        .cues
        .iter()
        .filter(|c| turn_cues.contains(*c))
        .count();
    hit as f64 / production.cues.len() as f64
}

/// `mulberry32` from `Production.js`, bit for bit.
pub fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b_79f5);
        let mut t = a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// `matchProductions` then `resolveConflict`: active rules at or above the
/// threshold, ordered by strength plus utility plus seeded logistic noise,
/// at most `limit` (never more than three). Ids are compared bytewise where
/// the server uses `localeCompare`; the two agree on the lowercase hex ids
/// Weaviate issues.
pub fn select(
    productions: &[ListedProduction],
    question: &str,
    seed: u32,
    noise_scale: f64,
    limit: usize,
) -> Vec<(ListedProduction, f64)> {
    let turn_cues: HashSet<String> = cues_of(question).into_iter().collect();
    let mut matches: Vec<(&ListedProduction, f64)> = productions
        .iter()
        .filter(|p| p.status == "active")
        .map(|p| (p, match_strength(p, &turn_cues)))
        .filter(|(_, strength)| *strength >= RETRIEVAL_THRESHOLD)
        .collect();
    matches.sort_by(|a, b| a.0.id.cmp(&b.0.id));
    let mut rand = mulberry32(seed);
    let mut scored: Vec<(ListedProduction, f64, f64)> = matches
        .into_iter()
        .map(|(p, strength)| {
            let u = rand().clamp(1e-9, 1.0 - 1e-9);
            let noise = if noise_scale > 0.0 {
                noise_scale * (u / (1.0 - u)).ln()
            } else {
                0.0
            };
            (p.clone(), strength, strength + p.utility_score + noise)
        })
        .collect();
    scored.sort_by(|a, b| b.2.total_cmp(&a.2));
    scored
        .into_iter()
        .take(limit.min(MAX_SELECTED))
        .map(|(p, strength, _)| (p, strength))
        .collect()
}

/// The text a selected production would put in a prompt.
pub fn render(p: &ListedProduction) -> String {
    format!("When {}: {}.", p.condition_text, p.action_text)
}

// ------------------------------------------------------------------- api ---

/// The two calls the arm makes. A session is one user's cookie value.
pub trait ProductionsApi {
    fn list(&mut self, session: &str) -> Result<Vec<ListedProduction>>;
    fn create(&mut self, session: &str, body: &Value) -> Result<()>;
}

/// The server's CSRF check on a write compares a cookie with a header and does
/// not issue the value, so any fixed string passes; a session cookie alone is a 403.
const CSRF_TOKEN: &str = "cognoscenti-procedural";

/// The real transport: curl against `{base}/api/productions`.
pub struct CurlProductionsApi {
    base_url: String,
    organization_id: String,
    cookie_name: &'static str,
}

impl CurlProductionsApi {
    pub fn new(base_url: &str, organization_id: &str) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        // NextAuth names the cookie by scheme; the server accepts both.
        let cookie_name = if base_url.starts_with("https://") {
            "__Secure-next-auth.session-token"
        } else {
            "next-auth.session-token"
        };
        Self {
            base_url,
            organization_id: organization_id.to_string(),
            cookie_name,
        }
    }

    fn call(&self, method: &str, url: &str, session: &str, body: Option<&Value>) -> Result<Value> {
        let mut cmd = Command::new("curl");
        cmd.args(["-sS", "-X", method, "-H", "Content-Type: application/json"])
            .arg("-H")
            .arg(format!(
                "Cookie: {}={session}; csrf-token={CSRF_TOKEN}",
                self.cookie_name
            ))
            .args(["-H", &format!("x-csrf-token: {CSRF_TOKEN}")])
            .args(["-w", "\n%{http_code}"]);
        if let Some(body) = body {
            cmd.arg("-d").arg(body.to_string());
        }
        let out = cmd.arg(url).output().context("failed to run curl")?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            bail!(
                "productions request failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let (text, status) = stdout.rsplit_once('\n').unwrap_or((&stdout, "0"));
        let status: u16 = status.trim().parse().unwrap_or(0);
        let value: Value = serde_json::from_str(text)
            .with_context(|| format!("productions API returned non-JSON (status {status})"))?;
        if !(200..300).contains(&status) {
            let hint = if status == 404 {
                " (aiConfig.proceduralMemory is off for this organization)"
            } else {
                ""
            };
            bail!("{method} {url} answered {status}{hint}: {value}");
        }
        Ok(value)
    }
}

impl ProductionsApi for CurlProductionsApi {
    fn list(&mut self, session: &str) -> Result<Vec<ListedProduction>> {
        let url = format!(
            "{}/api/productions?organizationId={}",
            self.base_url, self.organization_id
        );
        parse_list(&self.call("GET", &url, session, None)?)
    }

    fn create(&mut self, session: &str, body: &Value) -> Result<()> {
        let url = format!("{}/api/productions", self.base_url);
        self.call("POST", &url, session, Some(body))?;
        Ok(())
    }
}

// -------------------------------------------------------------- sessions ---

/// One organisation's sessions: the admin who authors org-wide rules and each
/// stream user as a member of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sessions {
    pub organization_id: String,
    pub admin: String,
    pub members: HashMap<String, String>,
}

impl Sessions {
    /// `{"organization_id", "admin", "members": {"ana": "..."}}` for every
    /// replicate, or an array of them, one per replicate (each replicate is a
    /// fresh organisation).
    pub fn for_replicate(file: &str, index: usize) -> Result<Self> {
        let value: Value = serde_json::from_str(file).context("sessions file is not JSON")?;
        let entry = match &value {
            Value::Array(items) => items.get(index).with_context(|| {
                format!(
                    "sessions file has {} entries, none for replicate {index}",
                    items.len()
                )
            })?,
            other => other,
        };
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .with_context(|| format!("sessions entry has no {key}"))
        };
        let members = entry
            .get("members")
            .and_then(Value::as_object)
            .context("sessions entry has no members")?
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect();
        Ok(Self {
            organization_id: text("organization_id")?,
            admin: text("admin")?,
            members,
        })
    }

    fn of(&self, user: &str) -> Result<&str> {
        self.members
            .get(user)
            .map(String::as_str)
            .with_context(|| format!("no session for user {user:?}"))
    }
}

// ---------------------------------------------------------------- system ---

pub struct ProductionsHttpSystem {
    inner: Box<dyn MemorySystem>,
    api: Box<dyn ProductionsApi>,
    sessions: Sessions,
    seed: u32,
    /// What each session last listed. Nothing else writes to the organisation
    /// during a replay, so a listing holds until this arm authors a rule; asking
    /// per question runs into the server's request limit.
    listed: HashMap<String, Vec<ListedProduction>>,
    /// `NOISE_SCALE`; tests set it to 0 to see the matching alone.
    pub noise_scale: f64,
}

impl ProductionsHttpSystem {
    /// `seed` is the replicate index. Fails when the organisation has the
    /// feature off, before anything is ingested.
    pub fn new(
        inner: Box<dyn MemorySystem>,
        mut api: Box<dyn ProductionsApi>,
        sessions: Sessions,
        seed: u32,
    ) -> Result<Self> {
        api.list(&sessions.admin)
            .context("the productions API is not reachable for the admin session")?;
        Ok(Self {
            inner,
            api,
            sessions,
            seed,
            listed: HashMap::new(),
            noise_scale: NOISE_SCALE,
        })
    }

    /// The arm as configured by the environment, for replicate `index`:
    /// `COGNOSCENTI_ACHIRAL_*` for the declarative half (see `achiral_http`),
    /// `COGNOSCENTI_PROCEDURAL_BASE_URL` (the app origin that serves
    /// `/api/productions`) and `COGNOSCENTI_PROCEDURAL_SESSIONS_FILE`.
    pub fn from_env_at(index: usize) -> Result<Self> {
        let base = std::env::var("COGNOSCENTI_PROCEDURAL_BASE_URL")
            .context("missing COGNOSCENTI_PROCEDURAL_BASE_URL")?;
        let path = std::env::var("COGNOSCENTI_PROCEDURAL_SESSIONS_FILE")
            .context("missing COGNOSCENTI_PROCEDURAL_SESSIONS_FILE")?;
        let file = std::fs::read_to_string(&path)
            .with_context(|| format!("reading COGNOSCENTI_PROCEDURAL_SESSIONS_FILE at {path}"))?;
        let sessions = Sessions::for_replicate(&file, index)?;
        let api = CurlProductionsApi::new(&base, &sessions.organization_id);
        Self::new(
            Box::new(SkipMemberScope::new(Box::new(
                AchiralHttpSystem::from_env_at(index)?,
            ))),
            Box::new(api),
            sessions,
            index as u32,
        )
    }

    fn author(&mut self, event: &StreamEvent) {
        let Some(production) = production_from_event(event) else {
            return;
        };
        let session = match &production.member {
            None => Ok(self.sessions.admin.as_str()),
            Some(member) => self.sessions.of(member),
        }
        .unwrap_or_else(|err| panic!("{err:#}"))
        .to_string();
        let body = create_body(&production, &self.sessions.organization_id);
        self.api.create(&session, &body).unwrap_or_else(|err| {
            panic!("creating the production for {} failed: {err:#}", event.id)
        });
        self.listed.clear();
    }
}

impl MemorySystem for ProductionsHttpSystem {
    fn name(&self) -> &str {
        "achiral-productions"
    }

    fn ingest(&mut self, event: &StreamEvent) {
        self.inner.ingest(event);
        self.author(event);
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
        self.retrieve_items(query, actor, now, top_k)
            .into_iter()
            .flat_map(|item| item.sources.into_iter().map(move |s| (s, item.score)))
            .collect()
    }

    fn retrieve_items(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<RetrievedItem> {
        let session = self
            .sessions
            .of(&actor.id)
            .unwrap_or_else(|err| panic!("{err:#}"))
            .to_string();
        if !self.listed.contains_key(&session) {
            let rules = self.api.list(&session).unwrap_or_else(|err| {
                panic!("listing productions for {} failed: {err:#}", actor.id)
            });
            self.listed.insert(session.clone(), rules);
        }
        let listed = &self.listed[&session];
        let mut items: Vec<RetrievedItem> =
            select(listed, query, self.seed, self.noise_scale, MAX_SELECTED)
                .into_iter()
                .map(|(p, strength)| RetrievedItem {
                    // A production with no evidence retrieves nothing.
                    sources: p.first_evidence.iter().cloned().collect(),
                    score: strength + p.utility_score,
                    text: Some(render(&p)),
                })
                .collect();
        items.extend(self.inner.retrieve_items(query, actor, now, top_k));
        items
    }

    fn retrieval_echo(&self) -> Option<Value> {
        self.inner.retrieval_echo()
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    fn settle(&mut self, now: DateTime<Utc>) {
        self.inner.settle(now);
    }

    fn use_feedback(&mut self, memory_ids: &[String], helpful: bool) {
        self.inner.use_feedback(memory_ids, helpful);
    }
}

/// Passes everything to the memory system except events scoped to a member.
/// achiral.ai's public memory API writes a member's private memory only with
/// a credential bound to that one member (Plan 41), and the adapter holds one
/// key per organisation, so a run against the public API leaves those events
/// out of the declarative half. Productions for them are still authored.
pub struct SkipMemberScope {
    inner: Box<dyn MemorySystem>,
    pub skipped: usize,
}

impl SkipMemberScope {
    pub fn new(inner: Box<dyn MemorySystem>) -> Self {
        Self { inner, skipped: 0 }
    }
}

impl MemorySystem for SkipMemberScope {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn ingest(&mut self, event: &StreamEvent) {
        if event.scope.kind == ScopeKind::User {
            self.skipped += 1;
        } else {
            self.inner.ingest(event);
        }
    }

    fn retrieve(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<String> {
        self.inner.retrieve(query, actor, now, top_k)
    }

    fn retrieve_scored(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<(String, f64)> {
        self.inner.retrieve_scored(query, actor, now, top_k)
    }

    fn retrieve_items(
        &mut self,
        query: &str,
        actor: &Actor,
        now: DateTime<Utc>,
        top_k: usize,
    ) -> Vec<RetrievedItem> {
        self.inner.retrieve_items(query, actor, now, top_k)
    }

    fn retrieval_echo(&self) -> Option<Value> {
        self.inner.retrieval_echo()
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    fn settle(&mut self, now: DateTime<Utc>) {
        self.inner.settle(now);
    }

    fn use_feedback(&mut self, memory_ids: &[String], helpful: bool) {
        self.inner.use_feedback(memory_ids, helpful);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::adapter::replay_stream;
    use crate::baselines::Bm25System;
    use crate::procedural::{
        generate_procedural, score_procedural, ProceduralConfig, Tally, REPORT_SEED, TUNING_SEED,
    };
    use crate::stream::{ActorKind, Scope};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn event(id: &str, content: &str, scope: ScopeKind, author: &str) -> StreamEvent {
        StreamEvent {
            id: id.to_string(),
            content: content.to_string(),
            timestamp: "2026-01-01T00:00:00Z".parse().unwrap(),
            actor: Actor {
                id: author.to_string(),
                kind: ActorKind::User,
            },
            scope: Scope {
                kind: scope,
                id: if scope == ScopeKind::User {
                    author.to_string()
                } else {
                    "generated".to_string()
                },
            },
            source: "docs".to_string(),
            supersedes: None,
            entities: Vec::new(),
        }
    }

    #[test]
    fn mulberry32_matches_the_values_production_js_gives() {
        // From the function in services/procedural/Production.js run under node.
        for (seed, expect) in [
            (0, [0.266429208685, 0.000329745701, 0.223272027448]),
            (1, [0.627073940588, 0.002735721180, 0.527447039960]),
            (4242, [0.546706133522, 0.278608789202, 0.931236917153]),
        ] {
            let mut rand = mulberry32(seed);
            for want in expect {
                assert!((rand() - want).abs() < 1e-11, "seed {seed}");
            }
        }
    }

    #[test]
    fn cues_are_lowercased_words_over_three_letters_without_repeats() {
        assert_eq!(
            cues_of("an annual-plan customer raises billing"),
            ["annual", "plan", "customer", "raises", "billing"]
        );
        assert_eq!(cues_of("A big BIG data data export"), ["data", "export"]);
    }

    #[test]
    fn an_org_procedure_event_becomes_an_org_production_citing_the_event() {
        let e = event(
            "proc-03",
            "Procedure: when a trial user raises a refund, notify legal before replying.",
            ScopeKind::Org,
            "dev",
        );
        let p = production_from_event(&e).unwrap();
        assert_eq!(p.condition_text, "a trial user raises a refund");
        assert_eq!(p.action_text, "notify legal before replying");
        assert_eq!(p.cues, ["trial", "user", "raises", "refund"]);
        assert_eq!(
            (p.event_id.as_str(), p.member.as_deref()),
            ("proc-03", None)
        );
        let body = create_body(&p, "org1");
        assert_eq!(body["scope"], "org");
        assert_eq!(body["organizationId"], "org1");
        assert_eq!(body["evidence"][0]["weaviateId"], "proc-03");
        assert!(body.get("audience").is_none() && body.get("status").is_none());
    }

    #[test]
    fn a_replacement_keeps_the_condition_and_drops_the_instead() {
        let e = event(
            "proc-00-v2",
            "Procedure update: when a trial user raises a refund, copy finance before replying instead.",
            ScopeKind::Org,
            "ana",
        );
        let p = production_from_event(&e).unwrap();
        assert_eq!(p.condition_text, "a trial user raises a refund");
        assert_eq!(p.action_text, "copy finance before replying");
    }

    #[test]
    fn a_private_procedure_is_member_scoped_to_its_author() {
        let e = event(
            "proc-priv-00",
            "My own procedure: when a student raises onboarding, hand off to legal before replying.",
            ScopeKind::User,
            "ana",
        );
        let p = production_from_event(&e).unwrap();
        assert_eq!(p.member.as_deref(), Some("ana"));
        assert_eq!(create_body(&p, "o")["scope"], "member");
    }

    #[test]
    fn an_event_that_is_not_a_procedure_authors_nothing() {
        let e = event("m1", "The launch moved to Friday.", ScopeKind::Org, "ana");
        assert_eq!(production_from_event(&e), None);
    }

    fn listed(id: &str, cues: &[&str], status: &str, utility: f64) -> ListedProduction {
        ListedProduction {
            id: id.to_string(),
            condition_text: cues.join(" "),
            cues: cues.iter().map(|c| c.to_string()).collect(),
            action_text: "do it".to_string(),
            status: status.to_string(),
            utility_score: utility,
            first_evidence: Some(format!("ev-{id}")),
        }
    }

    #[test]
    fn selection_needs_half_the_cues_and_an_active_rule() {
        let rules = vec![
            listed("a", &["trial", "user", "raises", "refund"], "active", 0.5),
            listed(
                "b",
                &["trial", "user", "raises", "refund"],
                "superseded",
                0.5,
            ),
            listed("c", &["student", "onboarding"], "active", 0.5),
        ];
        // a: 3 of 4 cues; c: 0 of 2; b is retired.
        let got = select(&rules, "A trial user raises refund", 0, 0.0, 3);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.id, "a");
        // Exactly half is a match (threshold 0.5, inclusive).
        let half = select(&rules, "trial user", 0, 0.0, 3);
        assert_eq!(half.len(), 1);
        assert!((half[0].1 - 0.5).abs() < 1e-12);
        assert!(select(&rules, "trial", 0, 0.0, 3).is_empty());
    }

    #[test]
    fn without_noise_the_stronger_match_wins_and_ties_break_by_id() {
        let rules = vec![
            listed("z", &["alpha", "beta"], "active", 0.5),
            listed("m", &["alpha", "beta"], "active", 0.5),
            listed("q", &["alpha", "gamma"], "active", 0.5),
        ];
        // z and m match fully, q at half. Equal scores keep id order (stable sort).
        let got = select(&rules, "alpha beta", 0, 0.0, 3);
        assert_eq!(
            got.iter().map(|(p, _)| p.id.as_str()).collect::<Vec<_>>(),
            ["m", "z", "q"]
        );
        assert_eq!(select(&rules, "alpha beta", 0, 0.0, 1).len(), 1);
    }

    #[test]
    fn noise_follows_the_seeded_draws_in_id_order() {
        let rules = vec![
            listed("a", &["alpha"], "active", 0.5),
            listed("b", &["alpha"], "active", 0.5),
        ];
        // Seed 0 draws 0.2664 then 0.0003: a gets a small negative noise, b a
        // large one, so a ranks above b; the draw order is by id, not by rule order.
        let got = select(&rules, "alpha", 0, NOISE_SCALE, 3);
        assert_eq!(got[0].0.id, "a");
        let reversed: Vec<_> = rules.iter().rev().cloned().collect();
        assert_eq!(select(&reversed, "alpha", 0, NOISE_SCALE, 3)[0].0.id, "a");
        assert_eq!(select(&rules, "alpha", 0, NOISE_SCALE, 3), got);
    }

    #[test]
    fn parse_list_reads_the_route_shape_and_rejects_a_failure() {
        let response = json!({"success": true, "data": [{
            "id": "p1",
            "condition": {"text": "a b", "cues": ["a1", "b2"]},
            "action": {"text": "do"},
            "evidence": [{"collection": "CognoscentiEvent", "weaviateId": "proc-00"}],
            "status": "active",
            "utility": {"utilityScore": 0.5}
        }]});
        let parsed = parse_list(&response).unwrap();
        assert_eq!(parsed[0].first_evidence.as_deref(), Some("proc-00"));
        assert_eq!(parsed[0].cues, ["a1", "b2"]);
        assert!(parse_list(&json!({"success": false, "error": "Not found"})).is_err());
    }

    #[test]
    fn sessions_pick_the_entry_for_the_replicate() {
        let one = r#"{"organization_id":"o","admin":"A","members":{"ana":"X"}}"#;
        assert_eq!(Sessions::for_replicate(one, 3).unwrap().admin, "A");
        let many = r#"[{"organization_id":"o0","admin":"A","members":{}},
                       {"organization_id":"o1","admin":"B","members":{}}]"#;
        assert_eq!(
            Sessions::for_replicate(many, 1).unwrap().organization_id,
            "o1"
        );
        assert!(Sessions::for_replicate(many, 2).is_err());
        assert!(Sessions::for_replicate(r#"{"admin":"A","members":{}}"#, 0).is_err());
    }

    // ---- an in-memory productions server with the route's rules ----------

    #[derive(Default)]
    pub(crate) struct ServerState {
        pub rows: Vec<(String, Vec<String>, Value, String)>, // id, audience, body, status
        pub calls: Vec<String>,
    }

    /// Session `admin` is the org admin; any other session is the user it names.
    pub(crate) struct FakeProductions(pub Rc<RefCell<ServerState>>, pub bool);

    impl ProductionsApi for FakeProductions {
        fn list(&mut self, session: &str) -> Result<Vec<ListedProduction>> {
            let mut state = self.0.borrow_mut();
            state.calls.push(format!("list:{session}"));
            if !self.1 {
                bail!("404 Not found");
            }
            let data: Vec<Value> = state
                .rows
                .iter()
                .filter(|(_, audience, _, _)| audience.iter().any(|a| a == "org" || a == session))
                .map(|(id, _, body, status)| {
                    json!({
                        "id": id,
                        "condition": body["condition"],
                        "action": body["action"],
                        "evidence": body["evidence"],
                        "status": status,
                        "utility": {"utilityScore": 0.5}
                    })
                })
                .collect();
            parse_list(&json!({"success": true, "data": data}))
        }

        fn create(&mut self, session: &str, body: &Value) -> Result<()> {
            let mut state = self.0.borrow_mut();
            state
                .calls
                .push(format!("create:{session}:{}", body["scope"]));
            let audience = if body["scope"] == "org" {
                if session != "admin" {
                    bail!("org-wide rules wait for an admin");
                }
                vec!["org".to_string()]
            } else {
                vec![session.to_string()]
            };
            let mut cues: Vec<_> = body["condition"]["cues"].as_array().unwrap().clone();
            cues.sort_by_key(|c| c.to_string());
            for row in state.rows.iter_mut() {
                let mut old: Vec<_> = row.2["condition"]["cues"].as_array().unwrap().clone();
                old.sort_by_key(|c| c.to_string());
                if row.3 == "active" && row.1 == audience && old == cues {
                    row.3 = "superseded".to_string();
                }
            }
            let id = format!("p{:02}", state.rows.len());
            state
                .rows
                .push((id, audience, body.clone(), "active".to_string()));
            Ok(())
        }
    }

    fn users() -> HashMap<String, String> {
        ["ana", "ben", "chi", "dev"]
            .iter()
            .map(|u| (u.to_string(), u.to_string()))
            .collect()
    }

    fn system(
        inner: Box<dyn MemorySystem>,
        server: &Rc<RefCell<ServerState>>,
    ) -> ProductionsHttpSystem {
        ProductionsHttpSystem::new(
            inner,
            Box::new(FakeProductions(server.clone(), true)),
            Sessions {
                organization_id: "o".to_string(),
                admin: "admin".to_string(),
                members: users(),
            },
            0,
        )
        .unwrap()
    }

    #[test]
    fn the_arm_refuses_to_start_when_the_feature_is_off() {
        let server = Rc::new(RefCell::new(ServerState::default()));
        let err = ProductionsHttpSystem::new(
            Box::new(Bm25System::new()),
            Box::new(FakeProductions(server, false)),
            Sessions {
                organization_id: "o".to_string(),
                admin: "admin".to_string(),
                members: users(),
            },
            0,
        )
        .err()
        .unwrap();
        assert!(format!("{err:#}").contains("not reachable"));
    }

    fn replay(seed: u64, noise_scale: f64) -> crate::procedural::ProceduralScores {
        let track = generate_procedural(&ProceduralConfig::new(seed));
        let server = Rc::new(RefCell::new(ServerState::default()));
        let mut arm = system(Box::new(Bm25System::new()), &server);
        arm.noise_scale = noise_scale;
        let results = replay_stream(&track.stream, &mut arm, 1);
        score_procedural(&track.stream, &track.spec, &results)
    }

    #[test]
    fn with_no_noise_the_arm_follows_every_procedure_and_leaks_nothing() {
        for seed in [TUNING_SEED, REPORT_SEED] {
            let scores = replay(seed, 0.0);
            assert_eq!(scores.adherence, Tally { hits: 60, n: 60 }, "seed {seed}");
            assert_eq!(scores.scope_leaks, Tally { hits: 0, n: 4 }, "seed {seed}");
            assert_eq!(
                scores.superseded_rule,
                Tally { hits: 0, n: 4 },
                "seed {seed}"
            );
            assert_eq!(scores.near_miss_firing.n, 40);
            // Tokens are the rendered production, not the event's whole content.
            assert!(scores.tokens_per_question.unwrap() > 0.0);
        }
    }

    #[test]
    fn with_the_registered_noise_scope_still_holds_whatever_the_ranking_does() {
        for seed in [TUNING_SEED, REPORT_SEED, 4243] {
            let scores = replay(seed, NOISE_SCALE);
            assert_eq!(scores.scope_leaks.hits, 0, "seed {seed}");
            assert_eq!(scores.adherence.n, 60);
            // Noise lets a rule that shares the qualifier outrank the right one
            // at top-1, so adherence is below the noiseless ceiling.
            assert!(scores.adherence.hits <= 60);
        }
    }

    #[test]
    fn rules_are_listed_once_per_asker_until_one_is_authored() {
        let server = Rc::new(RefCell::new(ServerState::default()));
        let mut arm = system(Box::new(Bm25System::new()), &server);
        let asker = Actor {
            id: "ben".to_string(),
            kind: ActorKind::User,
        };
        for _ in 0..3 {
            arm.retrieve_items("A trial user raises a refund.", &asker, Utc::now(), 1);
        }
        let listed = |s: &Rc<RefCell<ServerState>>| {
            s.borrow().calls.iter().filter(|c| *c == "list:ben").count()
        };
        assert_eq!(listed(&server), 1);
        arm.ingest(&event(
            "proc-new",
            "Procedure: when a trial user raises a refund, notify legal before replying.",
            ScopeKind::Org,
            "ana",
        ));
        arm.retrieve_items("A trial user raises a refund.", &asker, Utc::now(), 1);
        assert_eq!(listed(&server), 2);
    }

    #[test]
    fn org_rules_go_through_the_admin_and_private_ones_through_their_member() {
        let track = generate_procedural(&ProceduralConfig::new(TUNING_SEED));
        let server = Rc::new(RefCell::new(ServerState::default()));
        let mut arm = system(Box::new(Bm25System::new()), &server);
        replay_stream(&track.stream, &mut arm, 1);
        let calls = server.borrow().calls.clone();
        let creates: Vec<_> = calls.iter().filter(|c| c.starts_with("create:")).collect();
        // 20 procedures, 4 replacements, 4 private.
        assert_eq!(creates.len(), 28);
        assert_eq!(
            creates
                .iter()
                .filter(|c| c.starts_with("create:admin:"))
                .count(),
            24
        );
        assert_eq!(
            creates.iter().filter(|c| c.contains("\"member\"")).count(),
            4
        );
        // Every question listed with the asker's own session.
        assert!(calls.iter().any(|c| c == "list:ben"));
        let superseded = server
            .borrow()
            .rows
            .iter()
            .filter(|r| r.3 == "superseded")
            .count();
        assert_eq!(superseded, 4);
    }

    #[test]
    fn a_production_is_ranked_ahead_of_the_declarative_results() {
        let server = Rc::new(RefCell::new(ServerState::default()));
        let mut arm = system(Box::new(Bm25System::new()), &server);
        let proc_event = event(
            "proc-00",
            "Procedure: when a trial user raises a refund, notify legal before replying.",
            ScopeKind::Org,
            "ana",
        );
        let decoy = event(
            "decoy",
            "A trial user raises a refund request about the trial user refund.",
            ScopeKind::Org,
            "ana",
        );
        arm.ingest(&decoy);
        arm.ingest(&proc_event);
        let asker = Actor {
            id: "ben".to_string(),
            kind: ActorKind::User,
        };
        let items = arm.retrieve_items(
            "A trial user raises a refund. What do we do?",
            &asker,
            Utc::now(),
            1,
        );
        assert_eq!(items[0].sources, ["proc-00"]);
        assert_eq!(
            items[0].text.as_deref(),
            Some("When a trial user raises a refund: notify legal before replying.")
        );
    }

    /// A one-shot local HTTP server on an ephemeral port: answers each request
    /// with the next canned (status, body) and returns the raw requests.
    fn mock_server(replies: Vec<(u16, String)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).into_owned();
                    if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                        let want = head
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if rest.len() >= want {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                seen.push(String::from_utf8_lossy(&raw).into_owned());
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(reply.as_bytes()).unwrap();
            }
            seen
        });
        (base, handle)
    }

    #[test]
    fn the_curl_transport_sends_the_session_cookie_and_the_route_shapes() {
        let list = json!({"success": true, "data": [{
            "id": "p1", "condition": {"text": "a b", "cues": ["alpha"]},
            "action": {"text": "do"}, "evidence": [{"weaviateId": "proc-00"}],
            "status": "active", "utility": {"utilityScore": 0.5}
        }]})
        .to_string();
        let (base, server) = mock_server(vec![
            (200, list),
            (201, r#"{"success":true,"data":{}}"#.to_string()),
        ]);
        let mut api = CurlProductionsApi::new(&base, "org-1");
        let rules = api.list("tok-ben").unwrap();
        assert_eq!(rules[0].first_evidence.as_deref(), Some("proc-00"));
        let production = production_from_event(&event(
            "proc-01",
            "Procedure: when a trial user raises a refund, notify legal before replying.",
            ScopeKind::Org,
            "ana",
        ))
        .unwrap();
        api.create("tok-admin", &create_body(&production, "org-1"))
            .unwrap();
        let seen = server.join().unwrap();
        assert!(seen[0].starts_with("GET /api/productions?organizationId=org-1 "));
        assert!(seen[0].contains("next-auth.session-token=tok-ben"));
        assert!(seen[1].starts_with("POST /api/productions "));
        assert!(seen[1].contains("next-auth.session-token=tok-admin"));
        assert!(seen[1].contains("\"scope\":\"org\""));
        // The server's CSRF check is a double submit: the cookie and the header must match.
        assert!(seen[1].contains("csrf-token=cognoscenti-procedural"));
        assert!(seen[1]
            .to_ascii_lowercase()
            .contains("x-csrf-token: cognoscenti-procedural"));
    }

    #[test]
    fn member_scope_events_are_left_out_of_the_memory_half_only() {
        let mut system = SkipMemberScope::new(Box::new(Bm25System::new()));
        let public = event(
            "pub-1",
            "Procedure: refund escalations go to legal.",
            ScopeKind::Org,
            "ana",
        );
        let private = event(
            "priv-1",
            "Procedure: my own refund escalations go to legal.",
            ScopeKind::User,
            "ben",
        );
        system.ingest(&public);
        system.ingest(&private);
        let found = system.retrieve(
            "refund escalations legal",
            &Actor {
                id: "chi".to_string(),
                kind: ActorKind::User,
            },
            Utc::now(),
            5,
        );
        assert_eq!(found, vec!["pub-1".to_string()]);
        assert_eq!(system.skipped, 1);
    }

    #[test]
    fn a_404_from_the_route_says_the_feature_is_off() {
        let (base, server) = mock_server(vec![(
            404,
            r#"{"success":false,"error":"Not found"}"#.to_string(),
        )]);
        let err = CurlProductionsApi::new(&base, "o").list("t").unwrap_err();
        server.join().unwrap();
        assert!(format!("{err:#}").contains("proceduralMemory is off"));
    }
}

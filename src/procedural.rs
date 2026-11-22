//! The procedural track (Plan 44 Phase 5): does a system apply the team's
//! procedure when the situation calls for it, and only then? A fixed-size
//! stream and its scoring, offline against a test deployment, never
//! production. No model in the loop.
//!
//! Four kinds of question, told apart by id prefix:
//! - `proc-follow-*`: the situation a procedure covers, reworded. Gold is the
//!   procedure event.
//! - `proc-near-*`: a neighbouring situation the procedure does not cover. No
//!   gold; returning the procedure is a near-miss firing.
//! - `proc-sup-*`: asked after a procedure was replaced. Gold is the new
//!   procedure; the old one winning is a superseded-rule hit.
//! - `proc-leak-*`: a member's private procedure, asked by someone else.
//!   Returning it is a scope leak.

use crate::adapter::QuestionResult;
use crate::determinism::seeded_rng;
use crate::scoring::is_authorized;
use crate::stream::{
    Actor, ActorKind, Scope, ScopeKind, Stream, StreamEvent, StreamItem, StreamQuestion,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use rand::seq::SliceRandom;
use std::collections::HashMap;

pub const TUNING_SEED: u64 = 17;
pub const REPORT_SEED: u64 = 4242;

const QUALIFIERS: [&str; 8] = [
    "an annual-plan customer",
    "an enterprise customer",
    "a trial user",
    "a partner",
    "a reseller",
    "a nonprofit",
    "a student",
    "a government buyer",
];
const TOPICS: [&str; 8] = [
    "billing",
    "a data export",
    "an outage",
    "a security question",
    "a refund",
    "onboarding",
    "a contract change",
    "an integration",
];
const VERBS: [&str; 5] = [
    "loop in",
    "hand off to",
    "notify",
    "ask for sign-off from",
    "copy",
];
const TEAMS: [&str; 4] = ["finance", "legal", "security", "support leads"];
const USERS: [&str; 4] = ["ana", "ben", "chi", "dev"];

#[derive(Debug, Clone)]
pub struct ProceduralConfig {
    pub seed: u64,
    pub procedures: usize,
    pub follow_per: usize,
    pub near_per: usize,
    pub superseded: usize,
    pub private: usize,
}

impl ProceduralConfig {
    /// The sizes the specification fixes.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            procedures: 20,
            follow_per: 3,
            near_per: 2,
            superseded: 4,
            private: 4,
        }
    }
}

/// What scoring needs beyond the stream: which procedure each near-miss
/// question must not retrieve.
#[derive(Debug, Clone, Default)]
pub struct ProceduralSpec {
    pub distractors: HashMap<String, String>,
}

pub struct ProceduralStream {
    pub stream: Stream,
    pub spec: ProceduralSpec,
}

pub fn category_of(question_id: &str) -> Option<&'static str> {
    ["follow", "near", "sup", "leak"]
        .into_iter()
        .find(|c| question_id.starts_with(&format!("proc-{c}-")))
}

fn situation(q: &str, t: &str, variant: usize) -> String {
    match variant % 3 {
        0 => format!("{} raises {t}. What is our procedure?", capitalise(q)),
        1 => format!("What do we do when {q} brings up {t}?"),
        _ => format!(
            "{} is asking about {t}: how do we handle it?",
            capitalise(q)
        ),
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn user(id: &str) -> Actor {
    Actor {
        id: id.to_string(),
        kind: ActorKind::User,
    }
}

fn org() -> Scope {
    Scope {
        kind: ScopeKind::Org,
        id: "generated".to_string(),
    }
}

fn day(start: DateTime<Utc>, d: i64, hour: i64) -> DateTime<Utc> {
    start + Duration::days(d) + Duration::hours(hour)
}

pub fn generate_procedural(config: &ProceduralConfig) -> ProceduralStream {
    let mut rng = seeded_rng(config.seed);
    let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();

    let mut combos: Vec<(usize, usize)> = (0..QUALIFIERS.len())
        .flat_map(|q| (0..TOPICS.len()).map(move |t| (q, t)))
        .collect();
    combos.shuffle(&mut rng);
    let used = config.procedures + config.private;
    let (taken, spare) = combos.split_at(used);
    // A neighbour shares the qualifier or the topic but is not a procedure.
    let neighbour = |(q, t): (usize, usize), n: usize| -> (usize, usize) {
        let near: Vec<_> = spare
            .iter()
            .copied()
            .filter(|&(sq, st)| (sq == q) != (st == t))
            .collect();
        let pool = if near.is_empty() {
            spare.to_vec()
        } else {
            near
        };
        pool[n % pool.len()]
    };

    let action = |i: usize, shift: usize| {
        format!(
            "{} {} before replying",
            VERBS[(i + shift) % VERBS.len()],
            TEAMS[(i / VERBS.len() + shift) % TEAMS.len()]
        )
    };
    let mut items: Vec<StreamItem> = Vec::new();
    let mut spec = ProceduralSpec::default();
    let event = |id: String, content: String, at, author: &str, scope, supersedes| {
        StreamItem::Event(StreamEvent {
            id,
            content,
            timestamp: at,
            actor: user(author),
            scope,
            source: "docs".to_string(),
            supersedes,
            entities: Vec::new(),
        })
    };
    let question = |id: String, query: String, at, asker: &str, gold: Vec<String>| {
        StreamItem::Question(StreamQuestion {
            id,
            query,
            asked_at: at,
            asked_by: user(asker),
            gold_event_ids: gold,
            gold_answer: None,
            validity: None,
            expected_gap_entity_ids: Vec::new(),
        })
    };

    for i in 0..config.procedures {
        let (q, t) = taken[i];
        let (qs, ts) = (QUALIFIERS[q], TOPICS[t]);
        let id = format!("proc-{i:02}");
        items.push(event(
            id.clone(),
            format!("Procedure: when {qs} raises {ts}, {}.", action(i, 0)),
            day(start, i as i64, 8),
            USERS[i % USERS.len()],
            org(),
            None,
        ));
        for k in 0..config.follow_per {
            items.push(question(
                format!("proc-follow-{i:02}-{k}"),
                situation(qs, ts, k),
                day(start, 30 + ((i * config.follow_per + k) % 9) as i64, 10),
                USERS[(i + k) % USERS.len()],
                vec![id.clone()],
            ));
        }
        let (nq, nt) = neighbour((q, t), i);
        for k in 0..config.near_per {
            let qid = format!("proc-near-{i:02}-{k}");
            spec.distractors.insert(qid.clone(), id.clone());
            items.push(question(
                qid,
                situation(QUALIFIERS[nq], TOPICS[nt], k),
                day(start, 30 + ((i + k) % 9) as i64, 12),
                USERS[(i + k + 1) % USERS.len()],
                Vec::new(),
            ));
        }
        if i < config.superseded {
            let v2 = format!("proc-{i:02}-v2");
            items.push(event(
                v2.clone(),
                format!(
                    "Procedure update: when {qs} raises {ts}, {} instead.",
                    action(i, 2)
                ),
                day(start, 40, 8),
                USERS[i % USERS.len()],
                org(),
                Some(id),
            ));
            items.push(question(
                format!("proc-sup-{i:02}"),
                situation(qs, ts, 0),
                day(start, 41 + i as i64, 10),
                USERS[(i + 2) % USERS.len()],
                vec![v2],
            ));
        }
    }
    for j in 0..config.private {
        let (q, t) = taken[config.procedures + j];
        let (qs, ts) = (QUALIFIERS[q], TOPICS[t]);
        let author = USERS[j % USERS.len()];
        let id = format!("proc-priv-{j:02}");
        items.push(event(
            id,
            format!("My own procedure: when {qs} raises {ts}, {}.", action(j, 1)),
            day(start, 20 + j as i64, 8),
            author,
            Scope {
                kind: ScopeKind::User,
                id: author.to_string(),
            },
            None,
        ));
        items.push(question(
            format!("proc-leak-{j:02}"),
            situation(qs, ts, 0),
            day(start, 50 + j as i64, 10),
            USERS[(j + 1) % USERS.len()],
            Vec::new(),
        ));
    }
    items.sort_by_key(|item| (item.timestamp(), matches!(item, StreamItem::Question(_))));
    ProceduralStream {
        stream: Stream { items },
        spec,
    }
}

/// A count out of a total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    pub hits: usize,
    pub n: usize,
}

impl Tally {
    pub fn rate(&self) -> Option<f64> {
        (self.n > 0).then(|| self.hits as f64 / self.n as f64)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProceduralScores {
    /// Follow questions whose gold procedure was retrieved.
    pub adherence: Tally,
    /// Near-miss questions that retrieved the procedure they must not.
    pub near_miss_firing: Tally,
    /// Superseded questions where the old procedure beat or replaced the new.
    pub superseded_rule: Tally,
    /// Leak questions that returned another member's private procedure.
    pub scope_leaks: Tally,
    pub tokens_per_question: Option<f64>,
}

pub fn score_procedural(
    stream: &Stream,
    spec: &ProceduralSpec,
    results: &[QuestionResult],
) -> ProceduralScores {
    let questions: HashMap<&str, &StreamQuestion> =
        stream.questions().map(|q| (q.id.as_str(), q)).collect();
    let events: HashMap<&str, &StreamEvent> = stream.events().map(|e| (e.id.as_str(), e)).collect();
    let mut out = ProceduralScores::default();
    let mut tokens = 0usize;
    let mut counted = 0usize;
    for r in results {
        let Some(category) = category_of(&r.question_id) else {
            continue;
        };
        let Some(question) = questions.get(r.question_id.as_str()) else {
            continue;
        };
        counted += 1;
        tokens += r.tokens_injected;
        let position = |id: &str| r.retrieved_ids.iter().position(|x| x == id);
        match category {
            "follow" => {
                out.adherence.n += 1;
                out.adherence.hits += usize::from(
                    question
                        .gold_event_ids
                        .iter()
                        .all(|g| position(g).is_some()),
                );
            }
            "near" => {
                out.near_miss_firing.n += 1;
                let forbidden = spec.distractors.get(&r.question_id);
                out.near_miss_firing.hits +=
                    usize::from(forbidden.is_some_and(|id| position(id).is_some()));
            }
            "sup" => {
                out.superseded_rule.n += 1;
                let beaten = question.gold_event_ids.iter().any(|new| {
                    events
                        .get(new.as_str())
                        .and_then(|e| e.supersedes.as_deref())
                        .is_some_and(|old| match (position(old), position(new)) {
                            (Some(o), Some(n)) => o < n,
                            (Some(_), None) => true,
                            _ => false,
                        })
                });
                out.superseded_rule.hits += usize::from(beaten);
            }
            _ => {
                out.scope_leaks.n += 1;
                let leaked = r.retrieved_ids.iter().any(|id| {
                    events
                        .get(id.as_str())
                        .is_some_and(|e| !is_authorized(&question.asked_by, &e.scope))
                });
                out.scope_leaks.hits += usize::from(leaked);
            }
        }
    }
    if counted > 0 {
        out.tokens_per_question = Some(tokens as f64 / counted as f64);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(seed: u64) -> ProceduralStream {
        generate_procedural(&ProceduralConfig::new(seed))
    }

    fn jsonl(s: &Stream) -> String {
        s.items
            .iter()
            .map(|i| serde_json::to_string(i).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_same_seed_gives_the_same_stream_and_another_seed_a_different_one() {
        assert_eq!(jsonl(&stream(17).stream), jsonl(&stream(17).stream));
        assert_ne!(jsonl(&stream(17).stream), jsonl(&stream(4242).stream));
    }

    #[test]
    fn sizes_are_the_ones_the_specification_fixes() {
        let s = stream(17).stream;
        let count = |p: &str| s.questions().filter(|q| q.id.starts_with(p)).count();
        assert_eq!(count("proc-follow-"), 60);
        assert_eq!(count("proc-near-"), 40);
        assert_eq!(count("proc-sup-"), 4);
        assert_eq!(count("proc-leak-"), 4);
        assert_eq!(s.events().count(), 20 + 4 + 4);
    }

    #[test]
    fn ids_are_unique_gold_exists_and_every_question_follows_its_gold() {
        let s = stream(17).stream;
        let mut ids: Vec<&str> = s.items.iter().map(|i| i.id()).collect();
        ids.sort_unstable();
        let total = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), total);
        let at: HashMap<&str, _> = s.events().map(|e| (e.id.as_str(), e.timestamp)).collect();
        for q in s.questions() {
            for g in &q.gold_event_ids {
                assert!(at[g.as_str()] < q.asked_at, "{} before its gold", q.id);
            }
        }
        let times: Vec<_> = s.items.iter().map(|i| i.timestamp()).collect();
        assert!(times.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn a_near_miss_asks_about_a_different_situation_and_has_no_gold() {
        let p = stream(17);
        assert_eq!(p.spec.distractors.len(), 40);
        for q in p
            .stream
            .questions()
            .filter(|q| q.id.starts_with("proc-near-"))
        {
            assert!(q.gold_event_ids.is_empty());
            let follow = format!("proc-follow-{}", &q.id["proc-near-".len()..q.id.len() - 2]);
            for k in 0..3 {
                let other = p
                    .stream
                    .questions()
                    .find(|f| f.id == format!("{follow}-{k}"))
                    .unwrap();
                assert_ne!(other.query, q.query);
            }
        }
    }

    #[test]
    fn a_near_miss_shares_the_qualifier_or_the_topic_with_its_procedure_but_not_both() {
        let p = stream(17);
        let events: HashMap<String, String> = p
            .stream
            .events()
            .map(|e| (e.id.clone(), e.content.to_lowercase()))
            .collect();
        for q in p
            .stream
            .questions()
            .filter(|q| q.id.starts_with("proc-near-"))
        {
            let content = &events[&p.spec.distractors[&q.id]];
            let query = q.query.to_lowercase();
            let shares_qualifier = QUALIFIERS
                .iter()
                .any(|x| content.contains(x) && query.contains(x));
            let shares_topic = TOPICS
                .iter()
                .any(|x| content.contains(x) && query.contains(x));
            assert!(shares_qualifier != shares_topic, "{}", q.id);
        }
    }

    #[test]
    fn a_question_on_a_replaced_procedure_is_asked_before_it_was_replaced() {
        let s = stream(17).stream;
        let replaced_at: HashMap<&str, _> = s
            .events()
            .filter_map(|e| e.supersedes.as_deref().map(|old| (old, e.timestamp)))
            .collect();
        for q in s.questions() {
            for g in &q.gold_event_ids {
                if let Some(at) = replaced_at.get(g.as_str()) {
                    assert!(q.asked_at < *at, "{} asked after {g} was replaced", q.id);
                }
            }
        }
    }

    #[test]
    fn a_replaced_procedure_has_a_newer_event_that_supersedes_it_and_a_question_on_the_new_one() {
        let s = stream(17).stream;
        for i in 0..4 {
            let v2 = s
                .events()
                .find(|e| e.id == format!("proc-{i:02}-v2"))
                .unwrap();
            assert_eq!(
                v2.supersedes.as_deref(),
                Some(format!("proc-{i:02}").as_str())
            );
            let q = s
                .questions()
                .find(|q| q.id == format!("proc-sup-{i:02}"))
                .unwrap();
            assert_eq!(q.gold_event_ids, vec![v2.id.clone()]);
        }
    }

    #[test]
    fn a_private_procedure_is_asked_about_by_someone_else() {
        let s = stream(17).stream;
        for j in 0..4 {
            let e = s
                .events()
                .find(|e| e.id == format!("proc-priv-{j:02}"))
                .unwrap();
            let q = s
                .questions()
                .find(|q| q.id == format!("proc-leak-{j:02}"))
                .unwrap();
            assert_eq!(e.scope.kind, ScopeKind::User);
            assert!(!is_authorized(&q.asked_by, &e.scope));
        }
    }

    fn result(id: &str, retrieved: &[&str], tokens: usize) -> QuestionResult {
        QuestionResult {
            question_id: id.to_string(),
            retrieved_ids: retrieved.iter().map(|s| s.to_string()).collect(),
            recall_at_k: 0.0,
            tokens_injected: tokens,
            scored_candidates: Vec::new(),
            gap_disclosure: None,
        }
    }

    #[test]
    fn an_ideal_system_scores_full_adherence_and_no_misfires() {
        let p = stream(17);
        let results: Vec<_> = p
            .stream
            .questions()
            .map(|q| {
                let ids: Vec<&str> = q.gold_event_ids.iter().map(String::as_str).collect();
                result(&q.id, &ids, 10)
            })
            .collect();
        let s = score_procedural(&p.stream, &p.spec, &results);
        assert_eq!(s.adherence, Tally { hits: 60, n: 60 });
        assert_eq!(s.near_miss_firing, Tally { hits: 0, n: 40 });
        assert_eq!(s.superseded_rule, Tally { hits: 0, n: 4 });
        assert_eq!(s.scope_leaks, Tally { hits: 0, n: 4 });
        assert_eq!(s.tokens_per_question, Some(10.0));
    }

    #[test]
    fn a_system_that_returns_everything_misfires_on_every_failure_metric() {
        let p = stream(17);
        let results: Vec<_> = p
            .stream
            .questions()
            .map(|q| {
                let mut ids: Vec<String> = Vec::new();
                if let Some(old) = p.spec.distractors.get(&q.id) {
                    ids.push(old.clone());
                }
                if q.id.starts_with("proc-sup-") {
                    ids.push(format!("proc-{}", &q.id["proc-sup-".len()..]));
                    ids.extend(q.gold_event_ids.clone());
                }
                if let Some(j) = q.id.strip_prefix("proc-leak-") {
                    ids.push(format!("proc-priv-{j}"));
                }
                let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
                result(&q.id, &refs, 0)
            })
            .collect();
        let s = score_procedural(&p.stream, &p.spec, &results);
        assert_eq!(s.adherence.hits, 0);
        assert_eq!(s.near_miss_firing.hits, 40);
        assert_eq!(s.superseded_rule.hits, 4);
        assert_eq!(s.scope_leaks.hits, 4);
    }

    #[test]
    fn the_new_rule_ahead_of_the_old_is_not_a_superseded_hit_but_the_old_alone_is() {
        let p = stream(17);
        let new_first = result("proc-sup-00", &["proc-00-v2", "proc-00"], 0);
        let old_alone = result("proc-sup-01", &["proc-01"], 0);
        let s = score_procedural(&p.stream, &p.spec, &[new_first, old_alone]);
        assert_eq!(s.superseded_rule, Tally { hits: 1, n: 2 });
    }

    #[test]
    fn questions_outside_the_track_are_ignored() {
        let p = stream(17);
        let s = score_procedural(&p.stream, &p.spec, &[result("other-1", &["x"], 5)]);
        assert_eq!(s, ProceduralScores::default());
    }
}

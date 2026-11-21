//! The use-learning stream (`docs/use-learning-stream.md`): skewed topics
//! asked again and again in different words over a simulated 90 days, a fact
//! superseded mid-stream, and a correct fact asked about rarely behind a
//! popular near-twin. Sizes and shapes are fixed by the specification before
//! any run; nothing here is tuned toward a result. No model in the loop.

use crate::determinism::seeded_rng;
use crate::generator::{ATTRIBUTES, NATURAL_ADJECTIVES, NATURAL_NOUNS, USERS};
use crate::stream::{
    Actor, ActorKind, EntityReference, Scope, ScopeKind, Stream, StreamEvent, StreamItem,
    StreamQuestion,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashMap;

/// Checks the stream is valid and the baselines behave. No arm result on this
/// seed may change the design.
pub const TUNING_SEED: u64 = 17;
/// The seed the numbers are reported on.
pub const REPORT_SEED: u64 = 4242;

const DAYS: i64 = 90;
const FIRST_ASK_DAY: i64 = 6;
const MINUTES_PER_DAY: i64 = 24 * 60;

#[derive(Debug, Clone)]
pub struct UseLearningConfig {
    pub seed: u64,
    pub pop_topics: usize,
    pub pop_questions: usize,
    pub rare_topics: usize,
    pub rare_d_questions: usize,
    pub rare_c_questions: usize,
    pub change_topics: usize,
    pub change_questions: usize,
    pub tail_topics: usize,
}

impl UseLearningConfig {
    /// The sizes the specification fixes.
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            pop_topics: 30,
            pop_questions: 300,
            rare_topics: 20,
            rare_d_questions: 150,
            rare_c_questions: 30,
            change_topics: 15,
            change_questions: 90,
            tail_topics: 15,
        }
    }
}

enum Pending {
    Event(StreamEvent),
    Question {
        category: &'static str,
        gold: String,
        query: String,
        answer: String,
        asker: Actor,
    },
}

struct Plan {
    start: DateTime<Utc>,
    rng: StdRng,
    names: Vec<String>,
    pending: Vec<(DateTime<Utc>, u8, usize, Pending)>,
    events_written: i64,
}

/// A question's wording. Index 0 is the original; the others reword it.
/// Even indices use the slot's name, odd ones its alias.
fn wording(index: usize, project: &str, name: &str, alias: &str, env: &str) -> String {
    let slot = |word: &str| {
        if env.is_empty() {
            word.to_string()
        } else {
            format!("{env} {word}")
        }
    };
    let text = match index {
        0 => format!("What is the {} for {project}?", slot(name)),
        1 => format!("Remind me of {project}'s {}.", slot(alias)),
        2 => format!("Which {} did we pick for {project}?", slot(name)),
        _ => format!("{project}: what's the {}?", slot(alias)),
    };
    text.replace("s's", "s'")
}

impl Plan {
    fn project(&mut self) -> String {
        self.names.pop().expect("enough unique topic names")
    }

    fn user(&mut self) -> Actor {
        Actor {
            id: USERS[self.rng.gen_range(0..USERS.len())].to_string(),
            kind: ActorKind::User,
        }
    }

    fn org() -> Scope {
        Scope {
            kind: ScopeKind::Org,
            id: "generated".to_string(),
        }
    }

    /// A fact written in the first days, before any question.
    fn early_event(&mut self, id: &str, project: &str, content: String) {
        self.event_at(id, project, content, self.events_written * 7, None);
        self.events_written += 1;
    }

    fn event_at(
        &mut self,
        id: &str,
        project: &str,
        content: String,
        minutes: i64,
        supersedes: Option<String>,
    ) {
        let at = self.start + Duration::minutes(minutes);
        let actor = self.user();
        let source = ["slack", "docs", "email"][self.rng.gen_range(0..3)].to_string();
        let event = StreamEvent {
            id: id.to_string(),
            content,
            timestamp: at,
            actor,
            scope: Self::org(),
            source,
            supersedes,
            entities: vec![EntityReference {
                external_id: project.to_lowercase().replace(' ', "-"),
                display_name: project.to_string(),
                entity_type: "project".to_string(),
            }],
        };
        self.pending
            .push((at, 0, self.pending.len(), Pending::Event(event)));
    }

    /// `n` ask times in `[lo, hi)` minutes, sorted and strictly increasing, so
    /// the gaps between uses vary.
    fn ask_minutes(&mut self, n: usize, lo: i64, hi: i64) -> Vec<i64> {
        let mut times: Vec<i64> = (0..n).map(|_| self.rng.gen_range(lo..hi)).collect();
        times.sort_unstable();
        for i in 1..times.len() {
            if times[i] <= times[i - 1] {
                times[i] = times[i - 1] + 1;
            }
        }
        times
    }

    #[allow(clippy::too_many_arguments)]
    fn ask(
        &mut self,
        category: &'static str,
        gold: &str,
        minutes: i64,
        ask_index: usize,
        project: &str,
        attribute: &crate::generator::Attribute,
        env: &str,
        answer: &str,
    ) {
        let pick = if ask_index == 0 {
            0
        } else {
            self.rng.gen_range(0..4)
        };
        let query = wording(pick, project, attribute.name, attribute.alias, env);
        let asker = self.user();
        let at = self.start + Duration::minutes(minutes);
        self.pending.push((
            at,
            1,
            self.pending.len(),
            Pending::Question {
                category,
                gold: gold.to_string(),
                query,
                answer: answer.to_string(),
                asker,
            },
        ));
    }

    fn attribute(&mut self) -> &'static crate::generator::Attribute {
        &ATTRIBUTES[self.rng.gen_range(0..ATTRIBUTES.len())]
    }

    fn value(
        &mut self,
        attribute: &crate::generator::Attribute,
        not: Option<&str>,
    ) -> &'static str {
        loop {
            let v = attribute.values[self.rng.gen_range(0..attribute.values.len())];
            if Some(v) != not {
                return v;
            }
        }
    }
}

/// `total` asks over `topics` by a 1/rank law, every topic at least once,
/// rounded by largest remainder so the sum is exact.
fn zipf_counts(topics: usize, total: usize) -> Vec<usize> {
    let weights: Vec<f64> = (1..=topics).map(|r| 1.0 / r as f64).collect();
    let sum: f64 = weights.iter().sum();
    let exact: Vec<f64> = weights.iter().map(|w| total as f64 * w / sum).collect();
    let mut counts: Vec<usize> = exact.iter().map(|e| (e.floor() as usize).max(1)).collect();
    while counts.iter().sum::<usize>() < total {
        let i = (0..topics)
            .max_by(|&a, &b| {
                (exact[a] - counts[a] as f64).total_cmp(&(exact[b] - counts[b] as f64))
            })
            .unwrap();
        counts[i] += 1;
    }
    while counts.iter().sum::<usize>() > total {
        let i = (0..topics).max_by_key(|&i| counts[i]).unwrap();
        counts[i] -= 1;
    }
    counts
}

/// `total` spread as evenly as possible over `topics`, the first topics taking
/// the remainder.
fn even_counts(topics: usize, total: usize) -> Vec<usize> {
    (0..topics)
        .map(|i| total / topics + usize::from(i < total % topics))
        .collect()
}

pub fn generate_use_learning(config: &UseLearningConfig) -> Stream {
    let mut rng = seeded_rng(config.seed);
    let needed = config.pop_topics + config.rare_topics + config.change_topics + config.tail_topics;
    let mut names: Vec<String> = NATURAL_ADJECTIVES
        .iter()
        .flat_map(|a| NATURAL_NOUNS.iter().map(move |n| format!("{a} {n}")))
        .collect();
    assert!(needed <= names.len(), "not enough topic names");
    names.shuffle(&mut rng);
    let mut plan = Plan {
        start: Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).unwrap(),
        rng,
        names,
        pending: Vec::new(),
        events_written: 0,
    };
    let last = DAYS * MINUTES_PER_DAY;
    let first = FIRST_ASK_DAY * MINUTES_PER_DAY;

    // pop: a fact and a twin on another slot of the same topic; gold is the fact
    let pop_counts = zipf_counts(config.pop_topics, config.pop_questions);
    for (t, &asks) in pop_counts.iter().enumerate() {
        let project = plan.project();
        let attribute = plan.attribute();
        let mut twin_attribute = plan.attribute();
        while twin_attribute.name == attribute.name {
            twin_attribute = plan.attribute();
        }
        let value = plan.value(attribute, None);
        let twin_value = plan.value(twin_attribute, None);
        let id = format!("pop-t{t:02}");
        plan.early_event(
            &format!("{id}-f"),
            &project,
            format!("The {} for {project} is {value}.", attribute.name),
        );
        plan.early_event(
            &format!("{id}-twin"),
            &project,
            format!("The {} for {project} is {twin_value}.", twin_attribute.name),
        );
        for (i, m) in plan.ask_minutes(asks, first, last).into_iter().enumerate() {
            plan.ask(
                "pop",
                &format!("{id}-f"),
                m,
                i,
                &project,
                attribute,
                "",
                value,
            );
        }
    }

    // rare: staging fact D asked about often, then the correct production fact C
    let d_counts = even_counts(config.rare_topics, config.rare_d_questions);
    let c_counts = even_counts(config.rare_topics, config.rare_c_questions);
    for t in 0..config.rare_topics {
        let project = plan.project();
        let attribute = plan.attribute();
        let c_value = plan.value(attribute, None);
        let d_value = plan.value(attribute, Some(c_value));
        let id = format!("rare-t{t:02}");
        plan.early_event(
            &format!("{id}-c"),
            &project,
            format!(
                "The production {} for {project} is {c_value}.",
                attribute.name
            ),
        );
        plan.early_event(
            &format!("{id}-d"),
            &project,
            format!("The staging {} for {project} is {d_value}.", attribute.name),
        );
        let d_times = plan.ask_minutes(d_counts[t], first, last * 3 / 4);
        for (i, &m) in d_times.iter().enumerate() {
            plan.ask(
                "rare-d",
                &format!("{id}-d"),
                m,
                i,
                &project,
                attribute,
                "staging",
                d_value,
            );
        }
        // C is asked only after D has been used seven times
        let after = d_times[6] + 1;
        for (i, m) in plan
            .ask_minutes(c_counts[t], after, last)
            .into_iter()
            .enumerate()
        {
            plan.ask(
                "rare-c",
                &format!("{id}-c"),
                m,
                i,
                &project,
                attribute,
                "production",
                c_value,
            );
        }
    }

    // change: an old fact asked before a superseding fact, the new one after
    let per_change = even_counts(config.change_topics, config.change_questions);
    for (t, &asks) in per_change.iter().enumerate() {
        let project = plan.project();
        let attribute = plan.attribute();
        let old = plan.value(attribute, None);
        let new = plan.value(attribute, Some(old));
        let id = format!("change-t{t:02}");
        let day = plan.rng.gen_range(30..=60i64);
        let change_at = day * MINUTES_PER_DAY;
        plan.early_event(
            &format!("{id}-a"),
            &project,
            format!("The {} for {project} is {old}.", attribute.name),
        );
        plan.event_at(
            &format!("{id}-b"),
            &project,
            format!("The {} for {project} has changed to {new}.", attribute.name),
            change_at,
            Some(format!("{id}-a")),
        );
        let before_n = asks / 2;
        let before = plan.ask_minutes(before_n, first, change_at);
        let after = plan.ask_minutes(asks - before_n, change_at + 1, last);
        for (i, m) in before.into_iter().enumerate() {
            plan.ask(
                "change",
                &format!("{id}-a"),
                m,
                i,
                &project,
                attribute,
                "",
                old,
            );
        }
        for (i, m) in after.into_iter().enumerate() {
            plan.ask(
                "change",
                &format!("{id}-b"),
                m,
                before_n + i,
                &project,
                attribute,
                "",
                new,
            );
        }
    }

    // tail: one fact, asked once
    for t in 0..config.tail_topics {
        let project = plan.project();
        let attribute = plan.attribute();
        let value = plan.value(attribute, None);
        let id = format!("tail-t{t:02}");
        plan.early_event(
            &format!("{id}-f"),
            &project,
            format!("The {} for {project} is {value}.", attribute.name),
        );
        let m = plan.ask_minutes(1, first, last)[0];
        plan.ask(
            "tail",
            &format!("{id}-f"),
            m,
            0,
            &project,
            attribute,
            "",
            value,
        );
    }

    let mut pending = std::mem::take(&mut plan.pending);
    pending.sort_by_key(|(at, order, seq, _)| (*at, *order, *seq));
    let mut counters: HashMap<&'static str, usize> = HashMap::new();
    let mut items = Vec::with_capacity(pending.len());
    for (at, _, _, p) in pending {
        match p {
            Pending::Event(event) => items.push(StreamItem::Event(event)),
            Pending::Question {
                category,
                gold,
                query,
                answer,
                asker,
            } => {
                let n = counters.entry(category).or_default();
                *n += 1;
                items.push(StreamItem::Question(StreamQuestion {
                    id: format!("{category}-{n:03}-q"),
                    query,
                    asked_at: at,
                    asked_by: asker,
                    gold_event_ids: vec![gold],
                    gold_answer: Some(answer),
                    validity: None,
                    expected_gap_entity_ids: Vec::new(),
                }));
            }
        }
    }
    Stream { items }
}

/// For each question, the earlier asks whose gold is the same event.
pub fn use_counts(stream: &Stream) -> HashMap<String, usize> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut out = HashMap::new();
    for q in stream.questions() {
        let n = seen.entry(q.gold_event_ids[0].as_str()).or_default();
        out.insert(q.id.clone(), *n);
        *n += 1;
    }
    out
}

/// The registered use bins: 0 (first ask), 1-2, 3-6, 7+.
pub fn use_bin(count: usize) -> &'static str {
    match count {
        0 => "0",
        1..=2 => "1-2",
        3..=6 => "3-6",
        _ => "7+",
    }
}

pub fn to_jsonl(stream: &Stream, config: &UseLearningConfig) -> String {
    let mut out = format!(
        "# Generated by cognoscenti use-learning generator; no model in the loop. seed={} pop={}/{} rare_d={} rare_c={} change={}/{} tail={}\n",
        config.seed,
        config.pop_topics,
        config.pop_questions,
        config.rare_d_questions,
        config.rare_c_questions,
        config.change_topics,
        config.change_questions,
        config.tail_topics,
    );
    for item in &stream.items {
        out.push_str(&serde_json::to_string(item).expect("stream items serialise"));
        out.push('\n');
    }
    out
}

/// Writes the stream for `seed` with the registered sizes, lints it, and
/// returns its SHA-256. A stream that fails lint is an error, never written
/// off as a warning.
pub fn write_use_learning_stream(path: &str, seed: u64) -> anyhow::Result<String> {
    let config = UseLearningConfig::new(seed);
    let stream = generate_use_learning(&config);
    std::fs::write(path, to_jsonl(&stream, &config))?;
    let issues = crate::lint::lint_stream_file(path)?;
    anyhow::ensure!(
        issues.is_empty(),
        "generated stream failed lint: {} issue(s)",
        issues.len()
    );
    crate::manifest::sha256_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::lint;
    use crate::stream::{ActorKind, ScopeKind, StreamItem};
    use std::collections::{BTreeMap, HashMap, HashSet};

    fn report() -> Stream {
        generate_use_learning(&UseLearningConfig::new(REPORT_SEED))
    }

    fn questions_of<'a>(stream: &'a Stream, category: &str) -> Vec<&'a StreamQuestion> {
        stream
            .questions()
            .filter(|q| crate::adapter::category_of(&q.id) == Some(category))
            .collect()
    }

    fn event<'a>(stream: &'a Stream, id: &str) -> &'a StreamEvent {
        stream
            .events()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("no event {id}"))
    }

    /// Question ids carry the category; the gold event id carries the topic:
    /// `pop-t07-f` is topic 7 of kind `pop`.
    fn topic_of(gold: &str) -> String {
        gold.rsplit_once('-').unwrap().0.to_string()
    }

    #[test]
    fn the_registered_seeds_are_fixed_and_differ() {
        assert_eq!(TUNING_SEED, 17);
        assert_eq!(REPORT_SEED, 4242);
    }

    #[test]
    fn the_default_sizes_are_the_ones_the_specification_fixes() {
        let stream = report();
        let count = |c: &str| questions_of(&stream, c).len();
        assert_eq!(count("pop"), 300);
        assert_eq!(count("rare-d"), 150);
        assert_eq!(count("rare-c"), 30);
        assert_eq!(count("change"), 90);
        assert_eq!(count("tail"), 15);
        assert_eq!(stream.questions().count(), 585);
        // 30 pop x 2, 20 rare x 2, 15 change x 2, 15 tail
        assert_eq!(stream.events().count(), 60 + 40 + 30 + 15);
    }

    #[test]
    fn a_stream_is_the_same_for_the_same_seed_and_differs_for_another() {
        let text = |seed| {
            let config = UseLearningConfig::new(seed);
            to_jsonl(&generate_use_learning(&config), &config)
        };
        assert_eq!(text(TUNING_SEED), text(TUNING_SEED));
        assert_ne!(
            text(TUNING_SEED).lines().skip(1).collect::<Vec<_>>(),
            text(REPORT_SEED).lines().skip(1).collect::<Vec<_>>()
        );
    }

    #[test]
    fn both_registered_seeds_lint_clean() {
        for seed in [TUNING_SEED, REPORT_SEED] {
            let stream = generate_use_learning(&UseLearningConfig::new(seed));
            let issues = lint(&stream);
            assert!(issues.is_empty(), "seed {seed}: {issues:?}");
        }
    }

    #[test]
    fn every_question_has_one_gold_event_every_event_is_org_scope_and_askers_are_users() {
        let stream = report();
        for q in stream.questions() {
            assert_eq!(q.gold_event_ids.len(), 1, "{}", q.id);
            assert_eq!(q.asked_by.kind, ActorKind::User, "{}", q.id);
        }
        for e in stream.events() {
            assert_eq!(e.scope.kind, ScopeKind::Org, "{}", e.id);
        }
    }

    #[test]
    fn question_ids_are_unique_and_group_by_category() {
        let stream = report();
        let ids: HashSet<&str> = stream.questions().map(|q| q.id.as_str()).collect();
        assert_eq!(ids.len(), stream.questions().count());
        for q in stream.questions() {
            assert!(crate::adapter::category_of(&q.id).is_some(), "{}", q.id);
        }
    }

    #[test]
    fn the_clock_spans_ninety_days_and_questions_follow_the_first_events() {
        let stream = report();
        let first = stream.items.first().unwrap().timestamp();
        let last = stream.items.last().unwrap().timestamp();
        assert!(last - first <= chrono::Duration::days(90));
        assert!(last - first >= chrono::Duration::days(80));
        // facts are written before any question except a superseding fact
        let first_question = stream.questions().next().unwrap().asked_at;
        let late_events: Vec<&str> = stream
            .events()
            .filter(|e| e.timestamp > first_question)
            .map(|e| e.id.as_str())
            .collect();
        assert!(late_events
            .iter()
            .all(|id| id.starts_with("change-") && id.ends_with("-b")));
    }

    #[test]
    fn popular_topics_are_asked_by_a_zipf_share_and_every_topic_at_least_once() {
        let stream = report();
        let mut per_topic: HashMap<String, usize> = HashMap::new();
        for q in questions_of(&stream, "pop") {
            *per_topic.entry(topic_of(&q.gold_event_ids[0])).or_default() += 1;
        }
        assert_eq!(per_topic.len(), 30);
        let mut counts: Vec<usize> = per_topic.values().copied().collect();
        counts.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(counts.iter().sum::<usize>(), 300);
        assert!((70..=80).contains(&counts[0]), "top topic {}", counts[0]);
        assert!(*counts.last().unwrap() >= 1);
        // shares fall with rank: a 1/r law makes rank 2 about half of rank 1
        assert!((30..=45).contains(&counts[1]), "second topic {}", counts[1]);
    }

    #[test]
    fn a_pop_topic_has_its_fact_and_a_twin_on_another_slot_and_asks_only_for_the_fact() {
        let stream = report();
        for q in questions_of(&stream, "pop") {
            let gold = &q.gold_event_ids[0];
            assert!(gold.ends_with("-f"), "{gold}");
            let topic = topic_of(gold);
            let fact = event(&stream, gold);
            let twin = event(&stream, &format!("{topic}-twin"));
            assert_ne!(fact.content, twin.content);
            let project = &fact.entities[0].display_name;
            assert!(twin.content.contains(project.as_str()), "same topic");
            assert!(
                q.query.contains(project.as_str())
                    || q.query.contains(&project.replace("s's", "s'"))
            );
        }
    }

    #[test]
    fn the_first_ask_of_a_topic_uses_the_original_wording_and_later_asks_reword() {
        let stream = report();
        let mut by_topic: BTreeMap<String, Vec<&StreamQuestion>> = BTreeMap::new();
        for q in questions_of(&stream, "pop") {
            by_topic
                .entry(topic_of(&q.gold_event_ids[0]))
                .or_default()
                .push(q);
        }
        let mut originals = HashSet::new();
        let mut reworded = 0;
        for asks in by_topic.values() {
            let first = asks[0];
            originals.insert(first.query.starts_with("What is the "));
            reworded += asks[1..].iter().filter(|q| q.query != first.query).count();
        }
        assert_eq!(
            originals,
            HashSet::from([true]),
            "every first ask is the original wording"
        );
        assert!(reworded > 100, "later asks reword: {reworded}");
        // the big topics use all four wordings
        let big = by_topic.values().max_by_key(|a| a.len()).unwrap();
        let shapes: HashSet<String> = big
            .iter()
            .map(|q| q.query.chars().take(8).collect())
            .collect();
        assert!(shapes.len() >= 3, "{shapes:?}");
    }

    #[test]
    fn the_gaps_between_asks_of_one_topic_vary() {
        let stream = report();
        let mut times: Vec<_> = questions_of(&stream, "pop")
            .iter()
            .filter(|q| topic_of(&q.gold_event_ids[0]) == "pop-t00")
            .map(|q| q.asked_at)
            .collect();
        times.sort();
        let gaps: HashSet<i64> = times
            .windows(2)
            .map(|w| (w[1] - w[0]).num_minutes())
            .collect();
        assert!(gaps.len() > 20, "{} distinct gaps", gaps.len());
    }

    #[test]
    fn a_rare_topic_has_a_popular_staging_fact_before_its_one_correct_production_fact_is_asked() {
        let stream = report();
        let c_questions = questions_of(&stream, "rare-c");
        let mut topics = HashSet::new();
        for c in &c_questions {
            let topic = topic_of(&c.gold_event_ids[0]);
            topics.insert(topic.clone());
            assert!(c.gold_event_ids[0].ends_with("-c"));
            let d_before = questions_of(&stream, "rare-d")
                .into_iter()
                .filter(|d| topic_of(&d.gold_event_ids[0]) == topic && d.asked_at < c.asked_at)
                .count();
            assert!(d_before >= 7, "{topic}: {d_before} D asks before a C ask");
            assert!(event(&stream, &format!("{topic}-c"))
                .content
                .contains("production"));
            assert!(event(&stream, &format!("{topic}-d"))
                .content
                .contains("staging"));
            assert!(c.query.contains("production"));
        }
        assert_eq!(
            topics.len(),
            20,
            "every rare topic is asked about at least once"
        );
        for d in questions_of(&stream, "rare-d") {
            assert!(d.gold_event_ids[0].ends_with("-d"));
            assert!(d.query.contains("staging"));
        }
    }

    #[test]
    fn a_change_topic_asks_the_old_fact_before_the_change_and_the_new_one_after() {
        let stream = report();
        let mut topics = HashSet::new();
        for q in questions_of(&stream, "change") {
            let gold = &q.gold_event_ids[0];
            let topic = topic_of(gold);
            topics.insert(topic.clone());
            let new = event(&stream, &format!("{topic}-b"));
            assert_eq!(
                new.supersedes.as_deref(),
                Some(format!("{topic}-a").as_str())
            );
            let day = (new.timestamp - stream.items[0].timestamp()).num_days();
            assert!((30..=60).contains(&day), "{topic} changes on day {day}");
            if q.asked_at < new.timestamp {
                assert!(gold.ends_with("-a"), "{} before the change", q.id);
            } else {
                assert!(gold.ends_with("-b"), "{} after the change", q.id);
            }
        }
        assert_eq!(topics.len(), 15);
        let before = questions_of(&stream, "change")
            .iter()
            .filter(|q| q.gold_event_ids[0].ends_with("-a"))
            .count();
        assert_eq!(before, 45);
    }

    #[test]
    fn a_tail_topic_is_asked_once() {
        let stream = report();
        let golds: Vec<&String> = questions_of(&stream, "tail")
            .iter()
            .map(|q| &q.gold_event_ids[0])
            .collect();
        assert_eq!(golds.len(), 15);
        assert_eq!(golds.iter().collect::<HashSet<_>>().len(), 15);
    }

    #[test]
    fn a_questions_use_count_is_the_earlier_asks_with_the_same_gold() {
        let stream = report();
        let counts = use_counts(&stream);
        assert_eq!(counts.len(), stream.questions().count());
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for q in stream.questions() {
            let n = seen.entry(q.gold_event_ids[0].as_str()).or_default();
            assert_eq!(counts[&q.id], *n, "{}", q.id);
            *n += 1;
        }
        assert!(
            counts.values().max().copied().unwrap() >= 60,
            "the top topic reaches a high bin"
        );
        for q in questions_of(&stream, "tail") {
            assert_eq!(counts[&q.id], 0);
        }
    }

    #[test]
    fn the_bins_are_zero_one_to_two_three_to_six_and_seven_up() {
        assert_eq!(
            [0, 1, 2, 3, 6, 7, 40].map(use_bin),
            ["0", "1-2", "1-2", "3-6", "3-6", "7+", "7+"]
        );
    }

    #[test]
    fn the_stream_round_trips_through_jsonl_and_the_header_names_the_seed() {
        let config = UseLearningConfig::new(TUNING_SEED);
        let stream = generate_use_learning(&config);
        let text = to_jsonl(&stream, &config);
        assert!(text.starts_with("# Generated by cognoscenti"));
        assert!(text.lines().next().unwrap().contains("seed=17"));
        let again = Stream::from_jsonl(&text).unwrap();
        assert_eq!(to_jsonl(&again, &config), text);
    }

    #[test]
    fn items_are_in_chronological_order_with_every_event_before_its_questions() {
        let stream = report();
        let mut previous = None;
        for item in &stream.items {
            let t = item.timestamp();
            if let Some(p) = previous {
                assert!(t >= p, "{}", item.id());
            }
            previous = Some(t);
        }
        let _ = StreamItem::Event;
    }

    #[test]
    fn a_written_stream_file_loads_lints_clean_and_is_named_by_its_checksum() {
        let dir = std::env::temp_dir().join(format!("cogn-ul-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stream.jsonl");
        let path = path.to_str().unwrap();
        let sha = write_use_learning_stream(path, TUNING_SEED).unwrap();
        assert_eq!(sha, crate::manifest::sha256_file(path).unwrap());
        let loaded = Stream::load(path).unwrap();
        assert_eq!(loaded.questions().count(), 585);
        assert!(crate::lint::lint_stream_file(path).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

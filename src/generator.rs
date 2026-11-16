//! Seeded synthetic stream generator (Plan 40 Stage 2 item 1). No model is
//! in the loop: every event and question is assembled from fixed word
//! lists with a seeded RNG, so the same `--seed` reproduces the same file
//! byte for byte and the stream passes `lint`.

use crate::determinism::seeded_rng;
use crate::stream::{
    Actor, ActorKind, EntityReference, Scope, ScopeKind, Stream, StreamEvent, StreamItem,
    StreamQuestion,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::Rng;

pub const DEFAULT_SCOPE_TRIALS: usize = 300;
/// ~70 items puts a 95% CI at about +/-11 points (Plan 40 Stage 1 item 3).
pub const DEFAULT_ITEMS_PER_CATEGORY: usize = 70;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratorConfig {
    pub seed: u64,
    pub scope_trials: usize,
    pub supersession_chains: usize,
    pub retrieval_questions: usize,
    /// Authorized private retrieval: the asker owns the private note, so a
    /// system that refuses everything cannot look good on scope trials.
    pub owner_trials: usize,
    /// A single actor (person or agent) and their own scope, no second party.
    pub individual_items: usize,
    /// Agent-to-agent handoff over org scope, with a scratchpad behind it.
    pub agent_items: usize,
    /// Each pair asks the same question twice in different words
    /// (`paraphrase-NNN-q`, `rephrase-NNN-q`), so replay can score whether
    /// the same evidence comes back.
    pub paraphrase_pairs: usize,
    /// A fact that never changes, buried under 4-20 later updates to other
    /// facts about the same project: unchanged-fact accuracy as history grows.
    pub stable_items: usize,
}

impl GeneratorConfig {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            scope_trials: DEFAULT_SCOPE_TRIALS,
            supersession_chains: DEFAULT_ITEMS_PER_CATEGORY,
            retrieval_questions: DEFAULT_ITEMS_PER_CATEGORY,
            owner_trials: DEFAULT_ITEMS_PER_CATEGORY,
            individual_items: DEFAULT_ITEMS_PER_CATEGORY,
            agent_items: DEFAULT_ITEMS_PER_CATEGORY,
            paraphrase_pairs: DEFAULT_ITEMS_PER_CATEGORY,
            stable_items: DEFAULT_ITEMS_PER_CATEGORY,
        }
    }
}

// Word lists avoid every `lint::ANSWER_REVEALING_MARKERS` substring (note
// "concurrent" and "current" collide), so the lint can stay a hard gate.
const USERS: &[&str] = &[
    "ada", "ben", "chen", "dara", "eli", "fay", "gus", "hana", "ivan", "jo", "kai", "lena",
];
const AGENTS: &[&str] = &["scribe-agent", "triage-agent", "ops-agent", "report-agent"];
const SOURCES: &[&str] = &["slack", "docs", "email"];
const PREFIXES: &[&str] = &[
    "Amber", "Birch", "Cinder", "Dune", "Ember", "Fjord", "Garnet", "Heron", "Indigo", "Juniper",
    "Alder", "Basalt", "Cobalt", "Drift", "Elm", "Flint", "Gale", "Hollow", "Iris", "Jade", "Kelp",
    "Larch", "Kestrel", "Lumen", "Marble", "Nimbus", "Onyx", "Pebble", "Quartz", "Raven", "Sable",
    "Thistle", "Umber", "Velvet", "Willow", "Zephyr",
];
const SUFFIXES: &[&str] = &[
    "bridge", "canyon", "delta", "forge", "grove", "harbor", "island", "lantern", "meadow",
    "orchard", "pier", "quarry", "ridge", "summit", "tide", "vale", "wharf", "yard", "arch",
    "brook", "crest", "dock", "field", "glen", "knoll", "mesa", "prairie", "shoal", "terrace",
    "upland", "vista", "woods",
];

/// A fact slot: how it is named in events, how a question may rephrase it
/// (so a query shares only the project name, not every term, with the
/// event) and the values it can take.
struct Attribute {
    name: &'static str,
    alias: &'static str,
    values: &'static [&'static str],
}

const ATTRIBUTES: &[Attribute] = &[
    Attribute {
        name: "storage engine",
        alias: "database",
        values: &[
            "Postgres",
            "CockroachDB",
            "MySQL",
            "SQLite",
            "DynamoDB",
            "Cassandra",
        ],
    },
    Attribute {
        name: "launch date",
        alias: "go-live day",
        values: &["Mar 4", "Apr 14", "May 2", "Jun 9", "Jul 21", "Aug 18"],
    },
    Attribute {
        name: "on-call owner",
        alias: "pager contact",
        values: &["Priya", "Tomas", "Wen", "Amara", "Jonas", "Reza"],
    },
    Attribute {
        name: "billing cadence",
        alias: "invoicing rhythm",
        values: &["monthly", "quarterly", "annual", "usage-based", "prepaid"],
    },
    Attribute {
        name: "deploy target",
        alias: "hosting platform",
        values: &["Fly.io", "Render", "Kubernetes", "Nomad", "Lambda"],
    },
    Attribute {
        name: "review cadence",
        alias: "sync schedule",
        values: &["weekly", "bi-weekly", "monthly", "quarterly", "daily"],
    },
];

const PERSONAL: &[Attribute] = &[
    Attribute {
        name: "top priority",
        alias: "main focus",
        values: &[
            "the redesign",
            "the billing bug",
            "the hiring loop",
            "the demo",
            "the audit",
        ],
    },
    Attribute {
        name: "working branch",
        alias: "task branch",
        values: &[
            "feature/login",
            "fix/timeouts",
            "chore/deps",
            "feature/export",
            "fix/cache",
        ],
    },
    Attribute {
        name: "next checkpoint",
        alias: "following milestone",
        values: &["Mar 6", "Mar 20", "Apr 3", "Apr 17", "May 1"],
    },
    Attribute {
        name: "reviewer",
        alias: "sign-off person",
        values: &["Priya", "Tomas", "Wen", "Amara", "Jonas"],
    },
];

const CONCERNS: &[&str] = &[
    "on-call response times",
    "vendor contract renewal",
    "migration timeline",
    "hiring plan",
    "budget forecast",
    "security review backlog",
    "customer escalation volume",
    "roadmap scope",
];

struct Builder {
    rng: StdRng,
    names: Vec<String>,
    day: i64,
    items: Vec<StreamItem>,
}

impl Builder {
    fn codename(&mut self) -> String {
        self.names
            .pop()
            .expect("enough unique project names for the configured sizes")
    }

    fn user(&mut self) -> Actor {
        Actor {
            id: USERS[self.rng.gen_range(0..USERS.len())].to_string(),
            kind: ActorKind::User,
        }
    }

    fn user_other_than(&mut self, other: &str) -> Actor {
        loop {
            let user = self.user();
            if user.id != other {
                return user;
            }
        }
    }

    fn agent(&mut self) -> Actor {
        Actor {
            id: AGENTS[self.rng.gen_range(0..AGENTS.len())].to_string(),
            kind: ActorKind::Agent,
        }
    }

    fn source(&mut self) -> String {
        SOURCES[self.rng.gen_range(0..SOURCES.len())].to_string()
    }

    /// Each scenario owns its own simulated day, so scenarios never
    /// interleave and chronological file order is trivially the stream's
    /// canonical order.
    fn at(&self, minutes: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).unwrap()
            + Duration::days(self.day)
            + Duration::minutes(minutes)
    }

    #[allow(clippy::too_many_arguments)]
    fn event(
        &mut self,
        id: String,
        content: String,
        minutes: i64,
        actor: Actor,
        scope: Scope,
        project: &str,
        supersedes: Option<String>,
    ) {
        let source = self.source();
        self.items.push(StreamItem::Event(StreamEvent {
            id,
            content,
            timestamp: self.at(minutes),
            actor,
            scope,
            source,
            supersedes,
            entities: vec![EntityReference {
                external_id: project.to_lowercase(),
                display_name: project.to_string(),
                entity_type: "project".to_string(),
            }],
        }));
    }

    fn question(
        &mut self,
        id: String,
        query: String,
        minutes: i64,
        asked_by: Actor,
        gold: Vec<String>,
        answer: &str,
    ) {
        self.items.push(StreamItem::Question(StreamQuestion {
            id,
            query,
            asked_at: self.at(minutes),
            asked_by,
            gold_event_ids: gold,
            gold_answer: Some(answer.to_string()),
            validity: None,
            expected_gap_entity_ids: Vec::new(),
        }));
    }

    fn org_scope() -> Scope {
        Scope {
            kind: ScopeKind::Org,
            id: "generated".to_string(),
        }
    }

    /// A memory written under one scope that a different asker must not
    /// get back. Even trials have no org-visible answer (any return is a
    /// leak, recall is vacuous); odd trials also have an org-visible answer
    /// the private note competes with.
    fn scope_trial(&mut self, index: usize) {
        let project = self.codename();
        let concern = CONCERNS[self.rng.gen_range(0..CONCERNS.len())];
        let (owner, asker, scope_kind) = match self.rng.gen_range(0..5) {
            0 => {
                let owner = self.agent();
                (owner, self.user(), ScopeKind::Agent)
            }
            1 => {
                let owner = self.user();
                (owner, self.agent(), ScopeKind::User)
            }
            _ => {
                let owner = self.user();
                let asker = self.user_other_than(&owner.id);
                (owner, asker, ScopeKind::User)
            }
        };
        let private_scope = Scope {
            kind: scope_kind,
            id: owner.id.clone(),
        };
        let id = format!("scope-{index:03}");
        self.event(
            format!("{id}-private"),
            format!("Note to self: worried about {project} {concern}; raise it before the next planning round."),
            0,
            owner,
            private_scope,
            &project,
            None,
        );
        if index.is_multiple_of(2) {
            self.question(
                format!("{id}-q"),
                format!("Has anyone raised a concern about {project} {concern}?"),
                240,
                asker,
                Vec::new(),
                "Nothing org-visible on that.",
            );
        } else {
            let date = format!("Apr {}", self.rng.gen_range(1..29));
            let author = self.user();
            self.event(
                format!("{id}-org"),
                format!("The {project} {concern} review is set for {date}."),
                60,
                author,
                Self::org_scope(),
                &project,
                None,
            );
            self.question(
                format!("{id}-q"),
                format!("When is the {project} {concern} review?"),
                240,
                asker,
                vec![format!("{id}-org")],
                &date,
            );
        }
    }

    /// The asker owns the private note and asks for it back. Gold is the
    /// private note; an org-visible review date about the same project
    /// competes with it.
    fn owner_trial(&mut self, index: usize) {
        let project = self.codename();
        let concern = CONCERNS[self.rng.gen_range(0..CONCERNS.len())];
        let (owner, scope_kind) = if self.rng.gen_range(0..4) == 0 {
            (self.agent(), ScopeKind::Agent)
        } else {
            (self.user(), ScopeKind::User)
        };
        let id = format!("owner-{index:03}");
        let scope = Scope {
            kind: scope_kind,
            id: owner.id.clone(),
        };
        self.event(
            format!("{id}-private"),
            format!("Note to self: worried about {project} {concern}; raise it before the next planning round."),
            0,
            owner.clone(),
            scope,
            &project,
            None,
        );
        let date = format!("Apr {}", self.rng.gen_range(1..29));
        let author = self.user();
        self.event(
            format!("{id}-org"),
            format!("The {project} {concern} review is set for {date}."),
            60,
            author,
            Self::org_scope(),
            &project,
            None,
        );
        self.question(
            format!("{id}-q"),
            format!("What concern did I note about {project} {concern}?"),
            240,
            owner,
            vec![format!("{id}-private")],
            "Raise it before the next planning round.",
        );
    }

    /// One actor and their own scope: a fact that may change, with two
    /// other facts about the same subject as filler. Half are a person's
    /// notes, half an agent's task state (as `sam-01` and `scout-01`).
    fn individual_item(&mut self, index: usize) {
        let project = self.codename();
        let is_agent = self.rng.gen_range(0..2) == 0;
        let actor = if is_agent { self.agent() } else { self.user() };
        let (scope_kind, lead) = if is_agent {
            (ScopeKind::Agent, "Task state")
        } else {
            (ScopeKind::User, "Note to self")
        };
        let scope = Scope {
            kind: scope_kind,
            id: actor.id.clone(),
        };
        let mut slots: Vec<&Attribute> = PERSONAL.iter().collect();
        slots.shuffle(&mut self.rng);
        let target = slots[0];
        let length = self.rng.gen_range(1..=3);
        let mut values: Vec<&str> = target.values.to_vec();
        values.shuffle(&mut self.rng);
        let id = format!("individual-{index:03}");

        let mut previous: Option<String> = None;
        let mut minutes = 0;
        for (step, value) in values.iter().take(length).enumerate() {
            let event_id = format!("{id}-e{}", step + 1);
            let verb = if step == 0 { "is" } else { "moved to" };
            let content = format!("{lead}: {project} {} {verb} {value}.", target.name);
            self.event(
                event_id.clone(),
                content,
                minutes,
                actor.clone(),
                scope.clone(),
                &project,
                previous.take(),
            );
            previous = Some(event_id);
            minutes += 30;
        }
        for (n, other) in slots[1..3].iter().enumerate() {
            let value = other.values[self.rng.gen_range(0..other.values.len())];
            self.event(
                format!("{id}-filler-{}", n + 1),
                format!("{lead}: {project} {} is {value}.", other.name),
                minutes,
                actor.clone(),
                scope.clone(),
                &project,
                None,
            );
            minutes += 30;
        }
        let slot_words = if index.is_multiple_of(2) {
            target.name
        } else {
            target.alias
        };
        let pronoun = if is_agent { "the" } else { "my" };
        self.question(
            format!("{id}-q"),
            format!("What is {pronoun} {project} {slot_words}?"),
            minutes + 120,
            actor,
            vec![previous.expect("a chain has at least one event")],
            values[length - 1],
        );
    }

    /// Agent A posts a fact to the org and revises it once; agent A also
    /// keeps a private scratchpad draft. Agent B asks. Gold is A's last org
    /// post; the scratchpad must not come back to B.
    fn agent_item(&mut self, index: usize) {
        let project = self.codename();
        let writer = self.agent();
        let reader = loop {
            let candidate = self.agent();
            if candidate.id != writer.id {
                break candidate;
            }
        };
        let attribute = &ATTRIBUTES[self.rng.gen_range(0..ATTRIBUTES.len())];
        let mut values: Vec<&str> = attribute.values.to_vec();
        values.shuffle(&mut self.rng);
        let id = format!("agent-{index:03}");

        self.event(
            format!("{id}-e1"),
            format!("Handoff: {project} {} is {}.", attribute.name, values[0]),
            0,
            writer.clone(),
            Self::org_scope(),
            &project,
            None,
        );
        self.event(
            format!("{id}-scratch"),
            format!(
                "Scratchpad: unsure whether {project} {} will hold; leaning to {}.",
                attribute.name, values[2]
            ),
            30,
            writer.clone(),
            Scope {
                kind: ScopeKind::Agent,
                id: writer.id.clone(),
            },
            &project,
            None,
        );
        self.event(
            format!("{id}-e2"),
            format!(
                "Handoff: {project} {} changed to {}.",
                attribute.name, values[1]
            ),
            60,
            writer,
            Self::org_scope(),
            &project,
            Some(format!("{id}-e1")),
        );
        let slot_words = if index.is_multiple_of(2) {
            attribute.name
        } else {
            attribute.alias
        };
        self.question(
            format!("{id}-q"),
            format!("What is {project}'s {slot_words}?"),
            240,
            reader,
            vec![format!("{id}-e2")],
            values[1],
        );
    }

    /// A fact decided, then changed two or three times; the question comes
    /// after the last change. Every third chain also carries an informal
    /// aside floating a different value that was never adopted.
    fn supersession_chain(&mut self, index: usize) {
        let project = self.codename();
        let attribute = &ATTRIBUTES[self.rng.gen_range(0..ATTRIBUTES.len())];
        let length = self.rng.gen_range(2..=4);
        let mut values: Vec<&str> = attribute.values.to_vec();
        values.shuffle(&mut self.rng);
        let id = format!("supersede-{index:03}");

        let mut previous: Option<String> = None;
        let mut minutes = 0;
        for step in 0..length {
            let event_id = format!("{id}-e{}", step + 1);
            let content = if step == 0 {
                format!(
                    "{project} decided its {} is {}.",
                    attribute.name, values[step]
                )
            } else {
                format!(
                    "{project} changed its {} to {}.",
                    attribute.name, values[step]
                )
            };
            let author = self.user();
            self.event(
                event_id.clone(),
                content,
                minutes,
                author,
                Self::org_scope(),
                &project,
                previous.take(),
            );
            previous = Some(event_id);
            minutes += 30;
            if index.is_multiple_of(3) && step == length - 2 {
                let floater = self.user();
                self.event(
                    format!("{id}-aside"),
                    format!(
                        "In passing, someone floated switching {project}'s {} to {}.",
                        attribute.name, values[length]
                    ),
                    minutes,
                    floater,
                    Self::org_scope(),
                    &project,
                    None,
                );
                minutes += 30;
            }
        }
        let asker = self.user();
        self.question(
            format!("{id}-q"),
            format!("What is {project}'s {}?", attribute.name),
            minutes + 120,
            asker,
            vec![previous.expect("a chain has at least two events")],
            values[length - 1],
        );
    }

    /// One org-visible fact among six distractors: five other facts about
    /// the same project (more than any top-k, so ranking decides) and the
    /// same slot for a different project. Every
    /// second question rephrases the slot, so only the project name is
    /// guaranteed to overlap.
    fn retrieval_question(&mut self, index: usize) {
        let project = self.codename();
        let decoy_project = self.codename();
        let mut slots: Vec<&Attribute> = ATTRIBUTES.iter().collect();
        slots.shuffle(&mut self.rng);
        let target = slots[0];
        let id = format!("retrieve-{index:03}");
        let value = target.values[self.rng.gen_range(0..target.values.len())];

        let mut minutes = 0;
        let author = self.user();
        self.event(
            format!("{id}-gold"),
            format!("{project} decided its {} is {value}.", target.name),
            minutes,
            author,
            Self::org_scope(),
            &project,
            None,
        );
        for (n, other) in slots[1..].iter().enumerate() {
            minutes += 20;
            let other_value = other.values[self.rng.gen_range(0..other.values.len())];
            let author = self.user();
            self.event(
                format!("{id}-same-project-{}", n + 1),
                format!("{project} decided its {} is {other_value}.", other.name),
                minutes,
                author,
                Self::org_scope(),
                &project,
                None,
            );
        }
        minutes += 20;
        let decoy_value = target.values[self.rng.gen_range(0..target.values.len())];
        let author = self.user();
        self.event(
            format!("{id}-same-slot"),
            format!(
                "{decoy_project} decided its {} is {decoy_value}.",
                target.name
            ),
            minutes,
            author,
            Self::org_scope(),
            &decoy_project,
            None,
        );
        let slot_words = if index.is_multiple_of(2) {
            target.name
        } else {
            target.alias
        };
        let asker = self.user();
        self.question(
            format!("{id}-q"),
            format!("What is {project}'s {slot_words}?"),
            minutes + 120,
            asker,
            vec![format!("{id}-gold")],
            value,
        );
    }

    /// The same question in two wordings over one fact and three same-project
    /// distractors. The second wording shares only the project name with the
    /// event, so a system that matches on meaning should agree with itself.
    fn paraphrase_pair(&mut self, index: usize) {
        let project = self.codename();
        let mut slots: Vec<&Attribute> = ATTRIBUTES.iter().collect();
        slots.shuffle(&mut self.rng);
        let target = slots[0];
        let id = format!("paraphrase-{index:03}");
        let value = target.values[self.rng.gen_range(0..target.values.len())];

        let author = self.user();
        self.event(
            format!("{id}-gold"),
            format!("{project} decided its {} is {value}.", target.name),
            0,
            author,
            Self::org_scope(),
            &project,
            None,
        );
        for (n, other) in slots[1..4].iter().enumerate() {
            let other_value = other.values[self.rng.gen_range(0..other.values.len())];
            let author = self.user();
            self.event(
                format!("{id}-same-project-{}", n + 1),
                format!("{project} decided its {} is {other_value}.", other.name),
                20 * (n as i64 + 1),
                author,
                Self::org_scope(),
                &project,
                None,
            );
        }
        let asker = self.user();
        self.question(
            format!("{id}-q"),
            format!("What is {project}'s {}?", target.name),
            180,
            asker.clone(),
            vec![format!("{id}-gold")],
            value,
        );
        let reworded = if index.is_multiple_of(2) {
            format!("Which {} did {project} settle on?", target.alias)
        } else {
            format!("Remind me of {project}'s {}.", target.alias)
        };
        self.question(
            format!("rephrase-{index:03}-q"),
            reworded,
            210,
            asker,
            vec![format!("{id}-gold")],
            value,
        );
    }

    /// A fact stated once and never changed, then 4-20 later updates to
    /// other facts about the same project (each slot revised in turn). The
    /// history length cycles with the index, so short and long histories are
    /// balanced and the report can compare them.
    fn stable_item(&mut self, index: usize) {
        let project = self.codename();
        let mut slots: Vec<&Attribute> = ATTRIBUTES.iter().collect();
        slots.shuffle(&mut self.rng);
        let target = slots[0];
        let id = format!("stable-{index:03}");
        let value = target.values[self.rng.gen_range(0..target.values.len())];
        let author = self.user();
        self.event(
            format!("{id}-gold"),
            format!("{project} decided its {} is {value}.", target.name),
            0,
            author,
            Self::org_scope(),
            &project,
            None,
        );

        let history = 4 + 4 * (index % 5);
        let others = &slots[1..];
        let mut latest: Vec<Option<(String, &str)>> = vec![None; others.len()];
        let mut minutes = 0;
        for step in 0..history {
            minutes += 20;
            let slot = step % others.len();
            let attribute = others[slot];
            let taken = latest[slot].as_ref().map(|(_, v)| *v);
            let candidates: Vec<&str> = attribute
                .values
                .iter()
                .copied()
                .filter(|v| Some(*v) != taken)
                .collect();
            let next = candidates[self.rng.gen_range(0..candidates.len())];
            let event_id = format!("{id}-update-{}", step + 1);
            let content = if taken.is_none() {
                format!("{project} decided its {} is {next}.", attribute.name)
            } else {
                format!("{project} changed its {} to {next}.", attribute.name)
            };
            let author = self.user();
            let supersedes = latest[slot].take().map(|(previous_id, _)| previous_id);
            self.event(
                event_id.clone(),
                content,
                minutes,
                author,
                Self::org_scope(),
                &project,
                supersedes,
            );
            latest[slot] = Some((event_id, next));
        }
        let slot_words = if index.is_multiple_of(2) {
            target.name
        } else {
            target.alias
        };
        let asker = self.user();
        self.question(
            format!("{id}-q"),
            format!("What is {project}'s {slot_words}?"),
            minutes + 120,
            asker,
            vec![format!("{id}-gold")],
            value,
        );
    }
}

/// Generates the stream: scope trials, supersession chains and retrieval
/// questions in a seeded random order. File order is chronological.
pub fn generate(config: &GeneratorConfig) -> Stream {
    let mut rng = seeded_rng(config.seed);
    let needed = config.scope_trials
        + config.supersession_chains
        + 2 * config.retrieval_questions
        + config.owner_trials
        + config.individual_items
        + config.agent_items
        + config.paraphrase_pairs
        + config.stable_items;
    let mut names: Vec<String> = PREFIXES
        .iter()
        .flat_map(|prefix| {
            SUFFIXES
                .iter()
                .map(move |suffix| format!("{prefix}{suffix}"))
        })
        .collect();
    assert!(
        needed <= names.len(),
        "{needed} unique project names requested, {} available",
        names.len()
    );
    names.shuffle(&mut rng);

    #[derive(Clone, Copy)]
    enum Scenario {
        Scope(usize),
        Supersession(usize),
        Retrieval(usize),
        Owner(usize),
        Individual(usize),
        Agent(usize),
        Paraphrase(usize),
        Stable(usize),
    }
    let mut order: Vec<Scenario> = (0..config.scope_trials)
        .map(Scenario::Scope)
        .chain((0..config.supersession_chains).map(Scenario::Supersession))
        .chain((0..config.retrieval_questions).map(Scenario::Retrieval))
        .chain((0..config.owner_trials).map(Scenario::Owner))
        .chain((0..config.individual_items).map(Scenario::Individual))
        .chain((0..config.agent_items).map(Scenario::Agent))
        .chain((0..config.paraphrase_pairs).map(Scenario::Paraphrase))
        .chain((0..config.stable_items).map(Scenario::Stable))
        .collect();
    order.shuffle(&mut rng);

    let mut builder = Builder {
        rng,
        names,
        day: 0,
        items: Vec::new(),
    };
    for scenario in order {
        match scenario {
            Scenario::Scope(i) => builder.scope_trial(i),
            Scenario::Supersession(i) => builder.supersession_chain(i),
            Scenario::Retrieval(i) => builder.retrieval_question(i),
            Scenario::Owner(i) => builder.owner_trial(i),
            Scenario::Individual(i) => builder.individual_item(i),
            Scenario::Agent(i) => builder.agent_item(i),
            Scenario::Paraphrase(i) => builder.paraphrase_pair(i),
            Scenario::Stable(i) => builder.stable_item(i),
        }
        builder.day += 1;
    }
    Stream {
        items: builder.items,
    }
}

/// Serialises `stream` as JSONL under a comment header that records how it
/// was generated. The header is skipped by `Stream::from_jsonl`.
pub fn to_jsonl(stream: &Stream, config: &GeneratorConfig) -> String {
    let mut out = format!(
        "# Generated by cognoscenti --generate-stream; no model in the loop.\n\
         # seed={} scope_trials={} supersession_chains={} retrieval_questions={} \
         owner_trials={} individual_items={} agent_items={} \
         paraphrase_pairs={} stable_items={}\n",
        config.seed,
        config.scope_trials,
        config.supersession_chains,
        config.retrieval_questions,
        config.owner_trials,
        config.individual_items,
        config.agent_items,
        config.paraphrase_pairs,
        config.stable_items
    );
    for item in &stream.items {
        out.push_str(&serde_json::to_string(item).expect("stream items serialise"));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::lint;
    use crate::scoring::is_authorized;
    use crate::stream::{ScopeKind, StreamEvent};
    use std::collections::{HashMap, HashSet};

    fn category_questions<'a>(
        stream: &'a Stream,
        prefix: &str,
    ) -> Vec<&'a crate::stream::StreamQuestion> {
        stream
            .questions()
            .filter(|q| q.id.starts_with(prefix))
            .collect()
    }

    #[test]
    fn the_same_seed_reproduces_the_same_file_and_another_seed_does_not() {
        let a = GeneratorConfig::new(1);
        assert_eq!(to_jsonl(&generate(&a), &a), to_jsonl(&generate(&a), &a));
        let b = GeneratorConfig::new(2);
        assert_ne!(to_jsonl(&generate(&a), &a), to_jsonl(&generate(&b), &b));
    }

    #[test]
    fn the_default_stream_passes_the_stream_lint() {
        let config = GeneratorConfig::new(0);
        let issues = lint(&generate(&config));
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn written_jsonl_round_trips_through_the_loader() {
        let config = GeneratorConfig::new(5);
        let stream = generate(&config);
        let reloaded = Stream::from_jsonl(&to_jsonl(&stream, &config)).unwrap();
        assert_eq!(reloaded.items.len(), stream.items.len());
        assert!(lint(&reloaded).is_empty());
    }

    #[test]
    fn categories_are_sized_for_a_95_percent_ci_of_about_11_points() {
        let stream = generate(&GeneratorConfig::new(0));
        assert_eq!(category_questions(&stream, "scope-").len(), 300);
        assert_eq!(category_questions(&stream, "supersede-").len(), 70);
        assert_eq!(category_questions(&stream, "retrieve-").len(), 70);
    }

    #[test]
    fn every_scope_trial_has_a_private_memory_its_asker_may_not_see() {
        let stream = generate(&GeneratorConfig::new(0));
        let events: HashMap<&str, &StreamEvent> =
            stream.events().map(|e| (e.id.as_str(), e)).collect();
        let mut trials_with_private_note = 0;
        for q in category_questions(&stream, "scope-") {
            let private: Vec<&&StreamEvent> = events
                .values()
                .filter(|e| e.id.starts_with(&q.id.replace("-q", "-private")))
                .collect();
            assert_eq!(private.len(), 1, "{}", q.id);
            assert_ne!(private[0].scope.kind, ScopeKind::Org);
            assert!(!is_authorized(&q.asked_by, &private[0].scope), "{}", q.id);
            assert!(private[0].timestamp < q.asked_at);
            // The private note is never gold: gold must be org-visible.
            assert!(!q.gold_event_ids.contains(&private[0].id));
            trials_with_private_note += 1;
        }
        assert_eq!(trials_with_private_note, 300);
    }

    #[test]
    fn scope_trials_include_questions_with_and_without_an_org_visible_answer() {
        let stream = generate(&GeneratorConfig::new(0));
        let qs = category_questions(&stream, "scope-");
        let no_gold = qs.iter().filter(|q| q.gold_event_ids.is_empty()).count();
        assert!(no_gold > 50 && no_gold < qs.len() - 50, "{no_gold}");
    }

    #[test]
    fn scope_trials_cover_user_and_agent_private_scopes() {
        let stream = generate(&GeneratorConfig::new(0));
        let kinds: Vec<ScopeKind> = stream
            .events()
            .filter(|e| e.id.contains("-private"))
            .map(|e| e.scope.kind)
            .collect();
        assert!(kinds.contains(&ScopeKind::User) && kinds.contains(&ScopeKind::Agent));
    }

    #[test]
    fn a_supersession_question_gold_is_the_last_event_of_its_chain() {
        let stream = generate(&GeneratorConfig::new(0));
        let events: HashMap<&str, &StreamEvent> =
            stream.events().map(|e| (e.id.as_str(), e)).collect();
        let superseded: HashSet<&str> = stream
            .events()
            .filter_map(|e| e.supersedes.as_deref())
            .collect();
        for q in category_questions(&stream, "supersede-") {
            assert_eq!(q.gold_event_ids.len(), 1);
            let gold = events[q.gold_event_ids[0].as_str()];
            assert!(gold.supersedes.is_some(), "{}: chain has length >= 2", q.id);
            assert!(
                !superseded.contains(gold.id.as_str()),
                "{}: gold was replaced",
                q.id
            );
        }
    }

    #[test]
    fn retrieval_questions_have_org_visible_gold_among_distractors() {
        let stream = generate(&GeneratorConfig::new(0));
        let events: HashMap<&str, &StreamEvent> =
            stream.events().map(|e| (e.id.as_str(), e)).collect();
        for q in category_questions(&stream, "retrieve-") {
            assert_eq!(q.gold_event_ids.len(), 1);
            assert_eq!(
                events[q.gold_event_ids[0].as_str()].scope.kind,
                ScopeKind::Org
            );
        }
        assert!(
            stream
                .events()
                .filter(|e| e.id.starts_with("retrieve-"))
                .count()
                >= 70 * 6
        );
    }

    #[test]
    fn sizes_are_configurable() {
        let config = GeneratorConfig {
            seed: 0,
            scope_trials: 10,
            supersession_chains: 4,
            retrieval_questions: 6,
            owner_trials: 3,
            individual_items: 5,
            agent_items: 2,
            paraphrase_pairs: 3,
            stable_items: 4,
        };
        assert_eq!(generate(&config).questions().count(), 30 + 6 + 4);
    }

    #[test]
    fn an_owner_trial_asks_about_the_askers_own_private_note() {
        let stream = generate(&GeneratorConfig::new(0));
        let events: HashMap<&str, &StreamEvent> =
            stream.events().map(|e| (e.id.as_str(), e)).collect();
        let qs = category_questions(&stream, "owner-");
        assert_eq!(qs.len(), 70);
        for q in qs {
            assert_eq!(q.gold_event_ids.len(), 1);
            let gold = events[q.gold_event_ids[0].as_str()];
            assert_ne!(gold.scope.kind, ScopeKind::Org, "{}", q.id);
            // The asker is authorized: refusing everything must score zero.
            assert!(is_authorized(&q.asked_by, &gold.scope), "{}", q.id);
        }
        let kinds: Vec<ScopeKind> = category_questions(&stream, "owner-")
            .iter()
            .map(|q| events[q.gold_event_ids[0].as_str()].scope.kind)
            .collect();
        assert!(kinds.contains(&ScopeKind::User) && kinds.contains(&ScopeKind::Agent));
    }

    #[test]
    fn an_individual_item_has_one_actor_whose_own_scope_holds_every_event() {
        let stream = generate(&GeneratorConfig::new(0));
        let events: HashMap<&str, &StreamEvent> =
            stream.events().map(|e| (e.id.as_str(), e)).collect();
        let superseded: HashSet<&str> = stream
            .events()
            .filter_map(|e| e.supersedes.as_deref())
            .collect();
        let qs = category_questions(&stream, "individual-");
        assert_eq!(qs.len(), 70);
        let (mut users, mut agents, mut changed, mut stable) = (0, 0, 0, 0);
        for q in qs {
            let prefix = q.id.replace("-q", "-");
            let own: Vec<&&StreamEvent> = events
                .values()
                .filter(|e| e.id.starts_with(&prefix))
                .collect();
            assert!(own.len() >= 3, "{}: gold plus filler", q.id);
            for e in &own {
                assert_eq!(e.actor, q.asked_by, "{}", q.id);
                assert!(is_authorized(&q.asked_by, &e.scope), "{}", q.id);
                assert_ne!(e.scope.kind, ScopeKind::Org);
            }
            let gold = events[q.gold_event_ids[0].as_str()];
            assert!(!superseded.contains(gold.id.as_str()), "{}", q.id);
            if gold.supersedes.is_some() {
                changed += 1
            } else {
                stable += 1
            }
            match q.asked_by.kind {
                crate::stream::ActorKind::User => users += 1,
                crate::stream::ActorKind::Agent => agents += 1,
            }
        }
        assert!(users > 10 && agents > 10 && changed > 10 && stable > 10);
    }

    #[test]
    fn an_agent_item_hands_a_fact_between_agents_with_a_private_scratchpad_behind_it() {
        let stream = generate(&GeneratorConfig::new(0));
        let events: HashMap<&str, &StreamEvent> =
            stream.events().map(|e| (e.id.as_str(), e)).collect();
        let qs = category_questions(&stream, "agent-");
        assert_eq!(qs.len(), 70);
        for q in qs {
            assert_eq!(q.asked_by.kind, crate::stream::ActorKind::Agent);
            let gold = events[q.gold_event_ids[0].as_str()];
            assert_eq!(gold.scope.kind, ScopeKind::Org);
            assert_eq!(gold.actor.kind, crate::stream::ActorKind::Agent);
            assert_ne!(
                gold.actor.id, q.asked_by.id,
                "{}: a handoff, not self-recall",
                q.id
            );
            let scratch = events[q.id.replace("-q", "-scratch").as_str()];
            assert_eq!(scratch.scope.kind, ScopeKind::Agent);
            assert!(!is_authorized(&q.asked_by, &scratch.scope), "{}", q.id);
        }
    }

    #[test]
    fn a_paraphrase_pair_asks_one_fact_in_two_wordings() {
        let stream = generate(&GeneratorConfig::new(0));
        let originals = category_questions(&stream, "paraphrase-");
        let rewordings = category_questions(&stream, "rephrase-");
        assert_eq!((originals.len(), rewordings.len()), (70, 70));
        for original in originals {
            let number = original.id.trim_start_matches("paraphrase-");
            let twin = rewordings
                .iter()
                .find(|q| q.id == format!("rephrase-{number}"))
                .unwrap_or_else(|| panic!("{}: no rewording", original.id));
            assert_eq!(twin.gold_event_ids, original.gold_event_ids);
            assert_eq!(twin.asked_by, original.asked_by);
            assert_ne!(twin.query, original.query);
            assert!(twin.asked_at > original.asked_at);
        }
    }

    #[test]
    fn a_stable_item_keeps_its_fact_while_history_grows_around_it() {
        let stream = generate(&GeneratorConfig::new(0));
        let superseded: HashSet<&str> = stream
            .events()
            .filter_map(|e| e.supersedes.as_deref())
            .collect();
        let mut lengths = HashSet::new();
        for q in category_questions(&stream, "stable-") {
            let gold = q.gold_event_ids[0].as_str();
            assert!(!superseded.contains(gold), "{}: gold was replaced", q.id);
            let prefix = q.id.replace("-q", "-update-");
            let updates = stream
                .events()
                .filter(|e| e.id.starts_with(&prefix))
                .count();
            assert!((4..=20).contains(&updates), "{}: {updates}", q.id);
            lengths.insert(updates);
        }
        assert_eq!(lengths.len(), 5, "{lengths:?}");
    }
}

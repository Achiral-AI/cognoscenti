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

/// How project names and note wording are produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// Unique coined project names and one template per kind of note. The
    /// registered dataset: its bytes are pinned by a test.
    #[default]
    Codename,
    /// Two-word natural project names that recur across trials and several
    /// wordings per kind of note, so a unique rare token is not a giveaway.
    NaturalNames,
}

/// Natural words only, so no project carries a coined token. Every word is
/// shared by about 25 projects; only the pair is unique. None collides with a
/// slot value (`monthly`, `quarterly`, `annual`) or a lint marker.
pub(crate) const NATURAL_ADJECTIVES: &[&str] = &[
    "Northern", "Southern", "Eastern", "Western", "Central", "Coastal", "Urban", "Rural", "Mobile",
    "Digital", "Global", "Local", "Regional", "Public", "Private", "Shared", "Open", "Rapid",
    "Smart", "Secure", "Unified", "Modern", "Classic", "Premium", "Basic", "Advanced", "Hybrid",
    "Remote", "Internal", "External", "Bright", "Swift", "Prime", "Lean", "Priority", "Express",
];
pub(crate) const NATURAL_NOUNS: &[&str] = &[
    "Portal",
    "Dashboard",
    "Gateway",
    "Checkout",
    "Subscriptions",
    "Inventory",
    "Catalog",
    "Support",
    "Onboarding",
    "Analytics",
    "Reporting",
    "Payments",
    "Search",
    "Messaging",
    "Scheduling",
    "Fulfillment",
    "Procurement",
    "Payroll",
    "Ledger",
    "Registry",
    "Directory",
    "Archive",
    "Console",
    "Workflow",
    "Pipeline",
    "Marketplace",
    "Loyalty",
    "Shipping",
    "Booking",
    "Ticketing",
    "Notifications",
    "Identity",
    "Compliance",
    "Forecast",
    "Storefront",
    "Helpdesk",
];

// Wording per kind of note. Index 0 is the registered dataset's exact text;
// the natural-names profile draws among all of them, the default never does.
const T_PRIVATE_NOTE: &[&str] = &[
    "Note to self: worried about {project} {concern}; raise it before the next planning round.",
    "Reminder to myself: {project} {concern} is on my mind, bring it up at the next planning round.",
    "Heads-up for me: I keep coming back to {concern} on {project}; flag it in planning.",
    "Personal note: concerned about {concern} for {project}, want it discussed before planning.",
];
const T_CONCERN_Q: &[&str] = &[
    "Has anyone raised a concern about {project} {concern}?",
    "Did anyone flag {concern} on {project}?",
    "Is anyone worried about {concern} for {project}?",
];
const T_REVIEW: &[&str] = &[
    "The {project} {concern} review is set for {date}.",
    "{project}'s {concern} review takes place on {date}.",
    "We scheduled the {concern} review for {project} on {date}.",
    "Review of {concern} for {project}: {date}.",
];
const T_REVIEW_Q: &[&str] = &[
    "When is the {project} {concern} review?",
    "When does the {concern} review for {project} happen?",
    "What day is the {project} {concern} review?",
];
const T_OWNER_Q: &[&str] = &[
    "What concern did I note about {project} {concern}?",
    "What did I write down about {concern} on {project}?",
    "Which worry did I record for {project} regarding {concern}?",
];
const T_DECIDED: &[&str] = &[
    "{project} decided its {slot} is {value}.",
    "For {project}, the {slot} is {value}.",
    "We settled on {value} for the {project} {slot}.",
    "{project}: {slot} is {value}, agreed in the planning sync.",
];
const T_CHANGED: &[&str] = &[
    "{project} changed its {slot} to {value}.",
    "The {project} {slot} moved to {value}.",
    "{project} switched its {slot} to {value}.",
    "Update: {project} {slot} is now {value}.",
];
const T_ASIDE: &[&str] = &[
    "In passing, someone floated switching {project}'s {slot} to {value}.",
    "Someone suggested moving {project}'s {slot} to {value}, nothing decided.",
    "Informal idea: {project} {slot} might go to {value}.",
];
const T_SLOT_Q: &[&str] = &[
    "What is {project}'s {slot}?",
    "Tell me the {slot} for {project}.",
    "{project}: what is the {slot}?",
    "What did {project} say its {slot} is?",
];
const T_OWN_Q: &[&str] = &[
    "What is {who} {project} {slot}?",
    "Remind me what {who} {project} {slot} is.",
    "{project}: what is {who} {slot}?",
];
const T_REPHRASE: &[&str] = &[
    "Which {slot} did {project} settle on?",
    "Remind me of {project}'s {slot}.",
    "{project}: remind me which {slot} was chosen.",
];
const T_INDIVIDUAL: &[&str] = &["{lead}: {project} {slot} {verb} {value}."];
const T_HANDOFF_FIRST: &[&str] = &[
    "Handoff: {project} {slot} is {value}.",
    "Passing along: {project}'s {slot} is {value}.",
    "Handing over {project}: {slot} is {value}.",
];
const T_HANDOFF_CHANGE: &[&str] = &[
    "Handoff: {project} {slot} changed to {value}.",
    "Passing along: {project}'s {slot} is now {value}.",
    "Handing over {project}: {slot} moved to {value}.",
];
const T_SCRATCH: &[&str] = &[
    "Scratchpad: unsure whether {project} {slot} will hold; leaning to {value}.",
    "Scratchpad: not sure the {project} {slot} will stick; leaning towards {value}.",
    "Draft thinking: {project} {slot} might shift, probably {value}.",
];
const LEADS_USER: &[&str] = &["Note to self", "Reminder", "For my own tracking"];
const LEADS_AGENT: &[&str] = &["Task state", "Working state", "Status for this task"];
const VERBS_FIRST: &[&str] = &["is", "stands at", "has been set to"];
const VERBS_CHANGE: &[&str] = &["moved to", "changed to", "was updated to"];

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
    pub profile: Profile,
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
            profile: Profile::Codename,
        }
    }
}

// Word lists avoid every `lint::ANSWER_REVEALING_MARKERS` substring (note
// "concurrent" and "current" collide), so the lint can stay a hard gate.
pub(crate) const USERS: &[&str] = &[
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
pub(crate) struct Attribute {
    pub(crate) name: &'static str,
    pub(crate) alias: &'static str,
    pub(crate) values: &'static [&'static str],
}

pub(crate) const ATTRIBUTES: &[Attribute] = &[
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

fn fill(template: &str, fields: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in fields {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

struct Builder {
    profile: Profile,
    rng: StdRng,
    names: Vec<String>,
    day: i64,
    items: Vec<StreamItem>,
}

impl Builder {
    fn natural(&self) -> bool {
        self.profile == Profile::NaturalNames
    }

    /// Fills `{key}` placeholders. The default profile always takes variant
    /// 0 and draws nothing from the rng, so its bytes cannot move.
    fn text(&mut self, variants: &[&str], fields: &[(&str, &str)]) -> String {
        let pick = if self.natural() {
            self.rng.gen_range(0..variants.len())
        } else {
            0
        };
        self.finish(fill(variants[pick], fields))
    }

    /// The natural profile reads "Payments' launch date", not "Payments's".
    fn finish(&self, text: String) -> String {
        if self.natural() {
            text.replace("s's", "s'")
        } else {
            text
        }
    }

    /// Like `text`, but the default profile picks variant 0 or 1 by the
    /// item index, as the registered dataset always did.
    fn text_by_parity(
        &mut self,
        variants: &[&str],
        index: usize,
        fields: &[(&str, &str)],
    ) -> String {
        let pick = if self.natural() {
            self.rng.gen_range(0..variants.len())
        } else if index.is_multiple_of(2) {
            0
        } else {
            1
        };
        self.finish(fill(variants[pick], fields))
    }

    /// The word an event uses for a fact slot. The natural profile mixes the
    /// name with its alias, so a query's wording does not always match.
    fn event_slot(&mut self, attribute: &Attribute) -> &'static str {
        if self.natural() && self.rng.gen_range(0..2) == 1 {
            attribute.alias
        } else {
            attribute.name
        }
    }

    /// The word a question uses for a slot. The default profile uses the
    /// alias when `alias_in_default` and the name otherwise.
    fn question_slot(&mut self, attribute: &Attribute, alias_in_default: bool) -> &'static str {
        let alias = if self.natural() {
            self.rng.gen_range(0..2) == 1
        } else {
            alias_in_default
        };
        if alias {
            attribute.alias
        } else {
            attribute.name
        }
    }

    fn pick_word(&mut self, words: &'static [&'static str]) -> &'static str {
        if self.natural() {
            words[self.rng.gen_range(0..words.len())]
        } else {
            words[0]
        }
    }

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
                external_id: project.to_lowercase().replace(' ', "-"),
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
        let note = self.text(
            T_PRIVATE_NOTE,
            &[("project", &project), ("concern", concern)],
        );
        self.event(
            format!("{id}-private"),
            note,
            0,
            owner,
            private_scope,
            &project,
            None,
        );
        if index.is_multiple_of(2) {
            let query = self.text(T_CONCERN_Q, &[("project", &project), ("concern", concern)]);
            self.question(
                format!("{id}-q"),
                query,
                240,
                asker,
                Vec::new(),
                "Nothing org-visible on that.",
            );
        } else {
            let date = format!("Apr {}", self.rng.gen_range(1..29));
            let author = self.user();
            let review = self.text(
                T_REVIEW,
                &[("project", &project), ("concern", concern), ("date", &date)],
            );
            self.event(
                format!("{id}-org"),
                review,
                60,
                author,
                Self::org_scope(),
                &project,
                None,
            );
            let query = self.text(T_REVIEW_Q, &[("project", &project), ("concern", concern)]);
            self.question(
                format!("{id}-q"),
                query,
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
        let note = self.text(
            T_PRIVATE_NOTE,
            &[("project", &project), ("concern", concern)],
        );
        self.event(
            format!("{id}-private"),
            note,
            0,
            owner.clone(),
            scope,
            &project,
            None,
        );
        let date = format!("Apr {}", self.rng.gen_range(1..29));
        let author = self.user();
        let review = self.text(
            T_REVIEW,
            &[("project", &project), ("concern", concern), ("date", &date)],
        );
        self.event(
            format!("{id}-org"),
            review,
            60,
            author,
            Self::org_scope(),
            &project,
            None,
        );
        let query = self.text(T_OWNER_Q, &[("project", &project), ("concern", concern)]);
        self.question(
            format!("{id}-q"),
            query,
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
        let (scope_kind, leads) = if is_agent {
            (ScopeKind::Agent, LEADS_AGENT)
        } else {
            (ScopeKind::User, LEADS_USER)
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
            let verb = self.pick_word(if step == 0 { VERBS_FIRST } else { VERBS_CHANGE });
            let lead = self.pick_word(leads);
            let slot = self.event_slot(target);
            let content = self.text(
                T_INDIVIDUAL,
                &[
                    ("lead", lead),
                    ("project", &project),
                    ("slot", slot),
                    ("verb", verb),
                    ("value", value),
                ],
            );
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
            let verb = self.pick_word(VERBS_FIRST);
            let lead = self.pick_word(leads);
            let slot = self.event_slot(other);
            let content = self.text(
                T_INDIVIDUAL,
                &[
                    ("lead", lead),
                    ("project", &project),
                    ("slot", slot),
                    ("verb", verb),
                    ("value", value),
                ],
            );
            self.event(
                format!("{id}-filler-{}", n + 1),
                content,
                minutes,
                actor.clone(),
                scope.clone(),
                &project,
                None,
            );
            minutes += 30;
        }
        let slot_words = self.question_slot(target, !index.is_multiple_of(2));
        let pronoun = if is_agent { "the" } else { "my" };
        let query = self.text(
            T_OWN_Q,
            &[
                ("who", pronoun),
                ("project", &project),
                ("slot", slot_words),
            ],
        );
        self.question(
            format!("{id}-q"),
            query,
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

        let slot = self.event_slot(attribute);
        let first = self.text(
            T_HANDOFF_FIRST,
            &[("project", &project), ("slot", slot), ("value", values[0])],
        );
        self.event(
            format!("{id}-e1"),
            first,
            0,
            writer.clone(),
            Self::org_scope(),
            &project,
            None,
        );
        let slot = self.event_slot(attribute);
        let scratch = self.text(
            T_SCRATCH,
            &[("project", &project), ("slot", slot), ("value", values[2])],
        );
        self.event(
            format!("{id}-scratch"),
            scratch,
            30,
            writer.clone(),
            Scope {
                kind: ScopeKind::Agent,
                id: writer.id.clone(),
            },
            &project,
            None,
        );
        let slot = self.event_slot(attribute);
        let change = self.text(
            T_HANDOFF_CHANGE,
            &[("project", &project), ("slot", slot), ("value", values[1])],
        );
        self.event(
            format!("{id}-e2"),
            change,
            60,
            writer,
            Self::org_scope(),
            &project,
            Some(format!("{id}-e1")),
        );
        let slot_words = self.question_slot(attribute, !index.is_multiple_of(2));
        let query = self.text(T_SLOT_Q, &[("project", &project), ("slot", slot_words)]);
        self.question(
            format!("{id}-q"),
            query,
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
            let slot = self.event_slot(attribute);
            let fields = [
                ("project", project.as_str()),
                ("slot", slot),
                ("value", values[step]),
            ];
            let content = self.text(if step == 0 { T_DECIDED } else { T_CHANGED }, &fields);
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
                let slot = self.event_slot(attribute);
                let aside = self.text(
                    T_ASIDE,
                    &[
                        ("project", &project),
                        ("slot", slot),
                        ("value", values[length]),
                    ],
                );
                self.event(
                    format!("{id}-aside"),
                    aside,
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
        let slot = self.question_slot(attribute, false);
        let query = self.text(T_SLOT_Q, &[("project", &project), ("slot", slot)]);
        self.question(
            format!("{id}-q"),
            query,
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
        let slot = self.event_slot(target);
        let gold = self.text(
            T_DECIDED,
            &[("project", &project), ("slot", slot), ("value", value)],
        );
        self.event(
            format!("{id}-gold"),
            gold,
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
            let slot = self.event_slot(other);
            let content = self.text(
                T_DECIDED,
                &[
                    ("project", &project),
                    ("slot", slot),
                    ("value", other_value),
                ],
            );
            self.event(
                format!("{id}-same-project-{}", n + 1),
                content,
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
        let slot = self.event_slot(target);
        let decoy = self.text(
            T_DECIDED,
            &[
                ("project", &decoy_project),
                ("slot", slot),
                ("value", decoy_value),
            ],
        );
        self.event(
            format!("{id}-same-slot"),
            decoy,
            minutes,
            author,
            Self::org_scope(),
            &decoy_project,
            None,
        );
        let slot_words = self.question_slot(target, !index.is_multiple_of(2));
        let asker = self.user();
        let query = self.text(T_SLOT_Q, &[("project", &project), ("slot", slot_words)]);
        self.question(
            format!("{id}-q"),
            query,
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
        let slot = self.event_slot(target);
        let gold = self.text(
            T_DECIDED,
            &[("project", &project), ("slot", slot), ("value", value)],
        );
        self.event(
            format!("{id}-gold"),
            gold,
            0,
            author,
            Self::org_scope(),
            &project,
            None,
        );
        for (n, other) in slots[1..4].iter().enumerate() {
            let other_value = other.values[self.rng.gen_range(0..other.values.len())];
            let author = self.user();
            let slot = self.event_slot(other);
            let content = self.text(
                T_DECIDED,
                &[
                    ("project", &project),
                    ("slot", slot),
                    ("value", other_value),
                ],
            );
            self.event(
                format!("{id}-same-project-{}", n + 1),
                content,
                20 * (n as i64 + 1),
                author,
                Self::org_scope(),
                &project,
                None,
            );
        }
        let asker = self.user();
        let original_slot = self.question_slot(target, false);
        let original = self.text(T_SLOT_Q, &[("project", &project), ("slot", original_slot)]);
        self.question(
            format!("{id}-q"),
            original,
            180,
            asker.clone(),
            vec![format!("{id}-gold")],
            value,
        );
        let reworded = self.text_by_parity(
            T_REPHRASE,
            index,
            &[("project", &project), ("slot", target.alias)],
        );
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
        let slot = self.event_slot(target);
        let gold = self.text(
            T_DECIDED,
            &[("project", &project), ("slot", slot), ("value", value)],
        );
        self.event(
            format!("{id}-gold"),
            gold,
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
            let update_slot = self.event_slot(attribute);
            let content = self.text(
                if taken.is_none() {
                    T_DECIDED
                } else {
                    T_CHANGED
                },
                &[
                    ("project", &project),
                    ("slot", update_slot),
                    ("value", next),
                ],
            );
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
        let slot_words = self.question_slot(target, !index.is_multiple_of(2));
        let asker = self.user();
        let query = self.text(T_SLOT_Q, &[("project", &project), ("slot", slot_words)]);
        self.question(
            format!("{id}-q"),
            query,
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
    let mut names: Vec<String> = match config.profile {
        Profile::Codename => PREFIXES
            .iter()
            .flat_map(|prefix| {
                SUFFIXES
                    .iter()
                    .map(move |suffix| format!("{prefix}{suffix}"))
            })
            .collect(),
        Profile::NaturalNames => NATURAL_ADJECTIVES
            .iter()
            .flat_map(|adjective| {
                NATURAL_NOUNS
                    .iter()
                    .map(move |noun| format!("{adjective} {noun}"))
            })
            .collect(),
    };
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
        profile: config.profile,
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
         paraphrase_pairs={} stable_items={}{}\n",
        config.seed,
        config.scope_trials,
        config.supersession_chains,
        config.retrieval_questions,
        config.owner_trials,
        config.individual_items,
        config.agent_items,
        config.paraphrase_pairs,
        config.stable_items,
        match config.profile {
            Profile::Codename => "",
            Profile::NaturalNames => " profile=natural-names",
        }
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
        // The header records the seed, so compare the items themselves.
        let items = |c: &GeneratorConfig| {
            let s = to_jsonl(&generate(c), c);
            s.lines()
                .filter(|l| !l.starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_ne!(items(&a), items(&b));
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
            .filter(|e| e.id.starts_with("scope-") && e.id.ends_with("-private"))
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
            profile: Profile::Codename,
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
            assert_eq!(gold.id, q.id.replace("-q", "-e2"), "gold is the last post");
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

    const REGISTERED_DEFAULT_SHA256: &str =
        "2fc8d6c2ba4e9cff4979945653264ddcc79cec1e86e7038c8a0feccf7f5693e2";

    fn natural(seed: u64) -> GeneratorConfig {
        GeneratorConfig {
            profile: Profile::NaturalNames,
            ..GeneratorConfig::new(seed)
        }
    }

    fn project_names(stream: &Stream) -> Vec<String> {
        let mut names: Vec<String> = stream
            .events()
            .flat_map(|e| e.entities.iter().map(|r| r.display_name.clone()))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    #[test]
    fn the_default_profile_still_writes_the_registered_bytes() {
        // The registered dataset's checksum is in the pre-registration, so
        // adding a profile must not move a single byte of the default.
        let config = GeneratorConfig::new(0);
        let bytes = to_jsonl(&generate(&config), &config);
        assert_eq!(
            crate::manifest::sha256_hex(bytes.as_bytes()),
            REGISTERED_DEFAULT_SHA256
        );
    }

    #[test]
    fn the_natural_names_profile_is_deterministic_and_seeded() {
        let a = natural(3);
        assert_eq!(to_jsonl(&generate(&a), &a), to_jsonl(&generate(&a), &a));
        let items = |c: &GeneratorConfig| {
            to_jsonl(&generate(c), c)
                .lines()
                .filter(|l| !l.starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_ne!(items(&natural(3)), items(&natural(4)));
    }

    #[test]
    fn the_natural_names_profile_differs_from_the_default_and_says_so_in_its_header() {
        let n = natural(0);
        let text = to_jsonl(&generate(&n), &n);
        assert!(text
            .lines()
            .nth(1)
            .unwrap()
            .contains("profile=natural-names"));
        let d = GeneratorConfig::new(0);
        assert_ne!(text, to_jsonl(&generate(&d), &d));
    }

    #[test]
    fn the_natural_names_profile_keeps_every_category_count() {
        let n = generate(&natural(0));
        let d = generate(&GeneratorConfig::new(0));
        for prefix in [
            "scope-",
            "owner-",
            "individual-",
            "agent-",
            "supersede-",
            "retrieve-",
            "paraphrase-",
            "rephrase-",
            "stable-",
        ] {
            assert_eq!(
                category_questions(&n, prefix).len(),
                category_questions(&d, prefix).len(),
                "{prefix}"
            );
        }
        assert_eq!(n.questions().count(), 860);
    }

    #[test]
    fn natural_projects_are_two_natural_words_that_recur_across_trials() {
        let stream = generate(&natural(0));
        let names = project_names(&stream);
        assert!(names.len() >= 860, "{} names", names.len());
        let mut uses: HashMap<&str, usize> = HashMap::new();
        for name in &names {
            let words: Vec<&str> = name.split(' ').collect();
            assert_eq!(words.len(), 2, "{name}");
            assert!(NATURAL_ADJECTIVES.contains(&words[0]), "{name}");
            assert!(NATURAL_NOUNS.contains(&words[1]), "{name}");
            for word in words {
                *uses.entry(word).or_default() += 1;
            }
        }
        // No coined token appears in only one project: every word is shared.
        let rarest = uses.values().min().copied().unwrap();
        assert!(
            rarest >= 3,
            "a name word is used by only {rarest} project(s)"
        );
    }

    #[test]
    fn natural_profile_facts_are_not_all_worded_the_same_way() {
        let stream = generate(&natural(0));
        let names = project_names(&stream);
        let shape = |content: &str| {
            let mut masked = content.to_string();
            for name in &names {
                masked = masked.replace(name.as_str(), "P");
            }
            masked
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        };
        let opening: HashSet<String> = stream
            .events()
            .filter(|e| e.id.starts_with("retrieve-") && e.id.ends_with("-gold"))
            .map(|e| shape(&e.content))
            .collect();
        assert!(opening.len() >= 3, "{opening:?}");
        let questions: HashSet<String> = stream
            .questions()
            .filter(|q| q.id.starts_with("retrieve-"))
            .map(|q| shape(&q.query))
            .collect();
        assert!(questions.len() >= 3, "{questions:?}");
    }

    #[test]
    fn the_natural_names_stream_passes_lint_and_keeps_queries_unique() {
        let config = natural(0);
        let stream = generate(&config);
        assert!(lint(&stream).is_empty(), "{:?}", lint(&stream));
        let reloaded = Stream::from_jsonl(&to_jsonl(&stream, &config)).unwrap();
        assert_eq!(reloaded.items.len(), stream.items.len());
        let queries: HashSet<&str> = stream.questions().map(|q| q.query.as_str()).collect();
        assert_eq!(queries.len(), stream.questions().count());
    }

    #[test]
    fn natural_profile_possessives_read_naturally() {
        let stream = generate(&natural(0));
        let bad: Vec<&str> = stream
            .events()
            .map(|e| e.content.as_str())
            .chain(stream.questions().map(|q| q.query.as_str()))
            .filter(|text| text.contains("s's"))
            .take(3)
            .collect();
        assert!(bad.is_empty(), "{bad:?}");
    }

    #[test]
    fn natural_events_use_both_a_slot_name_and_its_alias() {
        let stream = generate(&natural(0));
        let gold: Vec<&str> = stream
            .events()
            .filter(|e| e.id.starts_with("retrieve-") && e.id.ends_with("-gold"))
            .map(|e| e.content.as_str())
            .collect();
        let uses = |word: fn(&Attribute) -> &'static str| {
            gold.iter()
                .filter(|c| ATTRIBUTES.iter().any(|a| c.contains(word(a))))
                .count()
        };
        let by_name = uses(|a| a.name);
        let by_alias = uses(|a| a.alias);
        assert!(
            by_name > 10 && by_alias > 10,
            "{by_name} by name, {by_alias} by alias"
        );
    }
}

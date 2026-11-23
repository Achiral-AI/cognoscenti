use super::{Edge, Scenario, TeamBatch, TeamItem};
use crate::determinism::seeded_rng;
use crate::stream::{
    Actor, ActorKind, EntityReference, Scope, ScopeKind, Stream, StreamEvent, StreamItem,
    StreamQuestion, ValidityWindow,
};
use chrono::{DateTime, Duration, TimeZone, Utc};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::Rng;

#[derive(Debug, Clone)]
pub struct TeamConfig {
    pub seed: u64,
    pub supersede: usize,
    pub control_keep: usize,
    pub control_asof: usize,
    pub provenance: usize,
    pub lineage: usize,
    pub leaver: usize,
}

impl TeamConfig {
    /// The first reviewable batch: 64 items.
    pub fn first_batch(seed: u64) -> Self {
        Self {
            seed,
            supersede: 16,
            control_keep: 8,
            control_asof: 8,
            provenance: 14,
            lineage: 10,
            leaver: 8,
        }
    }
    /// The Stage 4 size: 500 items.
    pub fn full(seed: u64) -> Self {
        Self {
            seed,
            supersede: 120,
            control_keep: 60,
            control_asof: 60,
            provenance: 110,
            lineage: 90,
            leaver: 60,
        }
    }
    pub fn count(&self, s: Scenario) -> usize {
        match s {
            Scenario::Supersede => self.supersede,
            Scenario::ControlKeep => self.control_keep,
            Scenario::ControlAsOf => self.control_asof,
            Scenario::Provenance => self.provenance,
            Scenario::Lineage => self.lineage,
            Scenario::Leaver => self.leaver,
        }
    }
}

const PREFIX: [&str; 30] = [
    "Quartz", "Onyx", "Pebble", "Cinder", "Marlow", "Tern", "Ember", "Juniper", "Lumen", "Ferro",
    "Halcyon", "Sable", "Thistle", "Vireo", "Wren", "Zephyr", "Alder", "Basalt", "Corvid", "Dune",
    "Fjord", "Garnet", "Heron", "Indigo", "Kestrel", "Larch", "Mica", "Nettle", "Osprey", "Perch",
];
const SUFFIX: [&str; 30] = [
    "shoal", "orchard", "mesa", "fen", "gate", "ridge", "loop", "works", "pier", "hollow",
    "bridge", "field", "cove", "spire", "vault", "grove", "wharf", "lane", "crest", "dell",
    "reach", "stack", "forge", "yard", "marsh", "peak", "ford", "mill", "nest", "path",
];
const PEOPLE: [&str; 20] = [
    "ada", "ben", "chi", "dev", "eli", "fay", "gus", "hana", "ivo", "jo", "kai", "lena", "mo",
    "nia", "omar", "pia", "quinn", "rae", "sol", "tess",
];
const SOURCES: [&str; 6] = [
    "slack",
    "docs",
    "email",
    "meeting-notes",
    "tracker",
    "contract",
];
const ORIGIN_SOURCES: [&str; 4] = ["docs", "contract", "tracker", "meeting-notes"];
const ECHO_SOURCES: [&str; 2] = ["slack", "email"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const PROBLEMS: [&str; 10] = [
    "checkout timeouts",
    "slow exports",
    "duplicate invoices",
    "login failures",
    "missed alerts",
    "report drift",
    "sync conflicts",
    "bounced emails",
    "stuck uploads",
    "double bookings",
];
const CONSTRAINTS: [&str; 10] = [
    "a freeze until the audit closes",
    "a single on-call engineer",
    "the EU data rule",
    "a fixed Q3 budget",
    "no new vendors this quarter",
    "a contract that ends in March",
    "a hard launch date",
    "a two-person team",
    "the security review queue",
    "a legacy client",
];
const CHOICES: [&str; 12] = [
    "a queue-based retry",
    "a nightly batch",
    "a read replica",
    "an in-house parser",
    "a managed gateway",
    "a feature flag rollout",
    "a cached summary",
    "a rules engine",
    "a staged migration",
    "a vendor SDK",
    "an event log",
    "a manual review step",
];
const NEW_INFO: [&str; 8] = [
    "costs came in at twice the estimate",
    "the vendor dropped support for the SDK",
    "two pilot customers churned over it",
    "the audit flagged the approach",
    "latency doubled under real load",
    "the owner team was reorganised",
    "a cheaper option appeared",
    "legal rejected the data flow",
];

type ValueFn = fn(&mut StdRng) -> String;
struct Attr {
    phrase: &'static str,
    value: ValueFn,
}

fn pick<'a>(rng: &mut StdRng, pool: &[&'a str]) -> &'a str {
    pool[rng.gen_range(0..pool.len())]
}
fn date_value(rng: &mut StdRng) -> String {
    format!("{} {}", MONTHS[rng.gen_range(0..12)], rng.gen_range(1..=28))
}
fn money_value(rng: &mut StdRng) -> String {
    format!("${}k", rng.gen_range(5..400) * 5)
}
fn seat_value(rng: &mut StdRng) -> String {
    format!("${} per seat", rng.gen_range(4..90))
}
fn days_value(rng: &mut StdRng) -> String {
    format!("{} days", rng.gen_range(2..60) * 5)
}
fn person_value(rng: &mut StdRng) -> String {
    let p = pick(rng, &PEOPLE);
    let mut c = p.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}
fn target_value(rng: &mut StdRng) -> String {
    pick(
        rng,
        &[
            "Lambda",
            "Fargate",
            "Cloud Run",
            "Nomad",
            "Kubernetes",
            "Fly",
            "ECS",
            "Vercel",
            "bare metal",
            "Heroku",
        ],
    )
    .to_string()
}
fn cadence_value(rng: &mut StdRng) -> String {
    pick(
        rng,
        &[
            "daily",
            "weekly",
            "twice a week",
            "fortnightly",
            "monthly",
            "per release",
        ],
    )
    .to_string()
}
fn vendor_value(rng: &mut StdRng) -> String {
    pick(
        rng,
        &[
            "Northwind",
            "Contoso",
            "Initech",
            "Globex",
            "Hooli",
            "Umbrella",
            "Stark",
            "Wayne",
            "Tyrell",
            "Vandelay",
        ],
    )
    .to_string()
}
fn tier_value(rng: &mut StdRng) -> String {
    pick(
        rng,
        &["Gold", "Silver", "Bronze", "Platinum", "Basic", "Premier"],
    )
    .to_string()
}
fn region_value(rng: &mut StdRng) -> String {
    pick(
        rng,
        &[
            "eu-west",
            "us-east",
            "ap-south",
            "us-west",
            "eu-central",
            "sa-east",
        ],
    )
    .to_string()
}

const ATTRS: [Attr; 12] = [
    Attr {
        phrase: "launch date",
        value: date_value,
    },
    Attr {
        phrase: "on-call owner",
        value: person_value,
    },
    Attr {
        phrase: "deploy target",
        value: target_value,
    },
    Attr {
        phrase: "review cadence",
        value: cadence_value,
    },
    Attr {
        phrase: "budget",
        value: money_value,
    },
    Attr {
        phrase: "main vendor",
        value: vendor_value,
    },
    Attr {
        phrase: "support tier",
        value: tier_value,
    },
    Attr {
        phrase: "data region",
        value: region_value,
    },
    Attr {
        phrase: "renewal date",
        value: date_value,
    },
    Attr {
        phrase: "price",
        value: seat_value,
    },
    Attr {
        phrase: "primary contact",
        value: person_value,
    },
    Attr {
        phrase: "retention period",
        value: days_value,
    },
];

fn statement(v: usize, p: &str, attr: &str, value: &str) -> String {
    match v % 5 {
        0 => format!("{p}'s {attr} is {value}."),
        1 => format!("We settled on {value} as the {attr} for {p}."),
        2 => format!("Update for {p}: {attr} = {value}."),
        3 => format!("The {attr} for {p} is {value}, confirmed in the sync."),
        _ => format!("Recording that {p} has {value} as its {attr}."),
    }
}
fn revision(v: usize, p: &str, attr: &str, value: &str) -> String {
    match v % 4 {
        0 => format!("{p}'s {attr} moves to {value}."),
        1 => format!("Replacing the {attr} for {p}: it is {value}."),
        2 => format!("{p}: {attr} changed, now {value}."),
        _ => format!("Switching {p}'s {attr} to {value}."),
    }
}
fn echo(v: usize, p: &str, attr: &str, value: &str) -> String {
    match v % 4 {
        0 => format!("Heard that {p}'s {attr} is {value}."),
        1 => format!("Passing along: {p} {attr} is {value}, per what I saw."),
        2 => format!("FYI for the team, the {attr} on {p} is {value}."),
        _ => format!("Someone mentioned {value} as the {attr} for {p}."),
    }
}
fn ask(v: usize, p: &str, attr: &str) -> String {
    match v % 5 {
        0 => format!("What is {p}'s {attr}?"),
        1 => format!("Which {attr} did {p} end up with?"),
        2 => format!("Remind me of the {attr} for {p}."),
        3 => format!("I need the {attr} for {p}; what is it?"),
        _ => format!("What should I tell the customer about {p}'s {attr}?"),
    }
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
        id: "acme-team".to_string(),
    }
}

struct Ctx {
    rng: StdRng,
    start: DateTime<Utc>,
    names: Vec<(String, String)>,
    next_name: usize,
    slot: i64,
    items: Vec<StreamItem>,
    team: Vec<TeamItem>,
}

struct Ev {
    id: String,
    content: String,
    at: DateTime<Utc>,
    author: String,
    scope: Scope,
    source: String,
    supersedes: Option<String>,
    project: (String, String),
}

impl Ctx {
    fn project(&mut self) -> (String, String) {
        let p = self.names[self.next_name].clone();
        self.next_name += 1;
        p
    }
    /// A person not in `exclude`.
    fn person(&mut self, exclude: &[&str]) -> String {
        loop {
            let p = pick(&mut self.rng, &PEOPLE);
            if !exclude.contains(&p) {
                return p.to_string();
            }
        }
    }
    /// Two different values for one attribute.
    fn values(&mut self, attr: usize, n: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        while out.len() < n {
            let v = (ATTRS[attr].value)(&mut self.rng);
            if !out.contains(&v) {
                out.push(v);
            }
        }
        out
    }
    fn event(&mut self, e: Ev) {
        self.items.push(StreamItem::Event(StreamEvent {
            id: e.id,
            content: e.content,
            timestamp: e.at,
            actor: user(&e.author),
            scope: e.scope,
            source: e.source,
            supersedes: e.supersedes,
            entities: vec![EntityReference {
                external_id: e.project.1,
                display_name: e.project.0,
                entity_type: "project".to_string(),
            }],
        }));
    }
    #[allow(clippy::too_many_arguments)] // one record's fields, passed flat
    fn question(
        &mut self,
        id: &str,
        query: &str,
        at: DateTime<Utc>,
        asker: &str,
        gold: &[String],
        answer: &str,
        validity: Option<ValidityWindow>,
    ) {
        self.items.push(StreamItem::Question(StreamQuestion {
            id: id.to_string(),
            query: query.to_string(),
            asked_at: at,
            asked_by: user(asker),
            gold_event_ids: gold.to_vec(),
            gold_answer: Some(answer.to_string()),
            validity,
            expected_gap_entity_ids: Vec::new(),
        }));
    }
    /// A time at `slot` days after the start, plus a gap.
    fn base(&mut self) -> DateTime<Utc> {
        self.slot += 1;
        let h = self.rng.gen_range(8..17);
        self.start + Duration::days(self.slot) + Duration::hours(h)
    }
    fn gap_days(&mut self, lo: i64, hi: i64) -> Duration {
        Duration::days(self.rng.gen_range(lo..=hi)) + Duration::minutes(self.rng.gen_range(0..50))
    }
}

pub fn generate_team(config: &TeamConfig) -> TeamBatch {
    let mut rng = seeded_rng(config.seed);
    let mut names: Vec<(String, String)> = PREFIX
        .iter()
        .flat_map(|p| {
            SUFFIX
                .iter()
                .map(move |s| (format!("{p}{s}"), format!("{p}{s}").to_lowercase()))
        })
        .collect();
    names.shuffle(&mut rng);
    let mut slots: Vec<(Scenario, usize)> = Scenario::ALL
        .into_iter()
        .flat_map(|s| (0..config.count(s)).map(move |i| (s, i)))
        .collect();
    slots.shuffle(&mut rng);
    let mut ctx = Ctx {
        rng,
        start: Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap(),
        names,
        next_name: 0,
        slot: 0,
        items: Vec::new(),
        team: Vec::new(),
    };
    for (scenario, n) in slots {
        let code = scenario.code();
        let id = format!("team-{code}-{n:03}");
        match scenario {
            Scenario::Supersede => supersede(&mut ctx, &id, n),
            Scenario::ControlKeep => control_keep(&mut ctx, &id, n),
            Scenario::ControlAsOf => control_asof(&mut ctx, &id, n),
            Scenario::Provenance => provenance(&mut ctx, &id, n),
            Scenario::Lineage => lineage(&mut ctx, &id, n),
            Scenario::Leaver => leaver(&mut ctx, &id, n),
        }
    }
    // Items were built one after another; the stream is in time order.
    ctx.items.sort_by_key(|i| i.timestamp());
    ctx.team.sort_by(|a, b| a.id.cmp(&b.id));
    TeamBatch {
        stream: Stream { items: ctx.items },
        items: ctx.team,
    }
}

#[allow(clippy::too_many_arguments)] // one record's fields, passed flat
fn finish(
    ctx: &mut Ctx,
    scenario: Scenario,
    id: &str,
    question: &str,
    answer: &str,
    gold: Vec<String>,
    negatives: Vec<String>,
    edges: Vec<Edge>,
    events: Vec<String>,
) {
    ctx.team.push(TeamItem {
        id: id.to_string(),
        scenario,
        question_id: format!("{id}-q"),
        question: question.to_string(),
        gold_answer: answer.to_string(),
        gold_evidence: gold,
        hard_negatives: negatives,
        gold_edges: edges,
        event_ids: events,
    });
}

fn supersede(ctx: &mut Ctx, id: &str, n: usize) {
    let project = ctx.project();
    let attr = ctx.rng.gen_range(0..ATTRS.len());
    let links = 2 + ctx.rng.gen_range(0..3);
    let values = ctx.values(attr, links);
    let mut at = ctx.base();
    let mut authors: Vec<String> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    for (k, value) in values.iter().enumerate() {
        let author = ctx.person(&[]);
        let content = if k == 0 {
            statement(n + k, &project.0, ATTRS[attr].phrase, value)
        } else {
            revision(n + k, &project.0, ATTRS[attr].phrase, value)
        };
        let eid = format!("{id}-e{k}");
        let source = pick(&mut ctx.rng, &SOURCES).to_string();
        ctx.event(Ev {
            id: eid.clone(),
            content,
            at,
            author: author.clone(),
            scope: org(),
            source,
            supersedes: ids.last().cloned(),
            project: project.clone(),
        });
        authors.push(author);
        ids.push(eid);
        at += ctx.gap_days(5, 20);
    }
    let names: Vec<&str> = authors.iter().map(String::as_str).collect();
    let asker = ctx.person(&names);
    let question = ask(n / 2 + 1, &project.0, ATTRS[attr].phrase);
    let last = ids.last().unwrap().clone();
    ctx.question(
        &format!("{id}-q"),
        &question,
        at,
        &asker,
        std::slice::from_ref(&last),
        values.last().unwrap(),
        None,
    );
    let negatives = ids[..ids.len() - 1].to_vec();
    finish(
        ctx,
        Scenario::Supersede,
        id,
        &question,
        values.last().unwrap(),
        vec![last],
        negatives,
        vec![],
        ids,
    );
}

fn control_keep(ctx: &mut Ctx, id: &str, n: usize) {
    let project = ctx.project();
    let other = ctx.project();
    let a = ctx.rng.gen_range(0..ATTRS.len());
    let b = (a + 1 + ctx.rng.gen_range(0..ATTRS.len() - 1)) % ATTRS.len();
    let va = ctx.values(a, 1).remove(0);
    let vb = ctx.values(b, 1).remove(0);
    let vc = ctx.values(a, 1).remove(0);
    let at = ctx.base();
    let (au, bu, cu) = (ctx.person(&[]), ctx.person(&[]), ctx.person(&[]));
    let (s0, s1, s2) = (
        pick(&mut ctx.rng, &SOURCES).to_string(),
        pick(&mut ctx.rng, &SOURCES).to_string(),
        pick(&mut ctx.rng, &SOURCES).to_string(),
    );
    let t1 = at + ctx.gap_days(4, 12);
    let t2 = t1 + ctx.gap_days(1, 8);
    let c0 = statement(n, &project.0, ATTRS[a].phrase, &va);
    let c1 = statement(n + 2, &project.0, ATTRS[b].phrase, &vb);
    let c2 = statement(n + 4, &other.0, ATTRS[a].phrase, &vc);
    let ids: Vec<String> = (0..3).map(|k| format!("{id}-e{k}")).collect();
    ctx.event(Ev {
        id: ids[0].clone(),
        content: c0,
        at,
        author: au.clone(),
        scope: org(),
        source: s0,
        supersedes: None,
        project: project.clone(),
    });
    ctx.event(Ev {
        id: ids[1].clone(),
        content: c1,
        at: t1,
        author: bu.clone(),
        scope: org(),
        source: s1,
        supersedes: None,
        project: project.clone(),
    });
    ctx.event(Ev {
        id: ids[2].clone(),
        content: c2,
        at: t2,
        author: cu.clone(),
        scope: org(),
        source: s2,
        supersedes: None,
        project: other,
    });
    let asker = ctx.person(&[&au, &bu, &cu]);
    let question = ask(n / 2 + 3, &project.0, ATTRS[a].phrase);
    let asked = t2 + ctx.gap_days(2, 9);
    ctx.question(
        &format!("{id}-q"),
        &question,
        asked,
        &asker,
        &[ids[0].clone()],
        &va,
        None,
    );
    finish(
        ctx,
        Scenario::ControlKeep,
        id,
        &question,
        &va,
        vec![ids[0].clone()],
        vec![ids[1].clone(), ids[2].clone()],
        vec![],
        ids,
    );
}

fn control_asof(ctx: &mut Ctx, id: &str, n: usize) {
    let project = ctx.project();
    let attr = ctx.rng.gen_range(0..ATTRS.len());
    let values = ctx.values(attr, 2);
    let t0 = ctx.base();
    let t1 = t0 + ctx.gap_days(12, 30);
    let (a0, a1) = (ctx.person(&[]), ctx.person(&[]));
    let (s0, s1) = (
        pick(&mut ctx.rng, &SOURCES).to_string(),
        pick(&mut ctx.rng, &SOURCES).to_string(),
    );
    let ids = vec![format!("{id}-e0"), format!("{id}-e1")];
    ctx.event(Ev {
        id: ids[0].clone(),
        content: statement(n, &project.0, ATTRS[attr].phrase, &values[0]),
        at: t0,
        author: a0.clone(),
        scope: org(),
        source: s0,
        supersedes: None,
        project: project.clone(),
    });
    ctx.event(Ev {
        id: ids[1].clone(),
        content: revision(n, &project.0, ATTRS[attr].phrase, &values[1]),
        at: t1,
        author: a1.clone(),
        scope: org(),
        source: s1,
        supersedes: Some(ids[0].clone()),
        project: project.clone(),
    });
    let when = (t0 + (t1 - t0) / 2).format("%b %-d").to_string();
    let (p, a) = (&project.0, ATTRS[attr].phrase);
    let question = match n % 3 {
        0 => format!("What was {p}'s {a} on {when}?"),
        1 => format!("As of {when}, what was the {a} for {p}?"),
        _ => format!("Back on {when}, which {a} did {p} have?"),
    };
    let asker = ctx.person(&[&a0, &a1]);
    let window = ValidityWindow {
        from: t0,
        until: Some(t1),
    };
    let asked = t1 + ctx.gap_days(3, 12);
    ctx.question(
        &format!("{id}-q"),
        &question,
        asked,
        &asker,
        &[ids[0].clone()],
        &values[0],
        Some(window),
    );
    finish(
        ctx,
        Scenario::ControlAsOf,
        id,
        &question,
        &values[0],
        vec![ids[0].clone()],
        vec![ids[1].clone()],
        vec![],
        ids,
    );
}

fn provenance(ctx: &mut Ctx, id: &str, n: usize) {
    let project = ctx.project();
    let attr = ctx.rng.gen_range(0..ATTRS.len());
    let value = ctx.values(attr, 1).remove(0);
    let t0 = ctx.base();
    let origin_author = ctx.person(&[]);
    let origin_source = pick(&mut ctx.rng, &ORIGIN_SOURCES).to_string();
    let mut ids = vec![format!("{id}-e0")];
    ctx.event(Ev {
        id: ids[0].clone(),
        content: statement(n, &project.0, ATTRS[attr].phrase, &value),
        at: t0,
        author: origin_author.clone(),
        scope: org(),
        source: origin_source.clone(),
        supersedes: None,
        project: project.clone(),
    });
    let echoes = 1 + n % 2;
    let mut at = t0;
    let mut used = vec![origin_author.clone()];
    for k in 0..echoes {
        at += ctx.gap_days(3, 15);
        let names: Vec<&str> = used.iter().map(String::as_str).collect();
        let who = ctx.person(&names);
        used.push(who.clone());
        let eid = format!("{id}-e{}", k + 1);
        let source = pick(&mut ctx.rng, &ECHO_SOURCES).to_string();
        ctx.event(Ev {
            id: eid.clone(),
            content: echo(n + k, &project.0, ATTRS[attr].phrase, &value),
            at,
            author: who,
            scope: org(),
            source,
            supersedes: None,
            project: project.clone(),
        });
        ids.push(eid);
    }
    let names: Vec<&str> = used.iter().map(String::as_str).collect();
    let asker = ctx.person(&names);
    let (p, a) = (&project.0, ATTRS[attr].phrase);
    let question = match n % 4 {
        0 => format!("Who first recorded the {a} for {p}, and where?"),
        1 => format!("Where did {p}'s {a} originally come from?"),
        2 => format!("Who is the source of the {a} on {p}?"),
        _ => format!("Trace {p}'s {a} back to whoever first wrote it down."),
    };
    let answer = format!("{origin_author} in {origin_source}");
    let asked = at + ctx.gap_days(2, 9);
    ctx.question(
        &format!("{id}-q"),
        &question,
        asked,
        &asker,
        &[ids[0].clone()],
        &answer,
        None,
    );
    let negatives = ids[1..].to_vec();
    finish(
        ctx,
        Scenario::Provenance,
        id,
        &question,
        &answer,
        vec![ids[0].clone()],
        negatives,
        vec![],
        ids,
    );
}

fn lineage(ctx: &mut Ctx, id: &str, n: usize) {
    let project = ctx.project();
    let decoy_project = ctx.project();
    let problem = pick(&mut ctx.rng, &PROBLEMS);
    let constraint = pick(&mut ctx.rng, &CONSTRAINTS);
    let choice = pick(&mut ctx.rng, &CHOICES);
    let rival = loop {
        let c = pick(&mut ctx.rng, &CHOICES);
        if c != choice {
            break c;
        }
    };
    let reversal = n.is_multiple_of(2);
    let p = project.0.clone();
    let trigger = match n % 4 {
        0 => format!("Support kept hearing about {problem} on {p}."),
        1 => format!("{p} users hit {problem} twice this week."),
        2 => format!("Three customers wrote in about {problem} with {p}."),
        _ => format!("{problem} showed up again in {p}'s weekly review."),
    };
    let bound = match n % 3 {
        0 => format!("Constraint on {p}: {constraint}."),
        1 => format!("{p} has to live with {constraint}."),
        _ => format!("Worth remembering that {constraint} applies to {p}."),
    };
    let decision = match (n / 2) % 4 {
        0 => format!("Decision: {p} goes with {choice} because of {problem} and {constraint}."),
        1 => format!("We chose {choice} for {p}. Reasons are in the earlier thread."),
        2 => format!("{p} will adopt {choice}; the reasoning follows what was raised before."),
        _ => format!("Settled: {choice} for {p}, given what we saw and what we are bound by."),
    };
    let ids: Vec<String> = ["t", "c", "d", "n", "r", "x"]
        .iter()
        .map(|s| format!("{id}-{s}"))
        .collect();
    let mut at = ctx.base();
    let step = |ctx: &mut Ctx, at: &mut DateTime<Utc>| {
        *at += ctx.gap_days(1, 6);
        *at
    };
    let authors: Vec<String> = (0..5).map(|_| ctx.person(&[])).collect();
    let mut events = vec![ids[0].clone(), ids[1].clone(), ids[2].clone()];
    ctx.event(Ev {
        id: ids[0].clone(),
        content: trigger,
        at,
        author: authors[0].clone(),
        scope: org(),
        source: "tracker".into(),
        supersedes: None,
        project: project.clone(),
    });
    let t = step(ctx, &mut at);
    ctx.event(Ev {
        id: ids[1].clone(),
        content: bound,
        at: t,
        author: authors[1].clone(),
        scope: org(),
        source: "docs".into(),
        supersedes: None,
        project: project.clone(),
    });
    let t = step(ctx, &mut at);
    ctx.event(Ev {
        id: ids[2].clone(),
        content: decision,
        at: t,
        author: authors[2].clone(),
        scope: org(),
        source: "meeting-notes".into(),
        supersedes: None,
        project: project.clone(),
    });
    let t = step(ctx, &mut at);
    // The rival decision comes from another project with the same problem.
    let rival_text = format!("{} settled on {rival} after {problem}.", decoy_project.0);
    ctx.event(Ev {
        id: ids[5].clone(),
        content: rival_text,
        at: t,
        author: authors[3].clone(),
        scope: org(),
        source: "meeting-notes".into(),
        supersedes: None,
        project: decoy_project,
    });
    events.push(ids[5].clone());
    let mut edges = vec![
        Edge {
            from: ids[2].clone(),
            to: ids[0].clone(),
            relation: "triggered_by".into(),
        },
        Edge {
            from: ids[2].clone(),
            to: ids[1].clone(),
            relation: "constrained_by".into(),
        },
    ];
    let (question, answer, gold);
    if reversal {
        let info = pick(&mut ctx.rng, &NEW_INFO);
        let t = step(ctx, &mut at);
        ctx.event(Ev {
            id: ids[3].clone(),
            content: format!("New information on {p}: {info}."),
            at: t,
            author: authors[4].clone(),
            scope: org(),
            source: "slack".into(),
            supersedes: None,
            project: project.clone(),
        });
        let t = step(ctx, &mut at);
        let text = match n % 3 {
            0 => format!("Reversing the {p} choice: moving off {choice} to {rival} after that."),
            1 => format!("{p} drops {choice} and takes {rival}."),
            _ => format!("Change of course on {p}: {rival} replaces {choice}."),
        };
        ctx.event(Ev {
            id: ids[4].clone(),
            content: text,
            at: t,
            author: authors[2].clone(),
            scope: org(),
            source: "meeting-notes".into(),
            supersedes: Some(ids[2].clone()),
            project: project.clone(),
        });
        edges.push(Edge {
            from: ids[4].clone(),
            to: ids[2].clone(),
            relation: "reverses".into(),
        });
        edges.push(Edge {
            from: ids[4].clone(),
            to: ids[3].clone(),
            relation: "because".into(),
        });
        events.push(ids[3].clone());
        events.push(ids[4].clone());
        gold = vec![
            ids[4].clone(),
            ids[3].clone(),
            ids[2].clone(),
            ids[0].clone(),
            ids[1].clone(),
        ];
        question = match n % 3 {
            0 => format!("What is the full story behind {p} moving off {choice}?"),
            1 => format!("Why did {p} drop {choice}, and what led to it in the first place?"),
            _ => format!("Walk me through how {p} ended up away from {choice}."),
        };
        answer = format!("Moved from {choice} to {rival} after {info}; {choice} was chosen because of {problem} and {constraint}");
    } else {
        gold = vec![ids[2].clone(), ids[0].clone(), ids[1].clone()];
        question = match n % 3 {
            0 => format!("Why did {p} go with {choice}?"),
            1 => format!("What led {p} to choose {choice}?"),
            _ => format!("What was behind the {choice} decision on {p}?"),
        };
        answer = format!("{choice} because of {problem} and {constraint}");
    }
    let asker = ctx.person(&authors.iter().map(String::as_str).collect::<Vec<_>>());
    let asked = at + ctx.gap_days(3, 12);
    ctx.question(
        &format!("{id}-q"),
        &question,
        asked,
        &asker,
        &gold,
        &answer,
        None,
    );
    finish(
        ctx,
        Scenario::Lineage,
        id,
        &question,
        &answer,
        gold,
        vec![ids[5].clone()],
        edges,
        events,
    );
}

fn leaver(ctx: &mut Ctx, id: &str, n: usize) {
    let project = ctx.project();
    let attr = ctx.rng.gen_range(0..ATTRS.len());
    let values = ctx.values(attr, 2);
    let leaver = ctx.person(&[]);
    let successor = ctx.person(&[&leaver]);
    let asker = ctx.person(&[&leaver, &successor]);
    let t0 = ctx.base();
    let (p, a) = (&project.0, ATTRS[attr].phrase);
    let ids: Vec<String> = (0..3).map(|k| format!("{id}-e{k}")).collect();
    let private = match n % 3 {
        0 => format!(
            "Note to self: my own guess for the {a} on {p} is {}.",
            values[1]
        ),
        1 => format!(
            "Private thought, not shared: I lean toward {} for {p}'s {a}.",
            values[1]
        ),
        _ => format!(
            "Reminder for me only: {p} {a} might end up as {}.",
            values[1]
        ),
    };
    ctx.event(Ev {
        id: ids[0].clone(),
        content: private,
        at: t0,
        author: leaver.clone(),
        scope: Scope {
            kind: ScopeKind::User,
            id: leaver.clone(),
        },
        source: "notes".into(),
        supersedes: None,
        project: project.clone(),
    });
    let t1 = t0 + ctx.gap_days(1, 6);
    let leaver_source = pick(&mut ctx.rng, &SOURCES).to_string();
    ctx.event(Ev {
        id: ids[1].clone(),
        content: statement(n, p, a, &values[0]),
        at: t1,
        author: leaver.clone(),
        scope: org(),
        source: leaver_source,
        supersedes: None,
        project: project.clone(),
    });
    let t2 = t1 + ctx.gap_days(7, 25);
    let cap = |s: &str| {
        let mut c = s.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
    };
    let (lname, sname) = (cap(&leaver), cap(&successor));
    ctx.event(Ev {
        id: ids[2].clone(),
        content: format!("{lname} has left the team; {p} moves to {sname}."),
        at: t2,
        author: successor.clone(),
        scope: org(),
        source: "email".into(),
        supersedes: None,
        project: project.clone(),
    });
    let question = match n % 3 {
        0 => format!("What did {lname} settle on for the {a} of {p} before leaving?"),
        1 => format!("{lname} has gone; what did they decide about {p}'s {a}?"),
        _ => format!("Before {lname} left, which {a} had they set for {p}?"),
    };
    let asked = t2 + ctx.gap_days(2, 10);
    ctx.question(
        &format!("{id}-q"),
        &question,
        asked,
        &asker,
        &[ids[1].clone()],
        &values[0],
        None,
    );
    finish(
        ctx,
        Scenario::Leaver,
        id,
        &question,
        &values[0],
        vec![ids[1].clone()],
        vec![],
        vec![],
        ids,
    );
}

pub fn to_jsonl(batch: &TeamBatch, config: &TeamConfig) -> String {
    let mut out = format!(
        "# Generated by cognoscenti team_track; no model in the loop.\n\
         # seed={} supersede={} control_keep={} control_asof={} provenance={} lineage={} leaver={}\n",
        config.seed, config.supersede, config.control_keep, config.control_asof,
        config.provenance, config.lineage, config.leaver
    );
    for item in &batch.stream.items {
        out.push_str(&serde_json::to_string(item).expect("stream items serialise"));
        out.push('\n');
    }
    out
}

/// The item sidecar: one JSON object per item, the gold the scorer reads.
pub fn items_to_jsonl(items: &[TeamItem]) -> String {
    items
        .iter()
        .map(|i| serde_json::to_string(i).expect("items serialise") + "\n")
        .collect()
}

pub fn items_from_jsonl(input: &str) -> anyhow::Result<Vec<TeamItem>> {
    input
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| Ok(serde_json::from_str(l)?))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::lint;
    use crate::scoring::is_authorized;
    use crate::stream::ScopeKind;
    use std::collections::{HashMap, HashSet};

    fn batch(seed: u64) -> TeamBatch {
        generate_team(&TeamConfig::first_batch(seed))
    }

    #[test]
    fn same_seed_same_bytes_other_seed_differs() {
        let c = TeamConfig::first_batch(0);
        assert_eq!(to_jsonl(&batch(0), &c), to_jsonl(&batch(0), &c));
        assert_ne!(
            to_jsonl(&batch(0), &c),
            to_jsonl(&batch(1), &TeamConfig::first_batch(1))
        );
    }

    #[test]
    fn first_batch_has_at_least_60_items_in_the_configured_sizes() {
        let b = batch(0);
        assert!(b.items.len() >= 60);
        for s in Scenario::ALL {
            let n = b.items.iter().filter(|i| i.scenario == s).count();
            assert_eq!(n, TeamConfig::first_batch(0).count(s), "{s:?}");
        }
        assert_eq!(b.stream.questions().count(), b.items.len());
    }

    #[test]
    fn full_size_is_500_items_and_passes_lint() {
        let b = generate_team(&TeamConfig::full(0));
        assert_eq!(b.items.len(), 500);
        assert_eq!(lint(&b.stream), Vec::new());
    }

    #[test]
    fn the_stream_passes_the_existing_lint() {
        for seed in [0, 1, 17] {
            assert_eq!(lint(&batch(seed).stream), Vec::new(), "seed {seed}");
        }
    }

    #[test]
    fn gold_evidence_exists_and_matches_the_stream_question() {
        let b = batch(0);
        let ids: HashSet<&str> = b.stream.events().map(|e| e.id.as_str()).collect();
        let qs: HashMap<&str, _> = b.stream.questions().map(|q| (q.id.as_str(), q)).collect();
        for item in &b.items {
            assert!(!item.gold_evidence.is_empty(), "{}", item.id);
            let q = qs[item.question_id.as_str()];
            assert_eq!(q.gold_event_ids, item.gold_evidence, "{}", item.id);
            assert_eq!(q.query, item.question);
            assert_eq!(q.gold_answer.as_deref(), Some(item.gold_answer.as_str()));
            for e in item
                .gold_evidence
                .iter()
                .chain(&item.hard_negatives)
                .chain(&item.event_ids)
            {
                assert!(ids.contains(e.as_str()), "{}: {e}", item.id);
            }
            for edge in &item.gold_edges {
                assert!(item.event_ids.contains(&edge.from) && item.event_ids.contains(&edge.to));
            }
        }
    }

    #[test]
    fn supersession_gold_is_the_last_link_and_negatives_are_the_replaced_ones() {
        let b = batch(0);
        let events: HashMap<&str, _> = b.stream.events().map(|e| (e.id.as_str(), e)).collect();
        let replaced: HashSet<&str> = b
            .stream
            .events()
            .filter_map(|e| e.supersedes.as_deref())
            .collect();
        for item in b.items.iter().filter(|i| i.scenario == Scenario::Supersede) {
            let gold = events[item.gold_evidence[0].as_str()];
            assert!(gold.supersedes.is_some() && !replaced.contains(gold.id.as_str()));
            assert!(!item.hard_negatives.is_empty());
            for n in &item.hard_negatives {
                assert!(replaced.contains(n.as_str()), "{}: {n}", item.id);
            }
        }
    }

    #[test]
    fn controls_keep_gold_that_nothing_replaces_and_negatives_that_are_not_older() {
        let b = batch(0);
        let events: HashMap<&str, _> = b.stream.events().map(|e| (e.id.as_str(), e)).collect();
        let replaced: HashSet<&str> = b
            .stream
            .events()
            .filter_map(|e| e.supersedes.as_deref())
            .collect();
        for item in b
            .items
            .iter()
            .filter(|i| i.scenario == Scenario::ControlKeep)
        {
            let gold = events[item.gold_evidence[0].as_str()];
            assert!(!replaced.contains(gold.id.as_str()), "{}", item.id);
            assert!(item.hard_negatives.len() >= 2);
            for n in &item.hard_negatives {
                assert!(events[n.as_str()].timestamp > gold.timestamp, "{}", item.id);
            }
        }
        for item in b
            .items
            .iter()
            .filter(|i| i.scenario == Scenario::ControlAsOf)
        {
            let gold = events[item.gold_evidence[0].as_str()];
            assert!(
                replaced.contains(gold.id.as_str()),
                "{}: as-of gold is the older value",
                item.id
            );
            let q = b
                .stream
                .questions()
                .find(|q| q.id == item.question_id)
                .unwrap();
            let w = q
                .validity
                .as_ref()
                .expect("as-of items carry a validity window");
            assert_eq!(w.from, gold.timestamp);
            assert!(w.until.is_some());
        }
    }

    #[test]
    fn provenance_gold_is_the_earliest_statement_and_decoys_restate_it() {
        let b = batch(0);
        let events: HashMap<&str, _> = b.stream.events().map(|e| (e.id.as_str(), e)).collect();
        for item in b
            .items
            .iter()
            .filter(|i| i.scenario == Scenario::Provenance)
        {
            let gold = events[item.gold_evidence[0].as_str()];
            assert!(!item.hard_negatives.is_empty());
            for n in &item.hard_negatives {
                let echo = events[n.as_str()];
                assert!(echo.timestamp > gold.timestamp);
                assert_ne!(echo.source, gold.source, "{}", item.id);
                assert_ne!(echo.actor.id, gold.actor.id, "{}", item.id);
            }
            assert!(
                item.gold_answer.contains(&gold.actor.id)
                    && item.gold_answer.contains(&gold.source)
            );
        }
    }

    #[test]
    fn lineage_items_carry_edge_level_gold_and_a_decoy() {
        let b = batch(0);
        for item in b.items.iter().filter(|i| i.scenario == Scenario::Lineage) {
            assert!(item.gold_edges.len() >= 2, "{}", item.id);
            for e in &item.gold_edges {
                assert!(item.gold_evidence.contains(&e.from) && item.gold_evidence.contains(&e.to));
            }
            assert_eq!(item.hard_negatives.len(), 1, "{}", item.id);
        }
        assert!(b.items.iter().any(|i| i.scenario == Scenario::Lineage
            && i.gold_edges.iter().any(|e| e.relation == "reverses")));
    }

    #[test]
    fn leaver_items_hold_a_private_note_the_asker_may_not_see() {
        let b = batch(0);
        let events: HashMap<&str, _> = b.stream.events().map(|e| (e.id.as_str(), e)).collect();
        for item in b.items.iter().filter(|i| i.scenario == Scenario::Leaver) {
            let q = b
                .stream
                .questions()
                .find(|q| q.id == item.question_id)
                .unwrap();
            let gold = events[item.gold_evidence[0].as_str()];
            assert_eq!(gold.scope.kind, ScopeKind::Org);
            let private: Vec<_> = item
                .event_ids
                .iter()
                .map(|e| events[e.as_str()])
                .filter(|e| e.scope.kind == ScopeKind::User)
                .collect();
            assert_eq!(private.len(), 1, "{}", item.id);
            assert!(!is_authorized(&q.asked_by, &private[0].scope));
            assert_ne!(q.asked_by.id, gold.actor.id);
        }
    }

    #[test]
    fn items_are_not_look_alike() {
        let b = generate_team(&TeamConfig::full(0));
        let questions: HashSet<&str> = b.items.iter().map(|i| i.question.as_str()).collect();
        assert_eq!(
            questions.len(),
            b.items.len(),
            "every question text is distinct"
        );
        // Project names are unique per item, so no two items share a subject.
        let first_entities: HashSet<String> = b
            .items
            .iter()
            .map(|i| {
                let e = b.stream.events().find(|e| e.id == i.event_ids[0]).unwrap();
                e.entities[0].external_id.clone()
            })
            .collect();
        assert_eq!(first_entities.len(), b.items.len());
        // Several wordings per scenario, not one template with slots swapped.
        for s in [Scenario::Supersede, Scenario::Provenance, Scenario::Lineage] {
            let shapes: HashSet<String> = b
                .items
                .iter()
                .filter(|i| i.scenario == s)
                .map(|i| {
                    let e = b
                        .stream
                        .events()
                        .find(|e| e.id == i.gold_evidence[0])
                        .unwrap();
                    e.content
                        .split_whitespace()
                        .filter(|w| w.chars().all(|c| c.is_lowercase()))
                        .take(3)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect();
            assert!(shapes.len() >= 4, "{s:?}: {shapes:?}");
        }
    }

    #[test]
    fn ids_follow_the_team_pattern_and_scenario_round_trips() {
        for item in batch(0).items {
            assert_eq!(
                Scenario::from_question_id(&item.question_id),
                Some(item.scenario)
            );
            assert!(item
                .question_id
                .starts_with(&format!("team-{}-", item.scenario.code())));
        }
    }
}

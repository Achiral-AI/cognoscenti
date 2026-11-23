//! Track C, Team Memory (Plan 40 Stage 4): scenarios for the four buyer
//! questions - who can see what, which fact is current (supersession), where
//! a fact came from (provenance), and why a decision was made (decision
//! lineage, scored edge by edge). Deterministic and seeded, no model in the
//! loop, offline: this module never talks to a deployment.
//!
//! One item is one question with its own events about its own project, so
//! items can be scored, reviewed, changed and dropped independently. Question
//! ids are `team-<code>-NNN-q`; see `Scenario::code`.

pub mod generate;
pub mod review;
pub mod score;

use crate::stream::Stream;
use serde::{Deserialize, Serialize};

pub use generate::{generate_team, items_from_jsonl, items_to_jsonl, to_jsonl, TeamConfig};
pub use review::{apply_review, export_review_csv, import_review, ReviewSummary};
pub use score::{score_team, team_lines, team_stats, Tally, TeamScores};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    /// A value changed one to three times; gold is the last one.
    Supersede,
    /// Hard-negative control: later events exist, but none replaces the fact.
    ControlKeep,
    /// Hard-negative control: a point-in-time question whose gold is the
    /// older value, so "newest wins" is wrong.
    ControlAsOf,
    /// Where a fact was first recorded, with restatements as decoys.
    Provenance,
    /// Why a decision was made, with gold edges between events.
    Lineage,
    /// A member left: their org-visible decision stays, their private note
    /// must not surface for anyone else.
    Leaver,
}

impl Scenario {
    pub const ALL: [Scenario; 6] = [
        Scenario::Supersede,
        Scenario::ControlKeep,
        Scenario::ControlAsOf,
        Scenario::Provenance,
        Scenario::Lineage,
        Scenario::Leaver,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Scenario::Supersede => "sup",
            Scenario::ControlKeep => "keep",
            Scenario::ControlAsOf => "asof",
            Scenario::Provenance => "prov",
            Scenario::Lineage => "why",
            Scenario::Leaver => "leave",
        }
    }

    pub fn from_question_id(question_id: &str) -> Option<Scenario> {
        let rest = question_id.strip_prefix("team-")?;
        Scenario::ALL
            .into_iter()
            .find(|s| rest.starts_with(&format!("{}-", s.code())))
    }
}

/// A gold edge in a decision chain: `from` names `to` as its reason.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub relation: String,
}

/// One reviewable item: the question, its gold, and the events that belong
/// to it (so a dropped item takes its events with it).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamItem {
    pub id: String,
    pub scenario: Scenario,
    pub question_id: String,
    pub question: String,
    pub gold_answer: String,
    pub gold_evidence: Vec<String>,
    /// Events that look right and are not: superseded values, restatements,
    /// later events about something else. Scored as outranking gold.
    pub hard_negatives: Vec<String>,
    pub gold_edges: Vec<Edge>,
    pub event_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TeamBatch {
    pub stream: Stream,
    pub items: Vec<TeamItem>,
}

/// The reviewed items that sit beside a team stream (`items.jsonl` in the
/// same directory), or `None` when there is no such file or the stream holds
/// no team question. Any other stream is left alone.
pub fn items_beside_stream(
    stream_path: &std::path::Path,
    stream: &Stream,
) -> anyhow::Result<Option<Vec<TeamItem>>> {
    if !stream
        .questions()
        .any(|q| Scenario::from_question_id(&q.id).is_some())
    {
        return Ok(None);
    }
    let Some(dir) = stream_path.parent() else {
        return Ok(None);
    };
    let path = dir.join("items.jsonl");
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(items_from_jsonl(&std::fs::read_to_string(&path)?)?))
}

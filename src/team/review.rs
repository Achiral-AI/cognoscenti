//! The reviewer workflow. `export_review_csv` writes one row per item for a
//! person to open in a spreadsheet; `import_review` reads it back and counts
//! how many items review kept, changed and dropped; `apply_review` produces
//! the reviewed batch. Stage 4 requires that count to be reported, so a
//! reviewer cannot change an item without it showing up.
//!
//! Reviewers edit only `question`, `gold_answer` and `gold_evidence`, and
//! fill `reviewer_decision` (`keep`, `change` or `drop`) and `reviewer_notes`.
//! The other columns are context. A row that says `keep` but differs from the
//! original, or says `change` but does not, is an error, not a guess.

use super::{TeamBatch, TeamItem};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const COLUMNS: [&str; 9] = [
    "id",
    "scenario",
    "question",
    "gold_answer",
    "gold_evidence",
    "evidence_text",
    "hard_negatives",
    "reviewer_decision",
    "reviewer_notes",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Keep,
    Change,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewedRow {
    pub id: String,
    pub decision: Option<Decision>,
    pub question: String,
    pub gold_answer: String,
    pub gold_evidence: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub total: usize,
    pub unreviewed: usize,
    pub kept: usize,
    pub changed: usize,
    pub dropped: usize,
    pub question_edits: usize,
    pub answer_edits: usize,
    pub evidence_edits: usize,
}

impl ReviewSummary {
    pub fn reviewed(&self) -> usize {
        self.total - self.unreviewed
    }
    pub fn changed_rate(&self) -> Option<f64> {
        (self.reviewed() > 0).then(|| self.changed as f64 / self.reviewed() as f64)
    }
    pub fn dropped_rate(&self) -> Option<f64> {
        (self.reviewed() > 0).then(|| self.dropped as f64 / self.reviewed() as f64)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewImport {
    pub summary: ReviewSummary,
    pub rows: Vec<ReviewedRow>,
}

fn scenario_code(item: &TeamItem) -> &'static str {
    item.scenario.code()
}

pub fn export_review_csv(batch: &TeamBatch) -> String {
    let content: HashMap<&str, &str> = batch
        .stream
        .events()
        .map(|e| (e.id.as_str(), e.content.as_str()))
        .collect();
    let describe = |ids: &[String]| {
        ids.iter()
            .map(|id| format!("{id}: {}", content.get(id.as_str()).copied().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(COLUMNS).expect("write to memory");
    for item in &batch.items {
        w.write_record([
            item.id.as_str(),
            scenario_code(item),
            &item.question,
            &item.gold_answer,
            &item.gold_evidence.join(";"),
            &describe(&item.gold_evidence),
            &item.hard_negatives.join(";"),
            "",
            "",
        ])
        .expect("write to memory");
    }
    String::from_utf8(w.into_inner().expect("flush to memory")).expect("csv is utf-8")
}

fn split_ids(cell: &str) -> Vec<String> {
    cell.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub fn import_review(items: &[TeamItem], csv_text: &str) -> Result<ReviewImport> {
    let by_id: HashMap<&str, &TeamItem> = items.iter().map(|i| (i.id.as_str(), i)).collect();
    let mut reader = csv::Reader::from_reader(csv_text.as_bytes());
    let headers = reader.headers()?.clone();
    for column in COLUMNS {
        if !headers.iter().any(|h| h == column) {
            bail!("review file is missing the column {column}");
        }
    }
    let col = |name: &str| headers.iter().position(|h| h == name).unwrap();
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let mut summary = ReviewSummary {
        total: items.len(),
        ..Default::default()
    };
    for record in reader.records() {
        let record = record?;
        let cell = |name: &str| record.get(col(name)).unwrap_or("").to_string();
        let id = cell("id");
        let item = by_id
            .get(id.as_str())
            .with_context(|| format!("review row {id}: no such item"))?;
        if !seen.insert(id.clone()) {
            bail!("review row {id}: appears twice");
        }
        let row = ReviewedRow {
            id: id.clone(),
            decision: match cell("reviewer_decision").trim().to_lowercase().as_str() {
                "" => None,
                "keep" => Some(Decision::Keep),
                "change" => Some(Decision::Change),
                "drop" => Some(Decision::Drop),
                other => {
                    bail!("review row {id}: decision must be keep, change or drop, got {other:?}")
                }
            },
            question: cell("question"),
            gold_answer: cell("gold_answer"),
            gold_evidence: split_ids(&cell("gold_evidence")),
            notes: cell("reviewer_notes"),
        };
        let q = row.question != item.question;
        let a = row.gold_answer != item.gold_answer;
        let e = row.gold_evidence != item.gold_evidence;
        match row.decision {
            None => summary.unreviewed += 1,
            Some(Decision::Drop) => summary.dropped += 1,
            Some(Decision::Keep) => {
                if q || a || e {
                    bail!(
                        "review row {id}: marked keep but question, answer or evidence was edited"
                    );
                }
                summary.kept += 1;
            }
            Some(Decision::Change) => {
                if !(q || a || e) {
                    bail!("review row {id}: marked change but nothing was edited");
                }
                if row.gold_evidence.is_empty() {
                    bail!("review row {id}: gold_evidence cannot be empty");
                }
                if let Some(bad) = row
                    .gold_evidence
                    .iter()
                    .find(|g| !item.event_ids.contains(g))
                {
                    bail!("review row {id}: evidence {bad} is not an event of this item");
                }
                summary.changed += 1;
                summary.question_edits += usize::from(q);
                summary.answer_edits += usize::from(a);
                summary.evidence_edits += usize::from(e);
            }
        }
        rows.push(row);
    }
    summary.unreviewed += items.len() - rows.len();
    Ok(ReviewImport { summary, rows })
}

/// The batch after review: dropped items and their events are gone, changed
/// items carry the reviewer's text. Items without a decision are untouched.
pub fn apply_review(batch: &TeamBatch, review: &ReviewImport) -> TeamBatch {
    let rows: HashMap<&str, &ReviewedRow> =
        review.rows.iter().map(|r| (r.id.as_str(), r)).collect();
    let mut dropped_events: HashSet<&str> = HashSet::new();
    let mut dropped_questions: HashSet<&str> = HashSet::new();
    let mut items = Vec::new();
    for item in &batch.items {
        let row = rows.get(item.id.as_str());
        match row.and_then(|r| r.decision) {
            Some(Decision::Drop) => {
                dropped_events.extend(item.event_ids.iter().map(String::as_str));
                dropped_questions.insert(item.question_id.as_str());
            }
            Some(Decision::Change) => {
                let r = row.unwrap();
                let mut changed = item.clone();
                changed.question = r.question.clone();
                changed.gold_answer = r.gold_answer.clone();
                changed.gold_evidence = r.gold_evidence.clone();
                items.push(changed);
            }
            _ => items.push(item.clone()),
        }
    }
    let edited: HashMap<&str, &TeamItem> =
        items.iter().map(|i| (i.question_id.as_str(), i)).collect();
    let stream_items = batch
        .stream
        .items
        .iter()
        .filter(|s| match s {
            crate::stream::StreamItem::Event(e) => !dropped_events.contains(e.id.as_str()),
            crate::stream::StreamItem::Question(q) => !dropped_questions.contains(q.id.as_str()),
        })
        .cloned()
        .map(|s| match s {
            crate::stream::StreamItem::Question(mut q) => {
                if let Some(item) = edited.get(q.id.as_str()) {
                    q.query = item.question.clone();
                    q.gold_answer = Some(item.gold_answer.clone());
                    q.gold_event_ids = item.gold_evidence.clone();
                }
                crate::stream::StreamItem::Question(q)
            }
            other => other,
        })
        .collect();
    TeamBatch {
        stream: crate::stream::Stream {
            items: stream_items,
        },
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::lint;
    use crate::team::generate::{generate_team, TeamConfig};

    fn batch() -> TeamBatch {
        generate_team(&TeamConfig::first_batch(0))
    }

    /// Rewrites the export, letting `edit` change each row's cells by name.
    fn reviewed(batch: &TeamBatch, edit: impl Fn(usize, &mut HashMap<String, String>)) -> String {
        let text = export_review_csv(batch);
        let mut r = csv::Reader::from_reader(text.as_bytes());
        let headers: Vec<String> = r.headers().unwrap().iter().map(String::from).collect();
        let mut w = csv::Writer::from_writer(Vec::new());
        w.write_record(&headers).unwrap();
        for (i, rec) in r.records().enumerate() {
            let rec = rec.unwrap();
            let mut cells: HashMap<String, String> = headers
                .iter()
                .cloned()
                .zip(rec.iter().map(String::from))
                .collect();
            edit(i, &mut cells);
            w.write_record(headers.iter().map(|h| cells[h].clone()))
                .unwrap();
        }
        String::from_utf8(w.into_inner().unwrap()).unwrap()
    }

    fn set(c: &mut HashMap<String, String>, k: &str, v: &str) {
        c.insert(k.to_string(), v.to_string());
    }

    #[test]
    fn export_has_the_agreed_columns_one_row_per_item_and_the_evidence_text() {
        let b = batch();
        let text = export_review_csv(&b);
        let mut r = csv::Reader::from_reader(text.as_bytes());
        assert_eq!(
            r.headers().unwrap().iter().collect::<Vec<_>>(),
            COLUMNS.to_vec()
        );
        let rows: Vec<_> = r.records().map(|x| x.unwrap()).collect();
        assert_eq!(rows.len(), b.items.len());
        let first = &rows[0];
        let content = &b
            .stream
            .events()
            .find(|e| e.id == b.items[0].gold_evidence[0])
            .unwrap()
            .content;
        assert!(first.get(5).unwrap().contains(content.as_str()));
        assert_eq!(first.get(7), Some(""));
    }

    #[test]
    fn an_untouched_export_is_all_unreviewed() {
        let b = batch();
        let imp = import_review(&b.items, &export_review_csv(&b)).unwrap();
        assert_eq!(imp.summary.unreviewed, b.items.len());
        assert_eq!(imp.summary.changed_rate(), None);
    }

    #[test]
    fn counts_kept_changed_and_dropped() {
        let b = batch();
        let text = reviewed(&b, |i, c| match i {
            0 => set(c, "reviewer_decision", "change"),
            1 | 2 => set(c, "reviewer_decision", "drop"),
            _ => set(c, "reviewer_decision", "keep"),
        });
        // Row 0 says change without an edit: rejected until it carries one.
        assert!(import_review(&b.items, &text)
            .unwrap_err()
            .to_string()
            .contains("nothing was edited"));
        let text = reviewed(&b, |i, c| match i {
            0 => {
                set(c, "reviewer_decision", "change");
                set(c, "question", "A reworded question?");
                set(c, "reviewer_notes", "original was ambiguous");
            }
            1 | 2 => set(c, "reviewer_decision", "drop"),
            _ => set(c, "reviewer_decision", "keep"),
        });
        let s = import_review(&b.items, &text).unwrap().summary;
        assert_eq!((s.changed, s.dropped, s.unreviewed), (1, 2, 0));
        assert_eq!(s.kept, b.items.len() - 3);
        assert_eq!(s.question_edits, 1);
        assert!((s.dropped_rate().unwrap() - 2.0 / b.items.len() as f64).abs() < 1e-12);
    }

    #[test]
    fn apply_removes_dropped_items_with_their_events_and_keeps_the_stream_lint_clean() {
        let b = batch();
        let dropped = b.items[0].clone();
        let changed = b.items[1].clone();
        let text = reviewed(&b, |i, c| match i {
            0 => set(c, "reviewer_decision", "drop"),
            1 => {
                set(c, "reviewer_decision", "change");
                set(c, "gold_answer", "Edited answer");
            }
            _ => {}
        });
        let imp = import_review(&b.items, &text).unwrap();
        let out = apply_review(&b, &imp);
        assert_eq!(out.items.len(), b.items.len() - 1);
        assert!(out.stream.questions().all(|q| q.id != dropped.question_id));
        assert!(out
            .stream
            .events()
            .all(|e| !dropped.event_ids.contains(&e.id)));
        let q = out
            .stream
            .questions()
            .find(|q| q.id == changed.question_id)
            .unwrap();
        assert_eq!(q.gold_answer.as_deref(), Some("Edited answer"));
        assert_eq!(lint(&out.stream), Vec::new());
    }

    #[test]
    fn a_changed_evidence_list_reaches_the_stream_question() {
        let b = batch();
        let item = b
            .items
            .iter()
            .position(|i| i.gold_evidence.len() > 1 && i.event_ids.len() > 2)
            .unwrap();
        let keep_first = b.items[item].gold_evidence[0].clone();
        let text = reviewed(&b, |i, c| {
            if i == item {
                set(c, "reviewer_decision", "change");
                set(c, "gold_evidence", &keep_first);
            }
        });
        let out = apply_review(&b, &import_review(&b.items, &text).unwrap());
        let q = out
            .stream
            .questions()
            .find(|q| q.id == b.items[item].question_id)
            .unwrap();
        assert_eq!(q.gold_event_ids, vec![keep_first]);
    }

    #[test]
    fn inconsistent_rows_are_errors() {
        let b = batch();
        let err = |edit: &dyn Fn(&mut HashMap<String, String>)| {
            import_review(
                &b.items,
                &reviewed(&b, |i, c| {
                    if i == 0 {
                        edit(c)
                    }
                }),
            )
            .unwrap_err()
            .to_string()
        };
        assert!(err(&|c| {
            set(c, "reviewer_decision", "keep");
            set(c, "question", "Edited?");
        })
        .contains("marked keep"));
        assert!(err(&|c| set(c, "reviewer_decision", "maybe")).contains("keep, change or drop"));
        assert!(err(&|c| {
            set(c, "reviewer_decision", "change");
            set(c, "gold_evidence", "nope");
        })
        .contains("not an event of this item"));
        assert!(err(&|c| set(c, "id", "team-sup-999")).contains("no such item"));
    }

    #[test]
    fn a_duplicate_row_is_an_error() {
        let b = batch();
        let text = export_review_csv(&b);
        let mut r = csv::Reader::from_reader(text.as_bytes());
        let mut w = csv::Writer::from_writer(Vec::new());
        w.write_record(r.headers().unwrap()).unwrap();
        let rows: Vec<_> = r.records().map(|x| x.unwrap()).collect();
        for row in rows.iter().chain(rows.first()) {
            w.write_record(row).unwrap();
        }
        let dup = String::from_utf8(w.into_inner().unwrap()).unwrap();
        assert!(import_review(&b.items, &dup)
            .unwrap_err()
            .to_string()
            .contains("appears twice"));
    }
}

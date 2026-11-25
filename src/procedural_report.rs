//! The procedural track's results and report (Plan 44 Phase 5). A run file is
//! one arm on one stream: for every replicate, every question's category,
//! outcome and tokens. `analyse` applies the pre-registered rules in
//! `docs/preregistration-procedural-2026-11.md` and `render_html` draws them.
//! Nothing here reads a key or an address; `report::ensure_clean` still checks
//! the output before it is written.

use crate::adapter::QuestionResult;
use crate::procedural::{category_of, score_procedural, ProceduralSpec};
use crate::report::{ensure_clean, esc};
use crate::stability::flipped_questions;
use crate::stats::{bootstrap_ci_95, mcnemar_exact, Interval};
use crate::stream::Stream;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const SCHEMA: &str = "cognoscenti-procedural/1";
/// The bootstrap seed the pre-registration fixes.
pub const BOOTSTRAP_SEED: u64 = 0;
/// Registered margin for near-miss firing (`procedural` minus `declarative`).
pub const NEAR_MISS_MARGIN: f64 = 0.10;
/// A rate over fewer questions than this is not given an interval.
pub const MIN_N: usize = 10;
pub const CATEGORIES: [&str; 4] = ["follow", "near", "sup", "leak"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionRow {
    pub id: String,
    pub category: String,
    /// The category's event: for `follow` the gold procedure was retrieved
    /// (wanted); for `near`, `sup` and `leak` the harm happened (unwanted).
    pub hit: bool,
    pub tokens: usize,
    pub retrieved: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProceduralRun {
    pub schema: String,
    pub label: Option<String>,
    pub arm: String,
    pub seed: u64,
    pub stream_file: String,
    pub stream_sha256: String,
    pub top_k: usize,
    pub replicates: usize,
    pub cognoscenti_commit: Option<String>,
    /// What the server said it used to retrieve, from replicate 0.
    pub retrieval: Option<serde_json::Value>,
    /// `[replicate][question]`, questions in stream order.
    pub rows: Vec<Vec<QuestionRow>>,
}

impl ProceduralRun {
    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::write(path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let run: Self = serde_json::from_str(&text)
            .with_context(|| format!("{} is not a procedural run file", path.display()))?;
        if run.schema != SCHEMA {
            bail!(
                "{} has schema {:?}, expected {SCHEMA}",
                path.display(),
                run.schema
            );
        }
        Ok(run)
    }
}

/// One replicate's results as rows. Scoring one result at a time with
/// `score_procedural` keeps the definitions in one place.
pub fn rows_of(
    stream: &Stream,
    spec: &ProceduralSpec,
    results: &[QuestionResult],
) -> Vec<QuestionRow> {
    results
        .iter()
        .filter_map(|r| {
            let category = category_of(&r.question_id)?;
            let s = score_procedural(stream, spec, std::slice::from_ref(r));
            let tally = match category {
                "follow" => s.adherence,
                "near" => s.near_miss_firing,
                "sup" => s.superseded_rule,
                _ => s.scope_leaks,
            };
            Some(QuestionRow {
                id: r.question_id.clone(),
                category: category.to_string(),
                hit: tally.hits > 0,
                tokens: r.tokens_injected,
                retrieved: r.retrieved_ids.clone(),
            })
        })
        .collect()
}

// -------------------------------------------------------------- analysis ---

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Count {
    pub hits: usize,
    pub n: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rate {
    pub tally: Count,
    /// `None` when n is under `MIN_N`.
    pub ci: Option<[f64; 2]>,
}

impl Rate {
    fn of(values: &[bool]) -> Self {
        let hits = values.iter().filter(|v| **v).count();
        let ci = (values.len() >= MIN_N).then(|| {
            let f: Vec<f64> = values.iter().map(|&v| f64::from(u8::from(v))).collect();
            let i = bootstrap_ci_95(&f, BOOTSTRAP_SEED);
            [i.lo, i.hi]
        });
        Self {
            tally: Count {
                hits,
                n: values.len(),
            },
            ci,
        }
    }

    pub fn describe(&self) -> String {
        let t = self.tally;
        if t.n == 0 {
            return "no questions".to_string();
        }
        let rate = t.hits as f64 / t.n as f64;
        match self.ci {
            Some([lo, hi]) => format!("{}/{} = {rate:.3} (95% CI {lo:.3}-{hi:.3})", t.hits, t.n),
            None => format!("{}/{} = {rate:.3}, n too small (n={})", t.hits, t.n, t.n),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmSummary {
    pub arm: String,
    pub seeds: Vec<u64>,
    pub replicates: usize,
    /// Replicate 0 of every seed, pooled: the unit the registered tests use.
    pub rates: BTreeMap<String, Rate>,
    /// Each category's event count per replicate index, pooled over seeds.
    pub per_replicate: BTreeMap<String, Vec<Count>>,
    /// Leaks over every replicate and every seed (decision rule 4).
    pub leaks_all: Count,
    pub tokens_per_question: Option<f64>,
    /// Share of questions whose outcome differs across replicates.
    pub flip_rate: Option<Rate>,
}

fn good(row: &QuestionRow) -> bool {
    if row.category == "follow" {
        row.hit
    } else {
        !row.hit
    }
}

pub fn summarise(arm: &str, runs: &[&ProceduralRun]) -> Result<ArmSummary> {
    if runs.is_empty() {
        bail!("no runs for arm {arm}");
    }
    let replicates = runs.iter().map(|r| r.rows.len()).min().unwrap_or(0);
    if replicates == 0 {
        bail!("arm {arm} has a run with no replicates");
    }
    let rows_at = |rep: usize, cat: &str| -> Vec<&QuestionRow> {
        runs.iter()
            .flat_map(|r| r.rows[rep].iter())
            .filter(|row| row.category == cat)
            .collect()
    };
    let mut rates = BTreeMap::new();
    let mut per_replicate = BTreeMap::new();
    for cat in CATEGORIES {
        let values: Vec<bool> = rows_at(0, cat).iter().map(|r| r.hit).collect();
        rates.insert(cat.to_string(), Rate::of(&values));
        per_replicate.insert(
            cat.to_string(),
            (0..replicates)
                .map(|rep| {
                    let rows = rows_at(rep, cat);
                    Count {
                        hits: rows.iter().filter(|r| r.hit).count(),
                        n: rows.len(),
                    }
                })
                .collect(),
        );
    }
    let leaks_all = (0..replicates).fold(Count { hits: 0, n: 0 }, |acc, rep| {
        let rows = rows_at(rep, "leak");
        Count {
            hits: acc.hits + rows.iter().filter(|r| r.hit).count(),
            n: acc.n + rows.len(),
        }
    });
    let all0: Vec<&QuestionRow> = runs.iter().flat_map(|r| r.rows[0].iter()).collect();
    let tokens_per_question = (!all0.is_empty())
        .then(|| all0.iter().map(|r| r.tokens).sum::<usize>() as f64 / all0.len() as f64);
    let flip_rate = (replicates >= 2).then(|| {
        let mut outcomes: Vec<Vec<bool>> = Vec::new();
        for run in runs {
            for q in 0..run.rows[0].len() {
                outcomes.push((0..replicates).map(|rep| good(&run.rows[rep][q])).collect());
            }
        }
        Rate::of(&flipped_questions(&outcomes))
    });
    Ok(ArmSummary {
        arm: arm.to_string(),
        seeds: runs.iter().map(|r| r.seed).collect(),
        replicates,
        rates,
        per_replicate,
        leaks_all,
        tokens_per_question,
        flip_rate,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Paired {
    pub category: String,
    pub n: usize,
    pub difference: f64,
    pub ci: [f64; 2],
    /// Exact McNemar p on the pairs; reported for adherence only.
    pub mcnemar_p: Option<f64>,
    pub only_procedural: usize,
    pub only_declarative: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Condition {
    pub name: String,
    pub holds: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub conditions: Vec<Condition>,
    pub claim: bool,
    pub harms: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub schema: String,
    pub arms: Vec<ArmSummary>,
    pub paired: Vec<Paired>,
    pub verdict: Option<Verdict>,
}

fn paired(
    cat: &str,
    procedural: &[&ProceduralRun],
    declarative: &[&ProceduralRun],
) -> Result<Paired> {
    let pick = |runs: &[&ProceduralRun]| -> BTreeMap<(u64, String), bool> {
        runs.iter()
            .flat_map(|r| {
                r.rows[0]
                    .iter()
                    .filter(|row| row.category == cat)
                    .map(|row| ((r.seed, row.id.clone()), row.hit))
            })
            .collect()
    };
    let (p, d) = (pick(procedural), pick(declarative));
    if p.keys().ne(d.keys()) {
        bail!("the two arms did not answer the same questions on the same seeds");
    }
    let a: Vec<bool> = p.values().copied().collect();
    let b: Vec<bool> = d.values().copied().collect();
    let diffs: Vec<f64> = a
        .iter()
        .zip(&b)
        .map(|(&x, &y)| f64::from(u8::from(x)) - f64::from(u8::from(y)))
        .collect();
    let Interval {
        estimate, lo, hi, ..
    } = bootstrap_ci_95(&diffs, BOOTSTRAP_SEED);
    let m = mcnemar_exact(&a, &b);
    Ok(Paired {
        category: cat.to_string(),
        n: a.len(),
        difference: estimate,
        ci: [lo, hi],
        mcnemar_p: (cat == "follow").then_some(m.p_value),
        only_procedural: m.only_a,
        only_declarative: m.only_b,
    })
}

/// The decision rule, as registered.
pub fn decide(paired: &[Paired], procedural: &ArmSummary) -> Verdict {
    let of = |c: &str| {
        paired
            .iter()
            .find(|p| p.category == c)
            .expect("all categories paired")
    };
    let (follow, near, sup) = (of("follow"), of("near"), of("sup"));
    let p = follow.mcnemar_p.unwrap_or(1.0);
    let c1 = follow.difference > 0.0 && follow.ci[0] > 0.0 && p < 0.05;
    let c2 = near.ci[1] <= NEAR_MISS_MARGIN;
    let c3 = sup.ci[0] <= 0.0;
    let c4 = procedural.leaks_all.hits == 0;
    let conditions = vec![
        Condition {
            name: "1. Adherence above declarative".into(),
            holds: c1,
            detail: format!(
                "difference {:+.3}, 95% CI {:+.3} to {:+.3}, exact McNemar p {:.4}, n={} pairs",
                follow.difference, follow.ci[0], follow.ci[1], p, follow.n
            ),
        },
        Condition {
            name: format!("2. Near-miss firing within +{NEAR_MISS_MARGIN:.2}"),
            holds: c2,
            detail: format!(
                "difference {:+.3}, upper end {:+.3}, n={} pairs{}",
                near.difference,
                near.ci[1],
                near.n,
                if c2 { "" } else { "; not shown to hold" }
            ),
        },
        Condition {
            name: "3. Superseded-rule rate not entirely above zero".into(),
            holds: c3,
            detail: format!(
                "difference {:+.3}, 95% CI {:+.3} to {:+.3}, n={} pairs",
                sup.difference, sup.ci[0], sup.ci[1], sup.n
            ),
        },
        Condition {
            name: "4. Zero scope leaks".into(),
            holds: c4,
            detail: format!(
                "{} leak(s) in {} trials over every replicate and seed",
                procedural.leaks_all.hits, procedural.leaks_all.n
            ),
        },
    ];
    let mut harms = Vec::new();
    for (name, p) in [("near-miss firing", near), ("superseded-rule rate", sup)] {
        if p.ci[0] > 0.0 {
            harms.push(format!(
                "procedural is worse than declarative on {name}: {:+.3} (95% CI {:+.3} to {:+.3})",
                p.difference, p.ci[0], p.ci[1]
            ));
        }
    }
    Verdict {
        claim: conditions.iter().all(|c| c.holds),
        conditions,
        harms,
    }
}

pub fn analyse(runs: &[ProceduralRun]) -> Result<Analysis> {
    let mut by_arm: BTreeMap<&str, Vec<&ProceduralRun>> = BTreeMap::new();
    for run in runs {
        by_arm.entry(run.arm.as_str()).or_default().push(run);
    }
    for (arm, group) in &by_arm {
        let mut seeds: Vec<u64> = group.iter().map(|r| r.seed).collect();
        seeds.sort_unstable();
        if seeds.windows(2).any(|w| w[0] == w[1]) {
            bail!("arm {arm} has two runs on the same seed; each run file is one cohort");
        }
    }
    let arms = by_arm
        .iter()
        .map(|(arm, group)| summarise(arm, group))
        .collect::<Result<Vec<_>>>()?;
    let (paired_rows, verdict) = match (by_arm.get("procedural"), by_arm.get("declarative")) {
        (Some(p), Some(d)) => {
            let rows = CATEGORIES
                .iter()
                .map(|c| paired(c, p, d))
                .collect::<Result<Vec<_>>>()?;
            let summary = arms
                .iter()
                .find(|a| a.arm == "procedural")
                .expect("summarised");
            let verdict = decide(&rows, summary);
            (rows, Some(verdict))
        }
        _ => (Vec::new(), None),
    };
    Ok(Analysis {
        schema: SCHEMA.to_string(),
        arms,
        paired: paired_rows,
        verdict,
    })
}

// ------------------------------------------------------------------ html ---

const LABELS: [(&str, &str, &str); 4] = [
    ("follow", "Adherence", "higher is better"),
    ("near", "Near-miss firing", "lower is better"),
    ("sup", "Superseded-rule rate", "lower is better"),
    ("leak", "Scope leaks", "must be zero"),
];

pub fn render_html(a: &Analysis) -> String {
    let mut h = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>The procedural track</title><style>\
         :root{color-scheme:light dark;--bg:#fff;--fg:#1a1a1a;--line:#d0d0d0;--ok:#1a7f37;--bad:#b42318}\
         @media(prefers-color-scheme:dark){:root{--bg:#161616;--fg:#eee;--line:#444;--ok:#56d364;--bad:#ff7b72}}\
         body{background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,sans-serif;margin:0 auto;max-width:60rem;padding:1rem}\
         table{border-collapse:collapse;width:100%;margin:1rem 0;display:block;overflow-x:auto}\
         th,td{border:1px solid var(--line);padding:.35rem .6rem;text-align:left;vertical-align:top}\
         .ok{color:var(--ok);font-weight:600}.bad{color:var(--bad);font-weight:600}\
         </style></head><body><h1>The procedural track</h1>\
         <p>Retrieval only: no model answers. Rates are for replicate 0 of every seed, pooled; \
         intervals are 95% percentile bootstraps over questions, seed 0.</p>",
    );
    h.push_str("<h2>Per arm</h2><table><tr><th>Metric</th>");
    for arm in &a.arms {
        h.push_str(&format!("<th>{}</th>", esc(&arm.arm)));
    }
    h.push_str("</tr>");
    for (cat, name, note) in LABELS {
        h.push_str(&format!("<tr><td>{name} <small>({note})</small></td>"));
        for arm in &a.arms {
            h.push_str(&format!("<td>{}</td>", esc(&arm.rates[cat].describe())));
        }
        h.push_str("</tr>");
    }
    h.push_str("<tr><td>Scope leaks, every replicate and seed</td>");
    for arm in &a.arms {
        h.push_str(&format!(
            "<td>{}/{}</td>",
            arm.leaks_all.hits, arm.leaks_all.n
        ));
    }
    h.push_str("</tr><tr><td>Tokens per question</td>");
    for arm in &a.arms {
        h.push_str(&format!(
            "<td>{}</td>",
            arm.tokens_per_question
                .map_or("none".to_string(), |t| format!("{t:.1}"))
        ));
    }
    h.push_str("</tr><tr><td>Flip rate over replicates</td>");
    for arm in &a.arms {
        h.push_str(&format!(
            "<td>{}</td>",
            arm.flip_rate
                .as_ref()
                .map_or("one replicate".to_string(), Rate::describe)
        ));
    }
    h.push_str("</tr></table>");
    h.push_str(
        "<h2>Per replicate</h2><table><tr><th>Arm</th><th>Metric</th><th>Events per replicate</th></tr>",
    );
    for arm in &a.arms {
        for (cat, name, _) in LABELS {
            let cells: Vec<String> = arm.per_replicate[cat]
                .iter()
                .map(|t| format!("{}/{}", t.hits, t.n))
                .collect();
            h.push_str(&format!(
                "<tr><td>{}</td><td>{name}</td><td>{}</td></tr>",
                esc(&arm.arm),
                cells.join(", ")
            ));
        }
    }
    h.push_str("</table>");
    if let Some(v) = &a.verdict {
        h.push_str("<h2>Paired: procedural minus declarative</h2><table><tr><th>Metric</th><th>Difference</th><th>95% CI</th><th>Pairs</th><th>Only procedural / only declarative</th></tr>");
        for (cat, name, _) in LABELS {
            if let Some(p) = a.paired.iter().find(|p| p.category == cat) {
                h.push_str(&format!(
                    "<tr><td>{name}</td><td>{:+.3}</td><td>{:+.3} to {:+.3}</td><td>{}</td><td>{} / {}</td></tr>",
                    p.difference, p.ci[0], p.ci[1], p.n, p.only_procedural, p.only_declarative
                ));
            }
        }
        h.push_str("</table><h2>Decision rule</h2><table>");
        for c in &v.conditions {
            h.push_str(&format!(
                "<tr><td>{}</td><td class=\"{}\">{}</td><td>{}</td></tr>",
                esc(&c.name),
                if c.holds { "ok" } else { "bad" },
                if c.holds { "holds" } else { "does not hold" },
                esc(&c.detail)
            ));
        }
        h.push_str("</table>");
        h.push_str(&format!(
            "<p class=\"{}\">{}</p>",
            if v.claim { "ok" } else { "bad" },
            if v.claim {
                "All four conditions hold: procedural memory beats declarative memory alone on this stream."
            } else {
                "At least one condition does not hold. No claim is made; every number above is the result."
            }
        ));
        for harm in &v.harms {
            h.push_str(&format!("<p class=\"bad\">{}</p>", esc(harm)));
        }
    } else {
        h.push_str(
            "<p>No paired comparison: the results need both a declarative and a procedural arm.</p>",
        );
    }
    h.push_str("<h2>Limits</h2><p>Registered in the pre-registration: one organisation shape, one seed pair, retrieval only, and every wording of a situation shares its qualifier and topic words.</p></body></html>");
    h
}

/// Writes `index.html` and `results.json` to `out`.
pub fn write_report(runs: &[ProceduralRun], out: &Path) -> Result<std::path::PathBuf> {
    let analysis = analyse(runs)?;
    let html = render_html(&analysis);
    let json = serde_json::to_string_pretty(&analysis)?;
    ensure_clean(&html)?;
    ensure_clean(&json)?;
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    std::fs::write(out.join("results.json"), json)?;
    std::fs::write(out.join("index.html"), html)?;
    Ok(out.join("index.html"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, cat: &str, hit: bool) -> QuestionRow {
        QuestionRow {
            id: id.to_string(),
            category: cat.to_string(),
            hit,
            tokens: 10,
            retrieved: Vec::new(),
        }
    }

    /// 60 `follow`, 40 `near`, 4 `sup`, 4 `leak` rows with the first n of each
    /// category set to `hit`, repeated for every replicate.
    fn run(
        arm: &str,
        seed: u64,
        hits: (usize, usize, usize, usize),
        replicates: usize,
    ) -> ProceduralRun {
        let rows: Vec<QuestionRow> = (0..60)
            .map(|i| row(&format!("f{i}"), "follow", i < hits.0))
            .chain((0..40).map(|i| row(&format!("n{i}"), "near", i < hits.1)))
            .chain((0..4).map(|i| row(&format!("s{i}"), "sup", i < hits.2)))
            .chain((0..4).map(|i| row(&format!("l{i}"), "leak", i < hits.3)))
            .collect();
        ProceduralRun {
            schema: SCHEMA.to_string(),
            label: None,
            arm: arm.to_string(),
            seed,
            stream_file: "s.jsonl".into(),
            stream_sha256: "00".into(),
            top_k: 1,
            replicates,
            cognoscenti_commit: None,
            retrieval: None,
            rows: vec![rows; replicates],
        }
    }

    type Hits = (usize, usize, usize, usize);

    fn runs(proc: Hits, decl: Hits) -> Vec<ProceduralRun> {
        let mut all = Vec::new();
        for seed in [4242, 4243] {
            all.push(run("procedural", seed, proc, 5));
            all.push(run("declarative", seed, decl, 5));
        }
        all
    }

    #[test]
    fn a_clear_adherence_gain_with_no_harm_is_claimed() {
        let a = analyse(&runs((55, 2, 0, 0), (30, 2, 0, 0))).unwrap();
        let v = a.verdict.unwrap();
        assert!(v.claim, "{:?}", v.conditions);
        assert!(v.harms.is_empty());
        let follow = a.paired.iter().find(|p| p.category == "follow").unwrap();
        assert_eq!(follow.n, 120);
        assert_eq!(follow.only_procedural, 50);
        assert!(follow.mcnemar_p.unwrap() < 0.05);
    }

    #[test]
    fn one_scope_leak_in_any_replicate_fails_the_claim_whatever_else_shows() {
        let mut all = runs((55, 2, 0, 0), (30, 2, 0, 0));
        let leaky = all
            .iter_mut()
            .find(|r| r.arm == "procedural" && r.seed == 4243)
            .unwrap();
        leaky.rows[3][104].hit = true;
        assert_eq!(leaky.rows[3][104].category, "leak");
        let v = analyse(&all).unwrap().verdict.unwrap();
        assert!(!v.claim);
        assert!(!v.conditions[3].holds);
        assert!(v.conditions[0].holds);
    }

    #[test]
    fn a_near_miss_rise_beyond_the_margin_fails_and_is_named_a_harm() {
        let v = analyse(&runs((55, 30, 0, 0), (30, 2, 0, 0)))
            .unwrap()
            .verdict
            .unwrap();
        assert!(!v.claim);
        assert!(!v.conditions[1].holds);
        assert!(v.harms.iter().any(|h| h.contains("near-miss firing")));
    }

    #[test]
    fn no_adherence_gain_is_no_claim() {
        let v = analyse(&runs((30, 2, 0, 0), (30, 2, 0, 0)))
            .unwrap()
            .verdict
            .unwrap();
        assert!(!v.claim);
        assert!(!v.conditions[0].holds);
    }

    #[test]
    fn rates_under_ten_questions_get_no_interval_and_say_so() {
        let a = analyse(&runs((55, 2, 1, 0), (30, 2, 0, 0))).unwrap();
        let arm = a.arms.iter().find(|x| x.arm == "procedural").unwrap();
        assert_eq!(arm.rates["leak"].ci, None);
        assert!(arm.rates["leak"].describe().contains("n too small"));
        assert!(arm.rates["follow"].ci.is_some());
    }

    #[test]
    fn arms_that_answered_different_questions_cannot_be_paired() {
        let mut all = runs((55, 2, 0, 0), (30, 2, 0, 0));
        let r = all.iter_mut().find(|r| r.arm == "declarative").unwrap();
        r.rows[0][0].id = "other".into();
        assert!(analyse(&all).is_err());
    }

    #[test]
    fn two_runs_of_one_arm_on_one_seed_are_refused() {
        let all = vec![
            run("procedural", 1, (1, 0, 0, 0), 1),
            run("procedural", 1, (1, 0, 0, 0), 1),
        ];
        assert!(analyse(&all).is_err());
    }

    #[test]
    fn flip_rate_counts_questions_whose_outcome_changes_between_replicates() {
        let mut r = run("procedural", 1, (60, 0, 0, 0), 3);
        r.rows[2][0].hit = false;
        let a = summarise("procedural", &[&r]).unwrap();
        let flip = a.flip_rate.unwrap();
        assert_eq!(flip.tally.hits, 1);
        assert_eq!(flip.tally.n, 108);
    }

    #[test]
    fn the_report_is_written_clean_and_names_the_decision() {
        let dir = std::env::temp_dir().join(format!("cogn-proc-report-{}", std::process::id()));
        let index = write_report(&runs((55, 2, 0, 0), (30, 2, 0, 0)), &dir).unwrap();
        let html = std::fs::read_to_string(index).unwrap();
        assert!(html.contains("Adherence") && html.contains("Decision rule"));
        assert!(html.contains("procedural memory beats declarative memory alone"));
        assert!(!html.contains("<script"));
        assert!(dir.join("results.json").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_report_with_one_arm_has_no_verdict() {
        let a = analyse(&[run("bm25", 1, (10, 0, 0, 0), 1)]).unwrap();
        assert!(a.verdict.is_none());
        assert!(render_html(&a).contains("No paired comparison"));
    }
}

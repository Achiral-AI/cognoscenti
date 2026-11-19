//! `cognoscenti report`: one self-contained HTML page per run, drawn from
//! results files (`--results-json`). Inline SVG only, colour carries meaning, no script,
//! no external fonts or links. Each cohort (a results file, named by its
//! `--label`) stays on its own rows; nothing is averaged across cohorts.

use crate::results::{RunFile, Stat, SystemResults};
use crate::stats::mcnemar_exact;
use anyhow::{bail, Context, Result};
use clap::Parser;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[clap(name = "cognoscenti report")]
pub struct ReportArgs {
    /// Results file(s) written by `--results-json`; each is one cohort.
    #[clap(long = "results", required = true, num_args = 1..)]
    pub results: Vec<PathBuf>,

    /// Directory to write `index.html`, `results.json` and `results.csv`
    /// into. Defaults to `report/` beside the first results file.
    #[clap(long)]
    pub out: Option<PathBuf>,
}

pub fn run_report(args: &ReportArgs) -> Result<PathBuf> {
    let cohorts: Vec<RunFile> = args
        .results
        .iter()
        .map(|p| RunFile::load(p))
        .collect::<Result<_>>()?;
    let out = match &args.out {
        Some(out) => out.clone(),
        None => args.results[0]
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("report"),
    };
    write_report(&cohorts, &out)?;
    Ok(out.join("index.html"))
}

/// Text a report must never contain: credentials, endpoints, and the
/// scheme that would make an external fetch possible.
const FORBIDDEN: [&str; 8] = [
    "http",
    "acm_",
    "Bearer",
    "localhost",
    "127.0.0.1",
    "API_KEY",
    "BASE_URL",
    "://",
];

/// Refuses output that carries a key, an endpoint or an external reference,
/// including the values of the adapter's own environment variables.
pub fn ensure_clean(text: &str) -> Result<()> {
    for needle in FORBIDDEN {
        if text.contains(needle) {
            bail!("report output would contain {needle:?}; refusing to write it");
        }
    }
    for var in [
        "COGNOSCENTI_ACHIRAL_API_KEY",
        "COGNOSCENTI_ACHIRAL_API_KEYS",
        "COGNOSCENTI_ACHIRAL_BASE_URL",
    ] {
        if let Ok(value) = std::env::var(var) {
            for part in value.split(',').map(str::trim).filter(|p| p.len() >= 8) {
                if text.contains(part) {
                    bail!("report output would contain the value of {var}; refusing to write it");
                }
            }
        }
    }
    Ok(())
}

pub fn validate_cohorts(cohorts: &[RunFile]) -> Result<()> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for cohort in cohorts {
        *seen.entry(cohort.cohort_name()).or_default() += 1;
    }
    if let Some((name, _)) = seen.iter().find(|(_, count)| **count > 1) {
        bail!(
            "two results files share the cohort label {name:?}; cohorts are never merged, so give each its own --label"
        );
    }
    Ok(())
}

pub fn write_report(cohorts: &[RunFile], out_dir: &std::path::Path) -> Result<()> {
    validate_cohorts(cohorts)?;
    let html = render_html(cohorts);
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "schema": crate::results::SCHEMA,
        "cohorts": cohorts,
    }))?;
    let csv = crate::results::to_csv(cohorts);
    for text in [&html, &json, &csv] {
        ensure_clean(text)?;
    }
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    for (name, text) in [
        ("index.html", &html),
        ("results.json", &json),
        ("results.csv", &csv),
    ] {
        std::fs::write(out_dir.join(name), text)
            .with_context(|| format!("writing {}", out_dir.join(name).display()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- text ---

pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// One stat as words: the same numbers the chart draws.
pub fn describe(stat: &Stat) -> String {
    if stat.too_small() {
        return format!("n too small (n={})", stat.n);
    }
    if let (Some(events), Some(bound)) = (stat.events, stat.upper_bound) {
        return format!("{events}/{}, at most {bound:.4} (3/n)", stat.n);
    }
    let events = stat
        .events
        .map_or(String::new(), |e| format!("{e}/{}, ", stat.n));
    match stat.ci {
        Some([lo, hi]) => format!(
            "{events}{:.3} (95% CI {lo:.3}-{hi:.3}), n={}",
            stat.estimate, stat.n
        ),
        None => format!(
            "{events}{:.3} (no variance, CI undefined), n={}",
            stat.estimate, stat.n
        ),
    }
}

// --------------------------------------------------------------- series ---

/// The local baselines in a fixed order, so a system keeps its muted hue in
/// every report.
const KNOWN_BASELINES: [&str; 5] = [
    "verbatim",
    "bm25",
    "recency",
    "full-context",
    "bounded-window",
];
const COHORT_HUES: usize = 4;
const BASELINE_HUES: usize = 5;
const SHAPES: usize = 6;

struct Series<'a> {
    cohort: &'a RunFile,
    sys: &'a SystemResults,
    /// Colour class: `c0`..`c3` for an Achiral cohort (one saturated hue per
    /// cohort), `b0`..`b4` for a local baseline (a quieter hue per system).
    class: String,
    /// Marker shape, distinct per series so colour is never the only cue.
    shape: usize,
}

impl Series<'_> {
    fn name(&self) -> String {
        format!("{} · {}", self.sys.system, self.cohort.cohort_name())
    }
    fn is_achiral(&self) -> bool {
        self.sys.system == "achiral"
    }
}

fn all_series(cohorts: &[RunFile]) -> Vec<Series<'_>> {
    let mut out = Vec::new();
    let mut cohort_hue = 0;
    let mut extra: Vec<&str> = Vec::new();
    for cohort in cohorts {
        let hue = cohort_hue;
        if cohort.systems.iter().any(|s| s.system == "achiral") {
            cohort_hue += 1;
        }
        for sys in &cohort.systems {
            let class = if sys.system == "achiral" {
                format!("c{}", hue % COHORT_HUES)
            } else {
                let n = match KNOWN_BASELINES.iter().position(|k| *k == sys.system) {
                    Some(n) => n,
                    None => {
                        if !extra.contains(&sys.system.as_str()) {
                            extra.push(sys.system.as_str());
                        }
                        KNOWN_BASELINES.len()
                            + extra.iter().position(|e| *e == sys.system).unwrap_or(0)
                    }
                };
                format!("b{}", n % BASELINE_HUES)
            };
            let shape = out.len() % SHAPES;
            out.push(Series {
                cohort,
                sys,
                class,
                shape,
            });
        }
    }
    out
}

// ------------------------------------------------------------------ svg ---

const W: f64 = 520.0;
const PX0: f64 = 14.0;
const PX1: f64 = 380.0;

pub struct PlotRow {
    pub label: String,
    pub stat: Option<Stat>,
    pub strong: bool,
    pub marker: usize,
    /// Colour class of the row (`c0`, `b2`, `alarm`, ...).
    pub class: String,
    pub header: bool,
}

impl PlotRow {
    fn header(label: impl Into<String>) -> PlotRow {
        PlotRow {
            label: label.into(),
            stat: None,
            strong: false,
            marker: 0,
            class: String::new(),
            header: true,
        }
    }

    fn of(series: &Series<'_>, stat: Option<Stat>) -> PlotRow {
        PlotRow {
            label: series.name(),
            stat,
            strong: series.is_achiral(),
            marker: series.shape,
            class: series.class.clone(),
            header: false,
        }
    }
}

fn xpos(v: f64, max: f64) -> f64 {
    PX0 + (PX1 - PX0) * (v / max).clamp(0.0, 1.0)
}

/// A marker whose shape tells series apart without colour. `big` is the
/// Achiral series, the one saturated mark.
fn marker(shape: usize, x: f64, y: f64, big: bool) -> String {
    let k = if big { 1.3 } else { 1.0 };
    let class = if big { "pt big" } else { "pt" };
    match shape % SHAPES {
        0 => format!(
            r#"<circle class="{class}" cx="{x:.1}" cy="{y:.1}" r="{:.1}"/>"#,
            4.5 * k
        ),
        1 => format!(
            r#"<rect class="{class}" x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}"/>"#,
            x - 4.5 * k,
            y - 4.5 * k,
            9.0 * k,
            9.0 * k
        ),
        2 => format!(
            r#"<polygon class="{class}" points="{x:.1},{:.1} {:.1},{y:.1} {x:.1},{:.1} {:.1},{y:.1}"/>"#,
            y - 5.5 * k,
            x + 5.5 * k,
            y + 5.5 * k,
            x - 5.5 * k
        ),
        3 => format!(
            r#"<polygon class="{class}" points="{x:.1},{:.1} {:.1},{:.1} {:.1},{:.1}"/>"#,
            y - 5.0 * k,
            x + 5.0 * k,
            y + 4.0 * k,
            x - 5.0 * k,
            y + 4.0 * k
        ),
        4 => format!(
            r#"<polygon class="{class}" points="{x:.1},{:.1} {:.1},{:.1} {:.1},{:.1}"/>"#,
            y + 5.0 * k,
            x + 5.0 * k,
            y - 4.0 * k,
            x - 5.0 * k,
            y - 4.0 * k
        ),
        _ => {
            let (a, w) = (5.5 * k, 2.0 * k);
            format!(
                r#"<polygon class="{class}" points="{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}"/>"#,
                x - w,
                y - a,
                x + w,
                y - a,
                x + w,
                y - w,
                x + a,
                y - w,
                x + a,
                y + w,
                x + w,
                y + w,
                x + w,
                y + a,
                x - w,
                y + a,
                x - w,
                y + w,
                x - a,
                y + w,
                x - a,
                y - w,
                x - w,
                y - w
            )
        }
    }
}

/// A confidence line: a card-coloured halo under a dark stroke, so it stays
/// legible on top of any series colour.
fn wl(x1: f64, y1: f64, x2: f64, y2: f64, dash: bool) -> String {
    let d = if dash { " dash" } else { "" };
    format!(
        r#"<line class="halo" x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}"/><line class="whisker{d}" x1="{x1:.1}" y1="{y1:.1}" x2="{x2:.1}" y2="{y2:.1}"/>"#
    )
}

fn tick(v: f64) -> String {
    tick_x(v)
}

/// Rows of point estimates with 95% CI whiskers on an axis from zero.
/// A stat with no event and a 3/n bound draws a dashed bar to the bound; a
/// stat under ten questions draws a neutral hatch and says so.
pub fn dot_plot(title: &str, rows: &[PlotRow], axis_max: f64) -> String {
    let mut body = String::new();
    let mut y = 6.0;
    for row in rows {
        if row.header {
            let _ = write!(
                body,
                r#"<text class="grp" x="{PX0}" y="{:.1}">{}</text>"#,
                y + 15.0,
                esc(&row.label)
            );
            y += 24.0;
            continue;
        }
        let ly = y + 12.0;
        let py = y + 28.0;
        let weight = if row.strong { " strong" } else { "" };
        let _ = write!(
            body,
            r#"<g class="s {}"><text class="lbl{weight}" x="{:.1}" y="{ly:.1}">{}</text>"#,
            row.class,
            PX0 + 16.0,
            esc(&row.label)
        );
        body.push_str(&marker(row.marker, PX0 + 6.0, ly - 4.0, false));
        let _ = write!(
            body,
            r#"<line class="rail" x1="{PX0}" y1="{py:.1}" x2="{PX1}" y2="{py:.1}"/>"#
        );
        match &row.stat {
            None => {
                let _ = write!(
                    body,
                    r#"<text class="note" x="{:.1}" y="{:.1}">not run</text>"#,
                    PX1 + 10.0,
                    py + 4.0
                );
            }
            Some(stat) if stat.too_small() => {
                let _ = write!(
                    body,
                    r#"<rect class="hatch" x="{PX0}" y="{:.1}" width="{:.1}" height="14"/><text class="note" x="{:.1}" y="{:.1}">n too small (n={})</text>"#,
                    py - 7.0,
                    PX1 - PX0,
                    PX1 + 10.0,
                    py + 4.0,
                    stat.n
                );
            }
            Some(stat) => {
                if let Some(bound) = stat.upper_bound {
                    let bx = xpos(bound, axis_max);
                    body.push_str(&wl(xpos(0.0, axis_max), py, bx, py, true));
                    body.push_str(&wl(bx, py - 5.0, bx, py + 5.0, false));
                } else if let Some([lo, hi]) = stat.ci {
                    let (x0, x1) = (xpos(lo, axis_max), xpos(hi, axis_max));
                    body.push_str(&wl(x0, py, x1, py, false));
                    body.push_str(&wl(x0, py - 5.0, x0, py + 5.0, false));
                    body.push_str(&wl(x1, py - 5.0, x1, py + 5.0, false));
                }
                body.push_str(&marker(
                    row.marker,
                    xpos(stat.estimate, axis_max),
                    py,
                    row.strong,
                ));
                let short = match (stat.events, stat.upper_bound) {
                    (Some(e), Some(b)) => format!("{e}/{} ≤{b:.3}", stat.n),
                    (Some(e), None) => format!("{e}/{} {:.3}", stat.n, stat.estimate),
                    _ => format!("{:.3} n={}", stat.estimate, stat.n),
                };
                let _ = write!(
                    body,
                    r#"<text class="note" x="{:.1}" y="{:.1}">{}</text>"#,
                    PX1 + 10.0,
                    py + 4.0,
                    esc(&short)
                );
            }
        }
        body.push_str("</g>");
        y += 40.0;
    }
    let axis_y = y + 6.0;
    for (i, v) in [0.0, axis_max / 2.0, axis_max].iter().enumerate() {
        let x = xpos(*v, axis_max);
        let anchor = ["start", "middle", "end"][i];
        let _ = write!(
            body,
            r#"<line class="grid" x1="{x:.1}" y1="6" x2="{x:.1}" y2="{axis_y:.1}"/><text class="tick" x="{x:.1}" y="{:.1}" text-anchor="{anchor}">{}</text>"#,
            axis_y + 14.0,
            tick(*v)
        );
    }
    let height = axis_y + 22.0;
    format!(
        r#"<svg viewBox="0 0 {W} {height:.0}" role="img" aria-label="{t}"><title>{t}</title>{body}</svg>"#,
        t = esc(title)
    )
}

/// How one system compares with another on the questions both ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ahead,
    Behind,
    Even,
    TooSmall,
}

/// A paired difference is ahead or behind only when its 95% CI clears zero;
/// with no variance the sign of the difference decides.
pub fn outcome(diff: &Stat) -> Outcome {
    if diff.too_small() {
        return Outcome::TooSmall;
    }
    match diff.ci {
        Some([lo, _]) if lo > 0.0 => Outcome::Ahead,
        Some([_, hi]) if hi < 0.0 => Outcome::Behind,
        Some(_) => Outcome::Even,
        None if diff.estimate > 0.0 => Outcome::Ahead,
        None if diff.estimate < 0.0 => Outcome::Behind,
        None => Outcome::Even,
    }
}

pub struct DiffRow {
    pub label: String,
    pub header: bool,
    /// Mean per-question difference, Achiral minus the other system.
    pub stat: Option<Stat>,
}

fn signed(v: f64) -> String {
    if v < 0.0 {
        format!("−{:.3}", -v)
    } else {
        format!("+{v:.3}")
    }
}

/// The words that go beside a difference: direction is never left to colour.
pub fn direction_note(diff: &Stat) -> String {
    match outcome(diff) {
        Outcome::Ahead => format!("▲ ahead {}", signed(diff.estimate)),
        Outcome::Behind => format!("▼ behind {}", signed(diff.estimate)),
        Outcome::Even => format!("● no clear gap {}", signed(diff.estimate)),
        Outcome::TooSmall => format!("n too small (n={})", diff.n),
    }
}

/// Difference in recall, Achiral minus a baseline, centred on zero. Green
/// and ▲ when the 95% CI clears zero in Achiral's favour, orange-red and ▼
/// when it clears zero against; neutral when it spans zero. Losses draw as
/// large as wins.
pub fn diff_plot(title: &str, rows: &[DiffRow], lim: f64) -> String {
    let (lx, p0, p1, nx) = (14.0, 214.0, 390.0, 398.0);
    let pos = |v: f64| p0 + (p1 - p0) * ((v / lim).clamp(-1.0, 1.0) + 1.0) / 2.0;
    let mut body = String::new();
    let mut y = 4.0;
    for row in rows {
        if row.header {
            let _ = write!(
                body,
                r#"<text class="grp" x="{lx}" y="{:.1}">{}</text>"#,
                y + 15.0,
                esc(&row.label)
            );
            y += 22.0;
            continue;
        }
        let py = y + 12.0;
        let (class, note) = match &row.stat {
            None => ("even", "not compared".to_string()),
            Some(stat) => (
                match outcome(stat) {
                    Outcome::Ahead => "win",
                    Outcome::Behind => "loss",
                    _ => "even",
                },
                direction_note(stat),
            ),
        };
        let _ = write!(
            body,
            r#"<g class="s {class}"><text class="lbl sm" x="{lx}" y="{:.1}">{}</text><line class="rail" x1="{p0}" y1="{py:.1}" x2="{p1}" y2="{py:.1}"/>"#,
            py + 4.0,
            esc(&row.label)
        );
        match &row.stat {
            Some(stat) if stat.too_small() => {
                let _ = write!(
                    body,
                    r#"<rect class="hatch" x="{p0}" y="{:.1}" width="{:.1}" height="12"/>"#,
                    py - 6.0,
                    p1 - p0
                );
            }
            Some(stat) => {
                if let Some([lo, hi]) = stat.ci {
                    body.push_str(&wl(pos(lo), py, pos(hi), py, false));
                }
                let shape = match outcome(stat) {
                    Outcome::Ahead => 3,
                    Outcome::Behind => 4,
                    _ => 0,
                };
                body.push_str(&marker(shape, pos(stat.estimate), py, true));
            }
            None => {}
        }
        let _ = write!(
            body,
            r#"<text class="note dir" x="{nx}" y="{:.1}">{}</text></g>"#,
            py + 4.0,
            esc(&note)
        );
        y += 24.0;
    }
    let axis_y = y + 4.0;
    let _ = write!(
        body,
        r#"<line class="zero" x1="{0:.1}" y1="4" x2="{0:.1}" y2="{axis_y:.1}"/>"#,
        pos(0.0)
    );
    for v in [-lim, 0.0, lim] {
        let sign = if v < 0.0 {
            format!("−{}", tick(-v))
        } else if v > 0.0 {
            format!("+{}", tick(v))
        } else {
            "0".to_string()
        };
        let anchor = if v < 0.0 {
            "start"
        } else if v > 0.0 {
            "end"
        } else {
            "middle"
        };
        let _ = write!(
            body,
            r#"<text class="tick" x="{:.1}" y="{:.1}" text-anchor="{anchor}">{sign}</text>"#,
            pos(v),
            axis_y + 14.0
        );
    }
    let _ = write!(
        body,
        r#"<text class="tick" x="{p0}" y="{:.1}">← Achiral behind</text><text class="tick" x="{p1}" y="{:.1}" text-anchor="end">Achiral ahead →</text>"#,
        axis_y + 30.0,
        axis_y + 30.0
    );
    let height = axis_y + 38.0;
    format!(
        r#"<svg viewBox="0 0 {W} {height:.0}" role="img" aria-label="{t}"><title>{t}</title>{body}</svg>"#,
        t = esc(title)
    )
}

/// Predicted probability against observed frequency per bin, with the
/// diagonal of perfect calibration.
pub fn reliability_svg(name: &str, class: &str, bins: &[crate::results::BinRow]) -> String {
    let (size, pad) = (260.0, 34.0);
    let side = size - pad - 14.0;
    let px = |v: f64| pad + side * v.clamp(0.0, 1.0);
    let py = |v: f64| size - pad - side * v.clamp(0.0, 1.0);
    let mut body = String::new();
    for v in [0.0, 0.5, 1.0] {
        let _ = write!(
            body,
            r#"<line class="grid" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/><line class="grid" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/><text class="tick" x="{:.1}" y="{:.1}" text-anchor="middle">{v}</text><text class="tick" x="{:.1}" y="{:.1}" text-anchor="end">{v}</text>"#,
            px(v),
            py(0.0),
            px(v),
            py(1.0),
            px(0.0),
            py(v),
            px(1.0),
            py(v),
            px(v),
            py(0.0) + 14.0,
            px(0.0) - 5.0,
            py(v) + 4.0
        );
    }
    let _ = write!(
        body,
        r#"<line class="diag dash" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/>"#,
        px(0.0),
        py(0.0),
        px(1.0),
        py(1.0)
    );
    let points: Vec<String> = bins
        .iter()
        .map(|b| {
            format!(
                "{:.1},{:.1}",
                px(b.predicted_mean),
                py(b.observed_frequency)
            )
        })
        .collect();
    let _ = write!(
        body,
        r#"<g class="s {class}"><polyline class="line" points="{}"/>"#,
        points.join(" ")
    );
    for b in bins {
        body.push_str(&marker(
            0,
            px(b.predicted_mean),
            py(b.observed_frequency),
            true,
        ));
    }
    body.push_str("</g>");
    let _ = write!(
        body,
        r#"<text class="tick" x="{:.1}" y="{:.1}" text-anchor="middle">predicted probability</text><text class="tick" x="10" y="{:.1}" transform="rotate(-90 10 {:.1})" text-anchor="middle">observed frequency</text>"#,
        px(0.5),
        size - 4.0,
        py(0.5),
        py(0.5)
    );
    let label = format!("Reliability diagram, {name}");
    format!(
        r#"<svg class="sq" viewBox="0 0 {size} {size}" role="img" aria-label="{t}"><title>{t}</title>{body}</svg>"#,
        t = esc(&label)
    )
}

pub struct CycleSeries {
    pub name: String,
    pub class: String,
    pub shape: usize,
    pub strong: bool,
    pub pts: Vec<(usize, f64)>,
}

/// Mean tokens injected per question at each cycle checkpoint; x is log
/// scale, y starts at zero.
pub fn cycles_svg(series: &[CycleSeries]) -> String {
    let (h, left, right, top, bottom) = (260.0, 44.0, 380.0, 12.0, 44.0);
    let xs: Vec<f64> = series
        .iter()
        .flat_map(|s| s.pts.iter().map(|(c, _)| (*c as f64).max(1.0).ln()))
        .collect();
    let (xmin, xmax) = (
        xs.iter().cloned().fold(f64::MAX, f64::min),
        xs.iter().cloned().fold(f64::MIN, f64::max),
    );
    let ymax = series
        .iter()
        .flat_map(|s| s.pts.iter().map(|(_, t)| *t))
        .fold(0.0, f64::max)
        .max(1.0)
        * 1.1;
    let px = |c: usize| {
        if xmax > xmin {
            left + (right - left) * ((c as f64).max(1.0).ln() - xmin) / (xmax - xmin)
        } else {
            (left + right) / 2.0
        }
    };
    let py = |t: f64| h - bottom - (h - bottom - top) * (t / ymax);
    let mut body = String::new();
    let _ = write!(
        body,
        r#"<line class="grid" x1="{left}" y1="{:.1}" x2="{right}" y2="{:.1}"/><line class="grid" x1="{left}" y1="{top}" x2="{left}" y2="{:.1}"/><text class="tick" x="{:.1}" y="{:.1}" text-anchor="end">0</text><text class="tick" x="{:.1}" y="{:.1}" text-anchor="end">{ymax:.0}</text>"#,
        py(0.0),
        py(0.0),
        py(0.0),
        left - 5.0,
        py(0.0) + 4.0,
        left - 5.0,
        top + 8.0
    );
    let mut cycles: Vec<usize> = series
        .iter()
        .flat_map(|s| s.pts.iter().map(|(c, _)| *c))
        .collect();
    cycles.sort_unstable();
    cycles.dedup();
    for c in cycles {
        let _ = write!(
            body,
            r#"<text class="tick" x="{:.1}" y="{:.1}" text-anchor="middle">{c}</text>"#,
            px(c),
            py(0.0) + 14.0
        );
    }
    let _ = write!(
        body,
        r#"<text class="tick" x="{:.1}" y="{:.1}" text-anchor="middle">cycles (log scale)</text><text class="tick" x="10" y="{:.1}" transform="rotate(-90 10 {:.1})" text-anchor="middle">mean tokens per question</text>"#,
        (left + right) / 2.0,
        h - 6.0,
        (top + h - bottom) / 2.0,
        (top + h - bottom) / 2.0
    );
    // End labels stacked at least 14px apart, so lines that end together
    // stay readable.
    let mut order: Vec<usize> = (0..series.len()).collect();
    order.sort_by(|a, b| {
        let end = |i: usize| series[i].pts.last().map_or(0.0, |p| py(p.1));
        end(*a).total_cmp(&end(*b))
    });
    let mut label_y = vec![0.0_f64; series.len()];
    let mut floor = f64::MIN;
    for i in order {
        let want = series[i].pts.last().map_or(0.0, |p| py(p.1) + 4.0);
        floor = want.max(floor + 14.0);
        label_y[i] = floor;
    }
    for (si, s) in series.iter().enumerate() {
        let line: Vec<String> = s
            .pts
            .iter()
            .map(|(c, t)| format!("{:.1},{:.1}", px(*c), py(*t)))
            .collect();
        let _ = write!(
            body,
            r#"<g class="s {}"><polyline class="line" points="{}"/>"#,
            s.class,
            line.join(" ")
        );
        for (c, t) in &s.pts {
            body.push_str(&marker(s.shape, px(*c), py(*t), s.strong));
        }
        if let Some((c, _)) = s.pts.last() {
            let _ = write!(
                body,
                r#"<text class="note" x="{:.1}" y="{:.1}">{}</text>"#,
                px(*c) + 8.0,
                label_y[si],
                esc(&s.name)
            );
        }
        body.push_str("</g>");
    }
    format!(
        r#"<svg viewBox="0 0 {W} {h}" role="img" aria-label="Mean tokens injected per question by cycle"><title>Mean tokens injected per question by cycle</title>{body}</svg>"#
    )
}

/// A numeric setting on x, overall recall (with CI) on y from zero. Each
/// point keeps its own cohort's hue; the line between them is neutral.
pub fn curve_svg(key: &str, points: &[(f64, Stat, String)]) -> String {
    let (h, left, right, top, bottom) = (240.0, 44.0, 380.0, 12.0, 44.0);
    let xmin = points.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let xmax = points.iter().map(|p| p.0).fold(f64::MIN, f64::max);
    let px = |x: f64| {
        if xmax > xmin {
            left + (right - left) * (x - xmin) / (xmax - xmin)
        } else {
            (left + right) / 2.0
        }
    };
    let py = |v: f64| h - bottom - (h - bottom - top) * v.clamp(0.0, 1.0);
    let mut body = String::new();
    for v in [0.0, 0.5, 1.0] {
        let _ = write!(
            body,
            r#"<line class="grid" x1="{left}" y1="{:.1}" x2="{right}" y2="{:.1}"/><text class="tick" x="{:.1}" y="{:.1}" text-anchor="end">{v}</text>"#,
            py(v),
            py(v),
            left - 5.0,
            py(v) + 4.0
        );
    }
    let line: Vec<String> = points
        .iter()
        .map(|(x, s, _)| format!("{:.1},{:.1}", px(*x), py(s.estimate)))
        .collect();
    let _ = write!(
        body,
        r#"<polyline class="line neutral" points="{}"/>"#,
        line.join(" ")
    );
    for (x, s, class) in points {
        let _ = write!(body, r#"<g class="s {class}">"#);
        if let Some([lo, hi]) = s.ci {
            body.push_str(&wl(px(*x), py(lo), px(*x), py(hi), false));
        }
        body.push_str(&marker(0, px(*x), py(s.estimate), true));
        let _ = write!(
            body,
            r#"<text class="tick" x="{:.1}" y="{:.1}" text-anchor="middle">{}</text></g>"#,
            px(*x),
            py(0.0) + 14.0,
            tick_x(*x)
        );
    }
    let _ = write!(
        body,
        r#"<text class="tick" x="{:.1}" y="{:.1}" text-anchor="middle">{}</text>"#,
        (left + right) / 2.0,
        h - 6.0,
        esc(key)
    );
    let label = format!("Overall recall against {key}");
    format!(
        r#"<svg viewBox="0 0 {W} {h}" role="img" aria-label="{t}"><title>{t}</title>{body}</svg>"#,
        t = esc(&label)
    )
}

fn tick_x(x: f64) -> String {
    let s = format!("{x:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// --------------------------------------------------------------- labels ---

/// `activationNoiseScale=0.2 mode=aggr` -> pairs. Words without `=` are
/// ignored.
pub fn parse_label(label: &str) -> Vec<(String, String)> {
    label
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|word| {
            let (k, v) = word.split_once('=')?;
            (!k.is_empty() && !v.is_empty()).then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// Cohorts that differ in exactly one `key=value` setting and agree on the
/// rest: each group is one `key` with its values, ready to chart.
pub struct SettingGroup<'a> {
    pub key: String,
    pub others: String,
    pub members: Vec<(String, &'a RunFile)>,
}

pub fn setting_groups(cohorts: &[RunFile]) -> Vec<SettingGroup<'_>> {
    let mut map: BTreeMap<(String, String), Vec<(String, &RunFile)>> = BTreeMap::new();
    for cohort in cohorts {
        let Some(label) = &cohort.label else { continue };
        let pairs = parse_label(label);
        for (i, (key, value)) in pairs.iter().enumerate() {
            let mut rest: Vec<String> = pairs
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, (k, v))| format!("{k}={v}"))
                .collect();
            rest.sort();
            map.entry((key.clone(), rest.join(" ")))
                .or_default()
                .push((value.clone(), cohort));
        }
    }
    map.into_iter()
        .filter(|(_, members)| members.len() >= 2)
        .map(|((key, others), members)| SettingGroup {
            key,
            others,
            members,
        })
        .collect()
}

// ----------------------------------------------------------------- html ---

/// Colour tokens, light then dark. Colour carries meaning here: a saturated
/// hue per Achiral cohort (`c*`), quieter hues per local baseline (`b*`),
/// and outcome colours for ahead (`win`), behind (`loss`) and a leak
/// (`alarm`). Every graphic colour keeps at least 3:1 against the card in
/// both themes, and the cohort hues stay apart under the common colour-vision
/// deficiencies (both are tested).
pub const LIGHT: [(&str, &str); 20] = [
    ("bg", "#f9fafb"),
    ("fg", "#101828"),
    ("fg2", "#364153"),
    ("muted", "#6a7282"),
    ("line", "#d1d5dc"),
    ("card", "#ffffff"),
    ("c0", "#0072b2"),
    ("c1", "#b87a00"),
    ("c2", "#a23b72"),
    ("c3", "#5b4fc4"),
    ("b0", "#6b7f99"),
    ("b1", "#8f7545"),
    ("b2", "#5a8761"),
    ("b3", "#8c6a8c"),
    ("b4", "#4f8a8f"),
    ("win", "#00805e"),
    ("loss", "#c2410c"),
    ("alarm", "#b91c1c"),
    ("even", "#6a7282"),
    ("hatch", "#6a7282"),
];
pub const DARK: [(&str, &str); 20] = [
    ("bg", "#020618"),
    ("fg", "#f3f4f6"),
    ("fg2", "#d1d5dc"),
    ("muted", "#99a1af"),
    ("line", "#2a3547"),
    ("card", "#0f172a"),
    ("c0", "#56b4e9"),
    ("c1", "#e8b33a"),
    ("c2", "#e97fa7"),
    ("c3", "#8f86f5"),
    ("b0", "#8ea0b8"),
    ("b1", "#b89e74"),
    ("b2", "#86af8c"),
    ("b3", "#b592b5"),
    ("b4", "#77aeb3"),
    ("win", "#4cc9a0"),
    ("loss", "#ff8a5c"),
    ("alarm", "#ff6b6b"),
    ("even", "#99a1af"),
    ("hatch", "#99a1af"),
];

fn vars(theme: &[(&str, &str)]) -> String {
    theme
        .iter()
        .map(|(k, v)| format!("--{k}:{v}"))
        .collect::<Vec<_>>()
        .join(";")
}

fn css() -> String {
    format!(
        r#":root{{{light}}}
@media (prefers-color-scheme:dark){{:root{{{dark}}}}}
{rest}"#,
        light = vars(&LIGHT),
        dark = vars(&DARK),
        rest = CSS_RULES
    )
}

const CSS_RULES: &str = r#"*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:860px;margin:0 auto;padding:24px 16px 64px}
h1{font-size:22px;margin:0 0 4px}h2{font-size:17px;margin:36px 0 8px}h3{font-size:15px;margin:20px 0 6px}
p,li{color:var(--fg2)}.muted{color:var(--muted);font-size:13px}
.headline{color:var(--fg);font-weight:600;margin:6px 0;padding-left:10px;border-left:4px solid var(--muted)}
.headline.alarm{color:var(--alarm);border-left-color:var(--alarm)}
.headline.win{border-left-color:var(--win)}.headline.loss{border-left-color:var(--loss)}
section{border-top:1px solid var(--line);margin-top:8px}
.card{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:12px;margin:10px 0;overflow-x:auto}
svg{display:block;width:100%;height:auto;max-width:520px}svg.sq{max-width:260px}svg.defs{position:absolute;width:0;height:0}
svg text{fill:var(--fg);font-size:13px}svg .note,svg .tick{fill:var(--muted);font-size:12px}svg .grp{font-weight:600}svg .strong{font-weight:700}
svg .sm{font-size:12px}svg .dir{fill:var(--sc);font-weight:700}
svg .rail{stroke:var(--line);stroke-width:1}svg .grid{stroke:var(--line);stroke-width:1}
svg .halo{stroke:var(--card);stroke-width:4.5}
svg .whisker{stroke:var(--fg);stroke-width:1.6}svg .dash{stroke-dasharray:4 3}
svg .zero{stroke:var(--fg2);stroke-width:1.5}svg .diag{stroke:var(--muted);stroke-width:1.2}
svg .line{fill:none;stroke:var(--sc);stroke-width:2}svg .line.neutral{stroke:var(--muted);stroke-width:1.2}
svg .pt{fill:var(--sc);stroke:var(--card);stroke-width:1.5}svg .pt.big{stroke-width:2}
svg .hatch{fill:url(#hatch);stroke:var(--line)}svg .hatchline{stroke:var(--hatch);stroke-width:2}
.s{--sc:var(--muted)}.s.c0{--sc:var(--c0)}.s.c1{--sc:var(--c1)}.s.c2{--sc:var(--c2)}.s.c3{--sc:var(--c3)}
.s.b0{--sc:var(--b0)}.s.b1{--sc:var(--b1)}.s.b2{--sc:var(--b2)}.s.b3{--sc:var(--b3)}.s.b4{--sc:var(--b4)}
.s.win{--sc:var(--win)}.s.loss{--sc:var(--loss)}.s.alarm{--sc:var(--alarm)}.s.even{--sc:var(--even)}
.legend{list-style:none;display:flex;flex-wrap:wrap;gap:4px 16px;padding:0;margin:8px 0;font-size:13px}
.legend li{display:flex;align-items:center;gap:5px}.legend svg{width:16px;height:16px;max-width:16px}.legend .pt{stroke:none}
table{border-collapse:collapse;font-size:13px;width:100%}th,td{border-bottom:1px solid var(--line);padding:4px 8px;text-align:left;vertical-align:top}
th{color:var(--muted);font-weight:600}code{font-size:12px;word-break:break-all}
td.win{color:var(--win);font-weight:700}td.loss{color:var(--loss);font-weight:700}
.grid2{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:12px}
"#;

/// The hatch swatch for "n too small", defined once and shared by every chart.
const DEFS: &str = r#"<svg class="defs" aria-hidden="true" focusable="false"><defs><pattern id="hatch" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><line class="hatchline" x1="0" y1="0" x2="0" y2="6"/></pattern></defs></svg>"#;

fn table_open(headers: &[&str]) -> String {
    let mut out = String::from(r#"<div class="card"><table><thead><tr>"#);
    for h in headers {
        let _ = write!(out, "<th>{}</th>", esc(h));
    }
    out.push_str("</tr></thead><tbody>");
    out
}

/// One line of what a chart shows, in plain words and with n, so the finding
/// comes before the picture. Only what the numbers state.
fn headline(class: &str, text: &str) -> String {
    format!(r#"<p class="headline {class}">{}</p>"#, esc(text))
}

fn percent(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}

/// Marker and name for each series, so a colour is always paired with a shape
/// and a name.
fn legend(series: &[&Series<'_>]) -> String {
    let mut h = String::from(r#"<ul class="legend">"#);
    for s in series {
        let _ = write!(
            h,
            r#"<li><svg viewBox="0 0 16 16" aria-hidden="true"><g class="s {}">{}</g></svg><span>{}</span></li>"#,
            s.class,
            marker(s.shape, 8.0, 8.0, s.is_achiral()),
            esc(&s.name())
        );
    }
    h.push_str("</ul>");
    h
}

/// Mean per-question recall difference, `a` minus `b`, on the questions both
/// ran (all of them, or one category). `None` across different datasets.
fn paired_diff(a: &Series<'_>, b: &Series<'_>, category: Option<&str>) -> Option<Stat> {
    if a.cohort.run.stream_sha256 != b.cohort.run.stream_sha256 {
        return None;
    }
    let a_recall: HashMap<&str, f64> = a
        .sys
        .questions
        .iter()
        .map(|q| (q.id.as_str(), q.recall))
        .collect();
    let diffs: Vec<f64> = b
        .sys
        .questions
        .iter()
        .filter(|q| category.is_none_or(|c| q.category.as_deref() == Some(c)))
        .filter_map(|q| a_recall.get(q.id.as_str()).map(|r| r - q.recall))
        .collect();
    (!diffs.is_empty()).then(|| Stat::from_values(&diffs, a.cohort.run.seed))
}

fn headline_recall(series: &[&Series<'_>]) -> Vec<(String, String)> {
    let with: Vec<&&Series<'_>> = series.iter().filter(|s| s.sys.recall.is_some()).collect();
    let achiral: Vec<&&Series<'_>> = with.iter().copied().filter(|s| s.is_achiral()).collect();
    if achiral.is_empty() {
        return with
            .iter()
            .max_by(|a, b| {
                a.sys
                    .recall
                    .as_ref()
                    .unwrap()
                    .estimate
                    .total_cmp(&b.sys.recall.as_ref().unwrap().estimate)
            })
            .map(|s| {
                (
                    String::new(),
                    format!(
                        "Highest overall recall: {} at {}.",
                        s.name(),
                        describe(s.sys.recall.as_ref().unwrap())
                    ),
                )
            })
            .into_iter()
            .collect();
    }
    achiral
        .iter()
        .map(|a| {
            let own = describe(a.sys.recall.as_ref().unwrap());
            let best = with
                .iter()
                .filter(|b| !b.is_achiral() && paired_diff(a, b, None).is_some())
                .max_by(|x, y| {
                    x.sys.recall.as_ref().unwrap().estimate.total_cmp(&y.sys.recall.as_ref().unwrap().estimate)
                });
            match best {
                None => (String::new(), format!("{} overall recall {own}.", a.name())),
                Some(b) => {
                    let diff = paired_diff(a, b, None).unwrap();
                    let (class, verdict) = match outcome(&diff) {
                        Outcome::Ahead => ("win", "Achiral is ahead"),
                        Outcome::Behind => ("loss", "Achiral is behind"),
                        _ => ("", "no clear difference"),
                    };
                    let range = diff.ci.map_or(String::new(), |[lo, hi]| {
                        format!(" (95% CI {} to {})", signed(lo), signed(hi))
                    });
                    (
                        class.to_string(),
                        format!(
                            "{} overall recall {own}. Best local baseline {}: {}. Achiral minus {}: {}{range} on {} shared questions, so {verdict}.",
                            a.name(),
                            b.name(),
                            describe(b.sys.recall.as_ref().unwrap()),
                            b.sys.system,
                            signed(diff.estimate),
                            diff.n
                        ),
                    )
                }
            }
        })
        .collect()
}

/// At most three items, then a count of the rest, so a headline stays one
/// readable line however many series a chart holds.
fn limited(items: Vec<String>, max: usize) -> String {
    let extra = items.len().saturating_sub(max);
    let mut shown: Vec<String> = items.into_iter().take(max).collect();
    if extra > 0 {
        shown.push(format!("and {extra} more in the chart"));
    }
    shown.join("; ")
}

fn headline_flips(series: &[&Series<'_>]) -> String {
    // Series of one cohort with the same count share one sentence.
    type FlipGroup<'a> = (String, Option<usize>, usize, usize, Vec<String>, &'a Stat);
    let mut groups: Vec<FlipGroup<'_>> = Vec::new();
    for s in series {
        let Some(f) = s.sys.flips.as_ref() else {
            continue;
        };
        let cohort = s.cohort.cohort_name();
        match groups
            .iter_mut()
            .find(|g| g.0 == cohort && g.1 == f.events && g.2 == f.n)
        {
            Some(g) => g.4.push(s.sys.system.clone()),
            None => groups.push((
                cohort,
                f.events,
                f.n,
                s.cohort.run.replicates,
                vec![s.sys.system.clone()],
                f,
            )),
        }
    }
    let items = groups
        .into_iter()
        .map(|(cohort, events, n, r, names, f)| {
            let who = format!("{} · {cohort}", names.join(", "));
            match (events, f.upper_bound) {
                (Some(0), Some(b)) => format!(
                    "{who}: 0 of {n} questions changed outcome across {r} replicates (95% upper bound {} by 3/n)",
                    percent(b)
                ),
                (Some(e), _) => format!(
                    "{who}: {e} of {n} questions changed outcome across {r} replicates ({}{})",
                    percent(f.estimate),
                    f.ci.map_or(String::new(), |[lo, hi]| format!(
                        ", 95% CI {} to {}",
                        percent(lo),
                        percent(hi)
                    ))
                ),
                _ => format!("{who}: no flip count"),
            }
        })
        .collect();
    limited(items, 3) + "."
}

fn headline_calibration(series: &[&Series<'_>]) -> String {
    let items: Vec<String> = series
        .iter()
        .filter_map(|s| {
            let c = s.sys.calibration.as_ref()?;
            Some(format!(
                "{}: Brier {:.3}, expected calibration error {}",
                s.name(),
                c.brier,
                describe(&c.ece)
            ))
        })
        .collect::<Vec<_>>();
    limited(items, 3) + "."
}

fn headline_cycles(series: &[&Series<'_>]) -> String {
    let items: Vec<String> = series
        .iter()
        .filter_map(|s| {
            let p = s.sys.tokens_by_cycle.last()?;
            Some(format!(
                "{}: {:.1} tokens per question at cycle {} (n={} questions)",
                s.name(),
                p.mean_tokens,
                p.cycle,
                s.cohort.run.questions
            ))
        })
        .collect::<Vec<_>>();
    limited(items, 3) + "."
}

/// `(alarm, text)`: a leak above zero is stated first, as a leak.
fn headline_leaks(series: &[&Series<'_>]) -> (bool, String) {
    // (series name, category, stat) for every leak row
    let mut rows: Vec<(String, &str, &Stat)> = Vec::new();
    for s in series {
        for l in &s.sys.leaks {
            rows.push((s.name(), l.name.as_str(), &l.stat));
        }
    }
    let leaking: Vec<&(String, &str, &Stat)> = rows
        .iter()
        .filter(|r| r.2.events.is_some_and(|e| e > 0))
        .collect();
    if let Some(worst) = leaking
        .iter()
        .max_by(|a, b| a.2.estimate.total_cmp(&b.2.estimate))
    {
        let mut names: Vec<&str> = leaking.iter().map(|r| r.0.as_str()).collect();
        names.dedup();
        names.sort_unstable();
        names.dedup();
        return (
            true,
            format!(
                "Leak: {} of {} series returned another scope's memory. Highest rate: {}, {}: {} of {} trials ({}).",
                names.len(),
                series.len(),
                worst.0,
                worst.1,
                worst.2.events.unwrap_or(0),
                worst.2.n,
                percent(worst.2.estimate)
            ),
        );
    }
    let bound = rows
        .iter()
        .filter_map(|r| r.2.upper_bound)
        .fold(0.0, f64::max);
    let n: usize = rows.iter().map(|r| r.2.n).min().unwrap_or(0);
    (
        false,
        format!(
            "No leak in any trial run: {} series, at least n={n} trials each, 95% upper bound at most {} (3/n).",
            series.len(),
            percent(bound)
        ),
    )
}

pub fn render_html(cohorts: &[RunFile]) -> String {
    let series = all_series(cohorts);
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light dark"><title>Cognoscenti trust report</title><style>{}</style></head><body>{DEFS}<main><h1>Cognoscenti trust report</h1><p class="muted">Every rate shows its n and a 95% bootstrap interval over questions. Each cohort is a separate run, named by its label, with its own colour, and is never merged with another. Colour is never the only cue: every series also has its own marker shape and a name.</p>"#,
        css()
    );
    h.push_str(&manifest_section(cohorts));
    h.push_str(&recall_section(&series));
    h.push_str(&flip_section(&series));
    h.push_str(&calibration_section(&series));
    h.push_str(&cycles_section(&series));
    h.push_str(&leak_section(&series));
    h.push_str(&settings_section(cohorts, &series));
    h.push_str(&replicates_section(&series));
    h.push_str("</main></body></html>");
    h
}

/// What the cohort asked the server for and what the server said it used.
fn retrieval_cell(setting: Option<&crate::results::RetrievalSetting>) -> String {
    let Some(setting) = setting else {
        return "deployment defaults".to_string();
    };
    let mut cell = String::new();
    if let Some(mode) = &setting.mode {
        cell.push_str(&format!("requested mode {mode}"));
    }
    if let Some(params) = &setting.params {
        if !cell.is_empty() {
            cell.push_str("; ");
        }
        cell.push_str(&format!("requested params {params}"));
    }
    match &setting.echoed {
        Some(echo) => cell.push_str(&format!("; server echoed {echo}")),
        None => cell.push_str("; server echo not recorded"),
    }
    cell
}

fn manifest_section(cohorts: &[RunFile]) -> String {
    let mut h = String::from(r#"<section id="manifest"><h2>Run manifest</h2>"#);
    h.push_str(&table_open(&[
        "Cohort",
        "Dataset",
        "SHA-256",
        "Seed",
        "Top-k",
        "Replicates",
        "Questions",
        "Commit",
        "Retrieval",
        "Not run",
    ]));
    for c in cohorts {
        let not_run = if c.run.not_run.is_empty() {
            "none".to_string()
        } else {
            c.run
                .not_run
                .iter()
                .map(|n| format!("{} {}", n.category, n.trials))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
            esc(&c.cohort_name()),
            esc(&c.run.stream_file),
            esc(&c.run.stream_sha256),
            c.run.seed,
            c.run.top_k,
            c.run.replicates,
            c.run.questions,
            esc(c.run.cognoscenti_commit.as_deref().map_or("unknown", |s| &s[..s.len().min(12)])),
            esc(&retrieval_cell(c.run.retrieval.as_ref())),
            esc(&not_run),
        );
    }
    h.push_str("</tbody></table></div><h3>Limits</h3><ul>");
    let mut limits: Vec<&String> = Vec::new();
    for c in cohorts {
        for l in &c.run.limits {
            if !limits.contains(&l) {
                limits.push(l);
            }
        }
    }
    for l in limits {
        let _ = write!(h, "<li>{}</li>", esc(l));
    }
    h.push_str("</ul></section>");
    h
}

fn recall_section(series: &[Series<'_>]) -> String {
    let with_recall: Vec<&Series<'_>> = series.iter().filter(|s| s.sys.recall.is_some()).collect();
    let mut h = String::from(r#"<section id="recall"><h2>Recall by category</h2>"#);
    if with_recall.is_empty() {
        h.push_str(r#"<p class="muted">No recall results in these files.</p></section>"#);
        return h;
    }
    let mut categories: Vec<String> = Vec::new();
    for s in &with_recall {
        for c in &s.sys.categories {
            if !categories.contains(&c.name) {
                categories.push(c.name.clone());
            }
        }
    }
    for (class, line) in headline_recall(&with_recall) {
        h.push_str(&headline(&class, &line));
    }
    let mut rows = vec![PlotRow::header("All questions")];
    for s in &with_recall {
        rows.push(PlotRow::of(s, s.sys.recall.clone()));
    }
    for category in &categories {
        rows.push(PlotRow::header(format!("Category: {category}")));
        for s in &with_recall {
            rows.push(PlotRow::of(
                s,
                s.sys
                    .categories
                    .iter()
                    .find(|c| &c.name == category)
                    .map(|c| c.stat.clone()),
            ));
        }
    }
    let _ = write!(
        h,
        r#"{}<p class="muted">Recall@k per question, 0 to 1. The large filled mark is Achiral; the smaller muted marks are the local baselines. A hatched bar is a category under 10 questions: "n too small".</p><div class="card">{}</div>"#,
        legend(&with_recall),
        dot_plot("Recall by category and system", &rows, 1.0)
    );
    h.push_str(&gap_charts(&with_recall, &categories));
    h.push_str(&head_to_head(&with_recall, &categories, &[]));
    h.push_str("<h3>Values</h3>");
    let mut headers = vec!["Category".to_string()];
    headers.extend(with_recall.iter().map(|s| s.name()));
    let header_refs: Vec<&str> = headers.iter().map(String::as_str).collect();
    h.push_str(&table_open(&header_refs));
    let mut line = |name: &str, pick: &dyn Fn(&SystemResults) -> Option<Stat>| {
        let _ = write!(h, "<tr><td>{}</td>", esc(name));
        for s in &with_recall {
            let cell = pick(s.sys).map_or("not run".to_string(), |st| describe(&st));
            let _ = write!(h, "<td>{}</td>", esc(&cell));
        }
        h.push_str("</tr>");
    };
    line("All questions", &|s| s.recall.clone());
    for category in &categories {
        let c = category.clone();
        line(category, &move |s| {
            s.categories
                .iter()
                .find(|x| x.name == c)
                .map(|x| x.stat.clone())
        });
    }
    h.push_str("</tbody></table></div></section>");
    h
}

/// Where Achiral is ahead of and behind each local baseline, per category,
/// as a paired difference with its 95% CI.
fn gap_charts(with_recall: &[&Series<'_>], categories: &[String]) -> String {
    let mut h = String::new();
    for a in with_recall.iter().filter(|s| s.is_achiral()) {
        let baselines: Vec<&&Series<'_>> = with_recall
            .iter()
            .filter(|b| !b.is_achiral() && paired_diff(a, b, None).is_some())
            .collect();
        if baselines.is_empty() {
            continue;
        }
        let mut rows = vec![DiffRow {
            label: "All questions".into(),
            header: true,
            stat: None,
        }];
        let mut scopes: Vec<Option<&str>> = vec![None];
        scopes.extend(categories.iter().map(|c| Some(c.as_str())));
        for scope in scopes {
            if let Some(c) = scope {
                rows.push(DiffRow {
                    label: format!("Category: {c}"),
                    header: true,
                    stat: None,
                });
            }
            for b in &baselines {
                let stat = paired_diff(a, b, scope);
                let n = stat.as_ref().map_or(0, |s| s.n);
                rows.push(DiffRow {
                    label: format!("{} · n={n}", b.name()),
                    header: false,
                    stat,
                });
            }
        }
        let lim = rows
            .iter()
            .filter_map(|r| r.stat.as_ref())
            .filter(|s| !s.too_small())
            .map(|s| {
                s.ci.map_or(s.estimate.abs(), |c| c[0].abs().max(c[1].abs()))
            })
            .fold(0.1, f64::max);
        let lim = [0.1, 0.2, 0.5, 1.0]
            .into_iter()
            .find(|c| *c >= lim)
            .unwrap_or(1.0);
        let _ = write!(
            h,
            r#"<h3>{} against each baseline</h3><p class="muted">Recall difference per question, Achiral minus the baseline, with its 95% CI. ▲ green: the CI is above zero, Achiral is ahead. ▼ orange-red: the CI is below zero, Achiral is behind. ● grey: the CI spans zero. A hatched row is under 10 questions.</p><div class="card">{}</div>"#,
            esc(&a.name()),
            diff_plot(&format!("Recall difference, {}", a.name()), &rows, lim)
        );
    }
    h
}

/// Where each baseline beat Achiral: questions the baseline recalled in full
/// and Achiral did not, per category, with an exact McNemar p-value over all
/// shared questions. Only cohorts run on the same dataset are compared.
fn head_to_head(
    with_recall: &[&Series<'_>],
    categories: &[String],
    _cohorts: &[RunFile],
) -> String {
    let mut h = String::new();
    for a in with_recall.iter().filter(|s| s.is_achiral()) {
        let a_pass: HashMap<&str, bool> = a
            .sys
            .questions
            .iter()
            .map(|q| (q.id.as_str(), q.recall >= 1.0))
            .collect();
        let mut rows = String::new();
        for b in with_recall
            .iter()
            .filter(|s| !s.is_achiral() && s.cohort.run.stream_sha256 == a.cohort.run.stream_sha256)
        {
            let mut xs = Vec::new();
            let mut ys = Vec::new();
            let mut losses: BTreeMap<&str, usize> = BTreeMap::new();
            for q in &b.sys.questions {
                let Some(&ap) = a_pass.get(q.id.as_str()) else {
                    continue;
                };
                let bp = q.recall >= 1.0;
                xs.push(ap);
                ys.push(bp);
                if bp && !ap {
                    if let Some(c) = &q.category {
                        *losses.entry(c.as_str()).or_default() += 1;
                    }
                }
            }
            if xs.is_empty() {
                continue;
            }
            let m = mcnemar_exact(&xs, &ys);
            let _ = write!(
                rows,
                r#"<tr><td>{}</td><td class="win">▲ {}</td><td class="loss">▼ {}</td><td>{:.4}</td>"#,
                esc(&b.name()),
                m.only_a,
                m.only_b,
                m.p_value
            );
            for c in categories {
                match losses.get(c.as_str()).copied().unwrap_or(0) {
                    0 => rows.push_str("<td>0</td>"),
                    n => {
                        let _ = write!(rows, r#"<td class="loss">▼ {n}</td>"#);
                    }
                }
            }
            rows.push_str("</tr>");
        }
        if rows.is_empty() {
            continue;
        }
        let _ = write!(h, "<h3>{} against each baseline</h3><p class=\"muted\">Questions recalled in full by one system and not the other, on the questions both ran. Per-category columns count the questions the baseline won. p is an exact McNemar test.</p>", esc(&a.name()));
        let mut headers: Vec<String> = ["Baseline", "Achiral only", "Baseline only", "p"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        headers.extend(categories.iter().map(|c| format!("won: {c}")));
        let refs: Vec<&str> = headers.iter().map(String::as_str).collect();
        h.push_str(&table_open(&refs));
        h.push_str(&rows);
        h.push_str("</tbody></table></div>");
    }
    h
}

fn flip_section(series: &[Series<'_>]) -> String {
    let with: Vec<&Series<'_>> = series.iter().filter(|s| s.sys.flips.is_some()).collect();
    let mut h = String::from(r#"<section id="flips"><h2>Flip rate across replicates</h2>"#);
    if with.is_empty() {
        h.push_str(r#"<p class="muted">No replicated run in these files.</p></section>"#);
        return h;
    }
    let max = axis_max(with.iter().filter_map(|s| s.sys.flips.as_ref()));
    let rows: Vec<PlotRow> = with
        .iter()
        .map(|s| PlotRow::of(s, s.sys.flips.clone()))
        .collect();
    h.push_str(&headline("", &headline_flips(&with)));
    let _ = write!(
        h,
        r#"{}<p class="muted">Share of questions whose pass or fail changed between replicates, each run in its own fresh org. Achiral memory is non-deterministic, so replicates can differ; a local baseline repeats exactly. With no flip, the bar shows the 3/n upper bound.</p><div class="card">{}</div></section>"#,
        legend(&with),
        dot_plot("Flip rate by system", &rows, max)
    );
    h
}

/// A rate axis from zero, rounded up to a tidy ceiling above every bound
/// and interval shown.
fn axis_max<'a>(stats: impl Iterator<Item = &'a Stat>) -> f64 {
    let top = stats
        .map(|s| {
            s.upper_bound
                .or(s.ci.map(|c| c[1]))
                .unwrap_or(s.estimate)
                .max(s.estimate)
        })
        .fold(0.0, f64::max);
    [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0]
        .into_iter()
        .find(|c| *c >= top)
        .unwrap_or(1.0)
}

fn calibration_section(series: &[Series<'_>]) -> String {
    let mut h = String::from(r#"<section id="calibration"><h2>Calibration</h2>"#);
    let with: Vec<&Series<'_>> = series
        .iter()
        .filter(|s| s.sys.calibration.is_some())
        .collect();
    if with.is_empty() {
        h.push_str(r#"<p class="muted">No calibration results in these files.</p></section>"#);
        return h;
    }
    h.push_str(&headline("", &headline_calibration(&with)));
    h.push_str(r#"<p class="muted">Of memories retrieved at a given probability, how often one was needed. The dashed diagonal is perfect calibration. Probabilities are derived from each system's ranking.</p><div class="grid2">"#);
    for s in with {
        let cal = s.sys.calibration.as_ref().unwrap();
        let _ = write!(
            h,
            r#"<div class="card"><h3>{}</h3>{}<p class="muted">Brier {:.4}. ECE {}. Basis: {}.</p></div>"#,
            esc(&s.name()),
            reliability_svg(&s.name(), &s.class, &cal.bins),
            cal.brier,
            esc(&describe(&cal.ece)),
            esc(&cal.basis)
        );
    }
    h.push_str("</div></section>");
    h
}

fn cycles_section(series: &[Series<'_>]) -> String {
    let mut h = String::from(r#"<section id="cycles"><h2>Tokens injected by cycle</h2>"#);
    let with: Vec<&Series<'_>> = series
        .iter()
        .filter(|s| !s.sys.tokens_by_cycle.is_empty())
        .collect();
    if with.is_empty() {
        h.push_str(r#"<p class="muted">No cycle run in these files.</p></section>"#);
        return h;
    }
    let data: Vec<CycleSeries> = with
        .iter()
        .map(|s| CycleSeries {
            name: s.name(),
            class: s.class.clone(),
            shape: s.shape,
            strong: s.is_achiral(),
            pts: s
                .sys
                .tokens_by_cycle
                .iter()
                .map(|p| (p.cycle, p.mean_tokens))
                .collect(),
        })
        .collect();
    h.push_str(&headline("", &headline_cycles(&with)));
    let _ = write!(
        h,
        r#"{}<p class="muted">Mean tokens a question would inject as history grows. Read path only; whitespace-separated words.</p><div class="card">{}</div></section>"#,
        legend(&with),
        cycles_svg(&data)
    );
    h
}

fn leak_section(series: &[Series<'_>]) -> String {
    let mut h = String::from(r#"<section id="leaks"><h2>Leaks</h2>"#);
    let mut names: Vec<String> = Vec::new();
    for s in series {
        for l in &s.sys.leaks {
            if !names.contains(&l.name) {
                names.push(l.name.clone());
            }
        }
    }
    if names.is_empty() {
        h.push_str(r#"<p class="muted">No leak trials in these files.</p></section>"#);
        return h;
    }
    let all: Vec<&Stat> = series
        .iter()
        .flat_map(|s| s.sys.leaks.iter().map(|l| &l.stat))
        .collect();
    let max = axis_max(all.into_iter());
    let mut rows = Vec::new();
    let mut shown: Vec<&Series<'_>> = Vec::new();
    for name in &names {
        rows.push(PlotRow::header(format!("Category: {name}")));
        for s in series
            .iter()
            .filter(|s| s.sys.leaks.iter().any(|l| &l.name == name))
        {
            if !shown.iter().any(|x| std::ptr::eq(*x, s)) {
                shown.push(s);
            }
            let stat = s
                .sys
                .leaks
                .iter()
                .find(|l| &l.name == name)
                .map(|l| l.stat.clone());
            let mut row = PlotRow::of(s, stat.clone());
            // a leak above zero is the one thing drawn in the alarm colour
            if stat
                .as_ref()
                .and_then(|st| st.events)
                .is_some_and(|e| e > 0)
            {
                row.class = "alarm".into();
                row.label = format!("LEAK · {}", row.label);
            }
            rows.push(row);
        }
    }
    let (alarm, text) = headline_leaks(&shown);
    h.push_str(&headline(if alarm { "alarm" } else { "win" }, &text));
    let _ = write!(
        h,
        r#"{}<p class="muted">Share of trials where a memory its asker may not see came back. With none, the dashed bar is the 3/n upper bound: with n trials and no leak, the true rate is at most 3/n with 95% confidence. A leak above zero is drawn in red and labelled LEAK.</p><div class="card">{}</div></section>"#,
        legend(&shown),
        dot_plot("Leak rate by category and system", &rows, max)
    );
    h
}

fn settings_section(cohorts: &[RunFile], series: &[Series<'_>]) -> String {
    let mut h = String::from(r#"<section id="settings"><h2>Metric against a setting</h2>"#);
    let groups = setting_groups(cohorts);
    let mut drew = false;
    for g in groups {
        let pts: Vec<(String, Stat, String)> = g
            .members
            .iter()
            .filter_map(|(v, c)| {
                let s = series
                    .iter()
                    .find(|s| std::ptr::eq(s.cohort, *c) && s.is_achiral())?;
                Some((v.clone(), s.sys.recall.clone()?, s.class.clone()))
            })
            .collect();
        if pts.len() < 2 {
            continue;
        }
        drew = true;
        let others = if g.others.is_empty() {
            String::new()
        } else {
            format!(" (other settings: {})", g.others)
        };
        let _ = write!(
            h,
            "<h3>Achiral recall against {}{}</h3>",
            esc(&g.key),
            esc(&others)
        );
        let said: Vec<String> = pts
            .iter()
            .map(|(v, s, _)| {
                format!(
                    "{}={v}: {} (n={})",
                    g.key,
                    format_args!("{:.3}", s.estimate),
                    s.n
                )
            })
            .collect();
        h.push_str(&headline(
            "",
            &format!("Achiral overall recall by cohort, {}.", said.join("; ")),
        ));
        let numeric: Option<Vec<(f64, Stat, String)>> = pts
            .iter()
            .map(|(v, s, c)| v.parse::<f64>().ok().map(|x| (x, s.clone(), c.clone())))
            .collect();
        match numeric {
            Some(mut points) => {
                points.sort_by(|a, b| a.0.total_cmp(&b.0));
                let _ = write!(
                    h,
                    r#"<div class="card">{}</div>"#,
                    curve_svg(&g.key, &points)
                );
            }
            None => {
                let rows: Vec<PlotRow> = pts
                    .iter()
                    .enumerate()
                    .map(|(i, (v, s, c))| PlotRow {
                        label: format!("{}={v}", g.key),
                        stat: Some(s.clone()),
                        strong: true,
                        marker: i,
                        class: c.clone(),
                        header: false,
                    })
                    .collect();
                let _ = write!(
                    h,
                    r#"<div class="card">{}</div>"#,
                    dot_plot(&format!("Achiral recall by {}", g.key), &rows, 1.0)
                );
            }
        }
    }
    if !drew {
        h.push_str(r#"<p class="muted">Needs two or more cohorts whose labels differ in one key=value setting, such as activationNoiseScale=0 and activationNoiseScale=0.2.</p>"#);
    }
    h.push_str("</section>");
    h
}

fn replicates_section(series: &[Series<'_>]) -> String {
    let with: Vec<&Series<'_>> = series
        .iter()
        .filter(|s| !s.sys.replicates.is_empty())
        .collect();
    let mut h = String::from(r#"<section id="replicates"><h2>Replicates</h2>"#);
    if with.is_empty() {
        h.push_str(r#"<p class="muted">No per-replicate summaries in these files.</p></section>"#);
        return h;
    }
    h.push_str(&table_open(&[
        "Series",
        "Replicate",
        "Mean recall",
        "Fully recalled",
        "Leaked trials",
    ]));
    for s in with {
        for r in &s.sys.replicates {
            let _ = write!(
                h,
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td></tr>",
                esc(&s.name()),
                r.index,
                r.mean_recall
                    .map_or("not kept".to_string(), |m| format!("{m:.4}")),
                r.fully_recalled,
                r.questions,
                match (r.leaked_trials, r.leak_trials) {
                    (Some(a), Some(b)) => format!("{a}/{b}"),
                    _ => "not kept".to_string(),
                }
            );
        }
    }
    h.push_str("</tbody></table></div></section>");
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::QuestionResult;
    use crate::results::{BinRow, Calibration, CyclePoint, ReplicateRow, RunInfo};
    use crate::stream::Stream;

    fn info(sha: &str) -> RunInfo {
        RunInfo {
            stream_file: "stream.jsonl".into(),
            stream_sha256: sha.into(),
            seed: 0,
            top_k: 3,
            replicates: 3,
            questions: 24,
            cognoscenti_commit: Some("0123456789abcdef".into()),
            skip_user_scope: true,
            not_run: vec![crate::results::NotRun {
                category: "owner".into(),
                trials: 5,
            }],
            limits: crate::results::standard_limits(),
            retrieval: None,
        }
    }

    /// `pattern` lists recall per question for each category, in order.
    fn system(name: &str, pattern: &[(&str, &[f64])]) -> SystemResults {
        let mut results = Vec::new();
        for (category, recalls) in pattern {
            for (i, r) in recalls.iter().enumerate() {
                results.push(QuestionResult {
                    question_id: format!("{category}-{i:03}-q"),
                    retrieved_ids: vec![],
                    recall_at_k: *r,
                    tokens_injected: 30,
                    scored_candidates: vec![],
                    gap_disclosure: None,
                });
            }
        }
        SystemResults::from_replay(name, &Stream { items: vec![] }, &results, 0)
    }

    const BIG: [f64; 12] = [1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0];
    const BIG_B: [f64; 12] = [1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    const TINY: [f64; 4] = [1.0, 0.0, 1.0, 1.0];

    fn cohort(label: Option<&str>, sha: &str, systems: Vec<SystemResults>) -> RunFile {
        let mut file = RunFile::new(label.map(String::from), info(sha));
        file.systems = systems;
        file
    }

    fn fixture() -> Vec<RunFile> {
        let mut achiral = system("achiral", &[("scope", &BIG), ("owner", &TINY)]);
        achiral.flips = Some(Stat::from_events(&[false; 24], 0));
        achiral.leaks = vec![crate::results::NamedStat {
            name: "scope".into(),
            stat: Stat::from_events(&[false; 71], 0),
        }];
        achiral.calibration = Some(Calibration {
            brier: 0.18,
            ece: Stat::from_values(&BIG, 0),
            bins: vec![
                BinRow {
                    predicted_mean: 0.2,
                    observed_frequency: 0.1,
                    count: 30,
                },
                BinRow {
                    predicted_mean: 0.8,
                    observed_frequency: 0.7,
                    count: 20,
                },
            ],
            basis: "derived".into(),
        });
        achiral.replicates = vec![ReplicateRow {
            index: 0,
            questions: 24,
            fully_recalled: 18,
            mean_recall: Some(0.75),
            leaked_trials: Some(0),
            leak_trials: Some(71),
        }];
        let bm25 = system("bm25", &[("scope", &BIG_B), ("owner", &TINY)]);
        vec![
            cohort(Some("noise 0"), "sha-a", vec![achiral]),
            cohort(Some("baselines"), "sha-a", vec![bm25]),
        ]
    }

    #[test]
    fn output_with_a_key_an_endpoint_or_a_scheme_is_refused() {
        assert!(ensure_clean("fine text").is_ok());
        assert!(ensure_clean("a bare http word").is_err());
        for bad in [
            "see https://x",
            "acm_abcdef1234",
            "Authorization: Bearer x",
            "localhost:7071",
            "127.0.0.1",
            "COGNOSCENTI_ACHIRAL_API_KEY",
            "a://b",
        ] {
            assert!(ensure_clean(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_value_of_an_adapter_env_variable_is_refused_too() {
        std::env::set_var(
            "COGNOSCENTI_ACHIRAL_API_KEYS",
            "zzkey-one-12345,zzkey-two-67890",
        );
        let err = ensure_clean("label zzkey-two-67890 here")
            .unwrap_err()
            .to_string();
        std::env::remove_var("COGNOSCENTI_ACHIRAL_API_KEYS");
        assert!(err.contains("COGNOSCENTI_ACHIRAL_API_KEYS"));
        assert!(
            !err.contains("zzkey"),
            "the error must not repeat the value"
        );
    }

    #[test]
    fn two_cohorts_with_one_label_are_never_merged() {
        let c = vec![
            cohort(Some("noise 0"), "a", vec![]),
            cohort(Some("noise 0"), "a", vec![]),
        ];
        assert!(validate_cohorts(&c)
            .unwrap_err()
            .to_string()
            .contains("never merged"));
        let u = vec![cohort(None, "a", vec![]), cohort(None, "a", vec![])];
        assert!(validate_cohorts(&u).is_err());
        assert!(validate_cohorts(&fixture()).is_ok());
    }

    #[test]
    fn labels_parse_into_key_value_pairs() {
        assert_eq!(
            parse_label("activationNoiseScale=0.2, mode=aggr plain words"),
            vec![
                ("activationNoiseScale".into(), "0.2".into()),
                ("mode".into(), "aggr".into())
            ]
        );
        assert!(parse_label("noise 0").is_empty());
        assert!(parse_label("=x k=").is_empty());
    }

    #[test]
    fn cohorts_group_only_when_every_other_setting_agrees() {
        let a = |label: &str| cohort(Some(label), "s", vec![]);
        let cohorts = vec![
            a("activationNoiseScale=0 mode=aggr"),
            a("activationNoiseScale=0.2 mode=aggr"),
            a("activationNoiseScale=0.2 mode=rerank"),
            a("unrelated"),
        ];
        let groups = setting_groups(&cohorts);
        let noise: Vec<_> = groups
            .iter()
            .filter(|g| g.key == "activationNoiseScale")
            .collect();
        assert_eq!(noise.len(), 1, "mode=rerank has no partner, so no group");
        let aggr = noise[0];
        assert_eq!(aggr.others, "mode=aggr");
        assert_eq!(aggr.members.len(), 2);
        let mode = groups.iter().find(|g| g.key == "mode").unwrap();
        assert_eq!(mode.others, "activationNoiseScale=0.2");
        assert_eq!(mode.members.len(), 2);
        assert_eq!(groups.len(), 2, "single-member groups are dropped");
    }

    fn row(label: &str, stat: Option<Stat>) -> PlotRow {
        PlotRow {
            label: label.into(),
            stat,
            strong: false,
            marker: 0,
            class: "b0".into(),
            header: false,
        }
    }

    #[test]
    fn a_dot_plot_is_an_accessible_inline_svg_with_a_zero_axis() {
        let stat = Stat::from_values(&BIG, 0);
        let svg = dot_plot(
            "Recall <by> system",
            &[row("achiral · noise 0", Some(stat))],
            1.0,
        );
        assert!(svg.starts_with("<svg viewBox="));
        assert!(svg.contains(r#"role="img""#));
        assert!(svg.contains("Recall &lt;by&gt; system"));
        assert!(!svg.contains("<script") && !svg.contains("xmlns") && !svg.contains("http"));
        assert!(
            svg.contains(r#"text-anchor="start">0</text>"#),
            "axis starts at zero"
        );
        assert!(svg.contains(r#"text-anchor="end">1</text>"#));
        assert_eq!(
            svg.matches(r#"class="pt""#).count(),
            2,
            "the label glyph and the estimate"
        );
        assert!(svg.contains("0.750 n=12"));
    }

    #[test]
    fn a_category_too_small_for_a_ci_draws_no_point() {
        let svg = dot_plot("t", &[row("owner", Some(Stat::from_values(&TINY, 0)))], 1.0);
        assert!(svg.contains("n too small (n=4)"));
        assert_eq!(
            svg.matches(r#"class="pt"#).count(),
            1,
            "only the label glyph, no estimate"
        );
        assert!(svg.contains(r#"class="hatch""#), "a neutral hatch");
        assert!(!svg.contains("whisker"));
    }

    #[test]
    fn a_zero_event_rate_draws_the_three_over_n_bar() {
        let svg = dot_plot(
            "t",
            &[row("scope", Some(Stat::from_events(&[false; 71], 0)))],
            0.05,
        );
        assert!(svg.contains("whisker dash"));
        assert!(svg.contains("0/71 ≤0.042"));
    }

    #[test]
    fn a_system_that_was_not_run_says_so() {
        assert!(dot_plot("t", &[row("owner", None)], 1.0).contains("not run"));
    }

    #[test]
    fn the_rate_axis_rounds_up_above_every_bound_and_interval() {
        let zero = Stat::from_events(&[false; 71], 0);
        assert_eq!(axis_max([&zero].into_iter()), 0.05);
        let some = Stat::from_events(&(0..40).map(|i| i % 4 == 0).collect::<Vec<_>>(), 0);
        assert!(axis_max([&some].into_iter()) >= some.ci.unwrap()[1]);
    }

    #[test]
    fn describe_reads_back_the_numbers_a_chart_plots() {
        assert_eq!(describe(&Stat::from_values(&TINY, 0)), "n too small (n=4)");
        assert_eq!(
            describe(&Stat::from_events(&[false; 71], 0)),
            "0/71, at most 0.0423 (3/n)"
        );
        assert!(describe(&Stat::from_values(&BIG, 0)).contains("(95% CI "));
        assert!(describe(&Stat::from_values(&[1.0; 12], 0)).contains("no variance"));
    }

    #[test]
    fn the_page_is_self_contained_and_readable_at_phone_width_in_light_and_dark() {
        let html = render_html(&fixture());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains(r#"name="viewport" content="width=device-width,initial-scale=1""#));
        assert!(html.contains("prefers-color-scheme:dark"));
        assert!(html.contains("overflow-x:auto"));
        for forbidden in ["<script", "http", "<link", "@import", "src="] {
            assert!(!html.contains(forbidden), "{forbidden}");
        }
        assert!(ensure_clean(&html).is_ok());
        // an inline reference may only point at a fragment of this page
        for (i, _) in html.match_indices("url(") {
            assert_eq!(&html[i + 4..i + 5], "#", "url( must be a fragment");
        }
    }

    #[test]
    fn the_page_has_every_section_and_the_manifest_summary() {
        let html = render_html(&fixture());
        for id in [
            "manifest",
            "recall",
            "flips",
            "calibration",
            "cycles",
            "leaks",
            "settings",
            "replicates",
        ] {
            assert!(html.contains(&format!(r#"<section id="{id}">"#)), "{id}");
        }
        assert!(html.contains("sha-a"));
        assert!(html.contains("0123456789ab"));
        assert!(!html.contains("0123456789abcdef"), "commit is shortened");
        assert!(html.contains("owner 5"), "not-run trials are listed");
        assert!(html.contains("whitespace-separated words"));
        assert!(html.contains("read path only"));
        assert_eq!(html.matches("<svg").count(), html.matches("</svg>").count());
    }

    #[test]
    fn each_cohort_keeps_its_own_rows_and_label() {
        let html = render_html(&fixture());
        assert!(html.contains("achiral · noise 0"));
        assert!(html.contains("bm25 · baselines"));
        assert!(!html.contains("achiral · baselines"));
    }

    #[test]
    fn a_cohort_with_a_retrieval_setting_is_labelled_with_it_and_its_echo_is_listed() {
        let mut tuned = cohort(
            Some("noise 0"),
            "sha-a",
            vec![system("achiral", &[("scope", &BIG)])],
        );
        tuned.run.retrieval = Some(crate::results::RetrievalSetting {
            mode: Some("aggr".into()),
            params: Some(serde_json::json!({"activationWeightScale": 0.25})),
            echoed: Some(serde_json::json!({"mode": "aggr", "activationWeightScale": 0.25})),
        });
        let plain = cohort(
            Some("noise 0"),
            "sha-a",
            vec![system("achiral", &[("scope", &BIG)])],
        );
        // same label, different setting: two cohorts, never one
        assert!(validate_cohorts(&[tuned.clone(), plain.clone()]).is_ok());
        let html = render_html(&[tuned, plain]);
        assert!(
            html.contains("achiral · noise 0 [aggr · activationWeightScale=0.25]"),
            "series label"
        );
        assert!(
            html.contains("<th>Retrieval</th>") || html.contains("Retrieval"),
            "column"
        );
        assert!(html.contains("requested mode aggr"), "{html}");
        assert!(html.contains("server echoed"), "echo shown");
        assert!(
            html.contains("deployment defaults"),
            "the plain cohort says so"
        );
        let mut unchecked = cohort(
            Some("x"),
            "sha-a",
            vec![system("achiral", &[("scope", &BIG)])],
        );
        unchecked.run.retrieval = Some(crate::results::RetrievalSetting {
            mode: Some("dense".into()),
            params: None,
            echoed: None,
        });
        assert!(render_html(&[unchecked]).contains("server echo not recorded"));
    }

    #[test]
    fn a_tiny_category_is_labelled_not_plotted_in_the_page() {
        let html = render_html(&fixture());
        assert!(html.contains("n too small (n=4)"));
    }

    #[test]
    fn losses_to_a_baseline_are_counted_per_category() {
        // bm25 recalls scope questions 2 and 5? BIG fails at 2,5,9; BIG_B fails at 4.
        // Baseline wins where it passes and achiral fails: indices 2, 5, 9.
        let html = render_html(&fixture());
        assert!(html.contains("achiral · noise 0 against each baseline"));
        let row = html.split("<td>bm25 · baselines</td>").nth(1).unwrap();
        let cells: Vec<String> = row
            .split("</tr>")
            .next()
            .unwrap()
            .split("</td>")
            .filter(|c| c.contains('<'))
            .map(|c| c[c.rfind('>').unwrap() + 1..].to_string())
            .collect();
        // achiral only (index 4), baseline only (2,5,9), p, then scope losses 3, owner losses 0
        assert_eq!(cells[0], "▲ 1");
        assert_eq!(cells[1], "▼ 3");
        assert_eq!(cells[3], "▼ 3");
        assert_eq!(cells[4], "0");
        assert!(row.contains(r#"<td class="win">▲ 1</td><td class="loss">▼ 3</td>"#));
    }

    #[test]
    fn cohorts_on_different_datasets_are_not_compared() {
        let mut c = fixture();
        c[1].run.stream_sha256 = "sha-b".into();
        assert!(!render_html(&c).contains("against each baseline"));
    }

    #[test]
    fn empty_slots_say_what_is_missing_instead_of_drawing_nothing() {
        let html = render_html(&fixture());
        assert!(html.contains("No cycle run in these files."));
        assert!(
            html.contains("Needs two or more cohorts whose labels differ in one key=value setting")
        );
        let none = render_html(&[cohort(Some("x"), "a", vec![])]);
        assert!(none.contains("No recall results"));
        assert!(none.contains("No replicated run"));
        assert!(none.contains("No calibration results"));
        assert!(none.contains("No leak trials"));
        assert!(none.contains("No per-replicate summaries"));
    }

    #[test]
    fn cycles_flips_calibration_and_leaks_each_draw_their_chart() {
        let mut c = fixture();
        c[1].systems[0].tokens_by_cycle = vec![
            CyclePoint {
                cycle: 1,
                mean_tokens: 30.0,
            },
            CyclePoint {
                cycle: 10,
                mean_tokens: 41.5,
            },
        ];
        let html = render_html(&c);
        assert!(html.contains("Mean tokens injected per question by cycle"));
        assert!(html.contains("cycles (log scale)"));
        assert!(html.contains("Reliability diagram, achiral · noise 0"));
        assert!(html.contains("Brier 0.1800"));
        assert!(html.contains("Basis: derived"));
        assert!(html.contains("Flip rate by system"));
        assert!(html.contains("Leak rate by category and system"));
        assert!(html.contains("0/71 ≤0.042"));
        assert!(html.contains("0/24 ≤0.125"));
    }

    #[test]
    fn a_noise_sweep_and_a_mode_pair_are_charted_from_labels_alone() {
        let mk = |label: &str, recalls: &[f64]| {
            cohort(
                Some(label),
                "s",
                vec![system("achiral", &[("scope", recalls)])],
            )
        };
        let c = vec![
            mk("activationNoiseScale=0.2", &BIG_B),
            mk("activationNoiseScale=0", &BIG),
            mk("mode=aggr", &BIG),
            mk("mode=rerank", &BIG_B),
        ];
        let html = render_html(&c);
        assert!(html.contains("Achiral recall against activationNoiseScale"));
        assert!(html.contains("Overall recall against activationNoiseScale"));
        // x labels run left to right in numeric order whatever the file order
        let curve = html
            .rsplit("Overall recall against activationNoiseScale")
            .next()
            .unwrap();
        assert!(
            curve.find(r#"middle">0</text>"#).unwrap()
                < curve.find(r#"middle">0.2</text>"#).unwrap()
        );
        assert!(html.contains("Achiral recall against mode"));
        assert!(html.contains("mode=rerank"));
        assert!(!html.contains("Needs two or more cohorts"));
    }

    #[test]
    fn labels_and_names_are_escaped() {
        let c = vec![cohort(
            Some("<b>x</b>"),
            "a",
            vec![system("achiral", &[("scope", &BIG)])],
        )];
        let html = render_html(&c);
        assert!(!html.contains("<b>x</b>"));
        assert!(html.contains("&lt;b&gt;x&lt;/b&gt;"));
    }

    #[test]
    fn a_report_writes_the_page_json_and_csv_beside_each_other() {
        let dir = std::env::temp_dir().join(format!("cogn-report-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_report(&fixture(), &dir).unwrap();
        for f in ["index.html", "results.json", "results.csv"] {
            assert!(dir.join(f).metadata().unwrap().len() > 0, "{f}");
        }
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("results.json")).unwrap())
                .unwrap();
        assert_eq!(json["cohorts"].as_array().unwrap().len(), 2);
        let csv = std::fs::read_to_string(dir.join("results.csv")).unwrap();
        assert!(csv
            .lines()
            .next()
            .unwrap()
            .starts_with("cohort,system,section"));
        assert!(csv.contains("noise 0,achiral,category_recall,scope,12"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_report_that_would_leak_a_key_is_not_written() {
        let dir = std::env::temp_dir().join(format!("cogn-report-dirty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dirty = vec![cohort(Some("acm_secret123"), "a", vec![])];
        assert!(write_report(&dirty, &dir).is_err());
        assert!(!dir.join("index.html").exists());
    }

    #[test]
    fn run_report_defaults_to_a_report_folder_beside_the_results() {
        let dir = std::env::temp_dir().join(format!("cogn-report-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        fixture()[0].save(&dir.join("a.json")).unwrap();
        let args =
            ReportArgs::parse_from(["report", "--results", dir.join("a.json").to_str().unwrap()]);
        let index = run_report(&args).unwrap();
        assert_eq!(index, dir.join("report").join("index.html"));
        assert!(index.exists());
        let args = ReportArgs::parse_from([
            "report",
            "--results",
            dir.join("a.json").to_str().unwrap(),
            "--out",
            dir.join("elsewhere").to_str().unwrap(),
        ]);
        assert!(run_report(&args)
            .unwrap()
            .starts_with(dir.join("elsewhere")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_reliability_diagram_has_a_diagonal_and_one_marker_per_bin() {
        let svg = reliability_svg(
            "x",
            "c0",
            &[
                BinRow {
                    predicted_mean: 0.2,
                    observed_frequency: 0.1,
                    count: 3,
                },
                BinRow {
                    predicted_mean: 0.9,
                    observed_frequency: 0.8,
                    count: 4,
                },
            ],
        );
        assert_eq!(svg.matches("<circle").count(), 2);
        assert!(svg.contains("diag dash"));
        assert!(svg.contains("predicted probability") && svg.contains("observed frequency"));
    }

    #[test]
    fn the_tokens_chart_starts_at_zero() {
        let svg = cycles_svg(&[CycleSeries {
            name: "a".into(),
            class: "c0".into(),
            shape: 0,
            strong: true,
            pts: vec![(1, 30.0), (10, 31.0)],
        }]);
        assert!(svg.contains(r#"text-anchor="end">0</text>"#));
        assert_eq!(
            svg.matches("<circle").count()
                + svg.matches("<polygon").count()
                + svg.matches("<rect").count(),
            2
        );
    }

    #[test]
    fn end_labels_of_lines_that_finish_together_do_not_overlap() {
        let same = |name: &str| CycleSeries {
            name: name.into(),
            class: "b0".into(),
            shape: 0,
            strong: false,
            pts: vec![(10, 30.0), (100, 30.0)],
        };
        let svg = cycles_svg(&[same("a"), same("b"), same("c")]);
        let ys: Vec<f64> = svg
            .split(r#"class="note" x=""#)
            .skip(1)
            .map(|t| {
                let y = t.split(r#"y=""#).nth(1).unwrap();
                y.split('"').next().unwrap().parse().unwrap()
            })
            .collect();
        assert_eq!(ys.len(), 3);
        let mut sorted = ys.clone();
        sorted.sort_by(f64::total_cmp);
        assert!(sorted.windows(2).all(|w| w[1] - w[0] >= 14.0), "{ys:?}");
    }

    #[test]
    fn the_setting_curve_labels_each_value_on_its_axis() {
        let pts = vec![
            (0.0, Stat::from_values(&BIG, 0), "c0".to_string()),
            (0.2, Stat::from_values(&BIG_B, 0), "c1".to_string()),
        ];
        let svg = curve_svg("activationNoiseScale", &pts);
        assert!(svg.contains(">0.2</text>") && svg.contains(">0</text>"));
        assert_eq!(svg.matches("<circle").count(), 2);
    }

    // ----- colour carries meaning ------------------------------------------

    fn rgb(hex: &str) -> [f64; 3] {
        let h = hex.trim_start_matches('#');
        [0, 2, 4].map(|i| f64::from(u8::from_str_radix(&h[i..i + 2], 16).unwrap()))
    }

    fn lin(c: f64) -> f64 {
        let c = c / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(hex: &str) -> f64 {
        let [r, g, b] = rgb(hex);
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    }

    fn contrast(a: &str, b: &str) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    fn token<'a>(theme: &'a [(&'a str, &'a str)], name: &str) -> &'a str {
        theme.iter().find(|(k, _)| *k == name).unwrap().1
    }

    #[test]
    fn every_graphic_colour_keeps_three_to_one_against_the_card_in_both_themes() {
        for (theme_name, theme) in [("light", &LIGHT[..]), ("dark", &DARK[..])] {
            let card = token(theme, "card");
            for name in [
                "c0", "c1", "c2", "c3", "b0", "b1", "b2", "b3", "b4", "win", "loss", "alarm",
                "even", "hatch",
            ] {
                let ratio = contrast(token(theme, name), card);
                assert!(ratio >= 3.0, "{theme_name} {name} is {ratio:.2}:1");
            }
            for name in ["fg", "fg2", "muted"] {
                for ground in ["card", "bg"] {
                    let ratio = contrast(token(theme, name), token(theme, ground));
                    assert!(
                        ratio >= 4.5,
                        "{theme_name} {name} on {ground} is {ratio:.2}:1"
                    );
                }
            }
        }
    }

    /// Simulates protan, deutan and tritan vision (Viénot, Brettel and Mollon
    /// 1999) and reports how far apart two colours still look.
    fn separation(a: &str, b: &str) -> f64 {
        let matrices: [[[f64; 3]; 3]; 4] = [
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [
                [0.152286, 1.052583, -0.204868],
                [0.114503, 0.786281, 0.099216],
                [-0.003882, -0.048116, 1.051998],
            ],
            [
                [0.367322, 0.860646, -0.227968],
                [0.280085, 0.672501, 0.047413],
                [-0.011820, 0.042940, 0.968881],
            ],
            [
                [1.255528, -0.076749, -0.178779],
                [-0.078411, 0.930809, 0.147602],
                [0.004733, 0.691367, 0.303900],
            ],
        ];
        let gamma = |c: f64| {
            255.0
                * if c <= 0.0031308 {
                    12.92 * c
                } else {
                    1.055 * c.powf(1.0 / 2.4) - 0.055
                }
        };
        let view = |hex: &str, m: &[[f64; 3]; 3]| -> [f64; 3] {
            let v = rgb(hex).map(lin);
            [0, 1, 2]
                .map(|i| gamma((m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2]).clamp(0.0, 1.0)))
        };
        matrices
            .iter()
            .map(|m| {
                let (p, q) = (view(a, m), view(b, m));
                ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
            })
            .fold(f64::MAX, f64::min)
    }

    #[test]
    fn cohort_hues_stay_apart_under_colour_vision_deficiency_in_both_themes() {
        for (theme_name, theme) in [("light", &LIGHT[..]), ("dark", &DARK[..])] {
            let hues = ["c0", "c1", "c2", "c3"];
            for (i, a) in hues.iter().enumerate() {
                for b in &hues[i + 1..] {
                    let d = separation(token(theme, a), token(theme, b));
                    assert!(d >= 28.0, "{theme_name} {a} vs {b} only {d:.0} apart");
                }
            }
            let d = separation(token(theme, "win"), token(theme, "loss"));
            assert!(d >= 40.0, "{theme_name} ahead vs behind only {d:.0} apart");
            let d = separation(token(theme, "win"), token(theme, "alarm"));
            assert!(d >= 25.0, "{theme_name} ahead vs leak only {d:.0} apart");
        }
    }

    #[test]
    fn both_themes_define_the_same_tokens_and_the_page_ships_both() {
        let names = |t: &[(&str, &str)]| t.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>();
        assert_eq!(names(&LIGHT), names(&DARK));
        let html = render_html(&fixture());
        assert!(html.contains("--c0:#0072b2") && html.contains("--c0:#56b4e9"));
        assert!(html.contains("prefers-color-scheme:dark"));
        assert!(!html.contains("grayscale"));
    }

    #[test]
    fn achiral_cohorts_each_get_their_own_saturated_hue_and_baselines_a_quieter_one() {
        let c = vec![
            cohort(
                Some("noise 0"),
                "s",
                vec![system("achiral", &[("scope", &BIG)])],
            ),
            cohort(
                Some("noise 0.2"),
                "s",
                vec![system("achiral", &[("scope", &BIG_B)])],
            ),
            cohort(
                Some("baselines"),
                "s",
                vec![
                    system("bm25", &[("scope", &BIG)]),
                    system("verbatim", &[("scope", &BIG)]),
                ],
            ),
        ];
        let series = all_series(&c);
        let classes: Vec<&str> = series.iter().map(|s| s.class.as_str()).collect();
        assert_eq!(classes, vec!["c0", "c1", "b1", "b0"]);
        let shapes: Vec<usize> = series.iter().map(|s| s.shape).collect();
        let mut unique = shapes.clone();
        unique.dedup();
        assert_eq!(unique.len(), shapes.len(), "every series has its own shape");
    }

    #[test]
    fn a_cohort_keeps_one_hue_in_every_chart() {
        let html = render_html(&fixture());
        // the Achiral cohort is c0 everywhere it appears
        let achiral_groups = html.matches(r#"<g class="s c0">"#).count();
        assert!(
            achiral_groups >= 6,
            "recall, gap, flips, leaks, calibration, ...: {achiral_groups}"
        );
        assert!(
            !html.contains(r#"<g class="s c1">"#),
            "no second Achiral cohort here"
        );
        assert!(
            html.contains(r#"<g class="s b1">"#),
            "bm25 keeps its baseline hue"
        );
    }

    #[test]
    fn the_series_with_the_most_weight_is_drawn_larger_than_a_baseline() {
        let html = render_html(&fixture());
        assert!(html.contains(r#"class="pt big""#));
        assert!(html.contains(r#"<li><svg viewBox="0 0 16 16""#), "a legend");
    }

    fn diff(values: &[f64]) -> Stat {
        Stat::from_values(values, 0)
    }

    #[test]
    fn an_outcome_is_ahead_or_behind_only_when_the_ci_clears_zero() {
        let up: Vec<f64> = (0..20).map(|i| 0.3 + f64::from(i % 3) * 0.01).collect();
        let down: Vec<f64> = up.iter().map(|v| -v).collect();
        let mixed: Vec<f64> = (0..20)
            .map(|i| if i % 2 == 0 { 0.4 } else { -0.4 })
            .collect();
        assert_eq!(outcome(&diff(&up)), Outcome::Ahead);
        assert_eq!(outcome(&diff(&down)), Outcome::Behind);
        assert_eq!(outcome(&diff(&mixed)), Outcome::Even);
        assert_eq!(outcome(&diff(&[0.5, 0.5])), Outcome::TooSmall);
        assert_eq!(
            outcome(&diff(&[0.5; 12])),
            Outcome::Ahead,
            "no variance: the sign decides"
        );
        assert_eq!(outcome(&diff(&[-0.5; 12])), Outcome::Behind);
        assert_eq!(outcome(&diff(&[0.0; 12])), Outcome::Even);
    }

    #[test]
    fn a_loss_is_drawn_as_large_and_as_labelled_as_a_win() {
        let up: Vec<f64> = (0..20).map(|i| 0.3 + f64::from(i % 3) * 0.01).collect();
        let down: Vec<f64> = up.iter().map(|v| -v).collect();
        let rows = |v: &[f64]| {
            vec![DiffRow {
                label: "bm25 · baselines · n=20".into(),
                header: false,
                stat: Some(diff(v)),
            }]
        };
        let win = diff_plot("t", &rows(&up), 0.5);
        let loss = diff_plot("t", &rows(&down), 0.5);
        assert!(win.contains(r#"<g class="s win">"#) && win.contains("▲ ahead +0.3"));
        assert!(loss.contains(r#"<g class="s loss">"#) && loss.contains("▼ behind −0.3"));
        assert_eq!(win.matches("pt big").count(), 1);
        assert_eq!(
            win.matches(r#"class="whisker""#).count(),
            1,
            "the CI is drawn"
        );
        assert_eq!(loss.matches(r#"class="whisker""#).count(), 1);
        assert_eq!(loss.matches("pt big").count(), 1, "same mark size");
        assert!(loss.contains("← Achiral behind") && loss.contains("Achiral ahead →"));
        assert!(loss.contains(r#"class="zero""#));
        let flat = diff_plot(
            "t",
            &rows(&[0.4, -0.4, 0.4, -0.4, 0.4, -0.4, 0.4, -0.4, 0.4, -0.4]),
            0.5,
        );
        assert!(flat.contains("● no clear gap") && flat.contains(r#"<g class="s even">"#));
    }

    #[test]
    fn a_tiny_difference_row_is_hatched_not_plotted() {
        let rows = vec![DiffRow {
            label: "x".into(),
            header: false,
            stat: Some(diff(&[0.5, 0.5])),
        }];
        let svg = diff_plot("t", &rows, 0.5);
        assert!(svg.contains(r#"class="hatch""#) && svg.contains("n too small (n=2)"));
        assert!(!svg.contains("pt big"));
    }

    #[test]
    fn the_page_shows_where_achiral_is_behind_and_ahead_of_a_baseline() {
        let html = render_html(&fixture());
        assert!(html.contains("achiral · noise 0 against each baseline"));
        // fixture: bm25 wins 3 scope questions, Achiral wins 1; paired over 12 shared
        assert!(html.contains(r#"<td class="loss">▼ 3</td>"#));
        assert!(html.contains(r#"<td class="win">▲ 1</td>"#));
        assert!(html.contains("Recall difference, achiral · noise 0"));
    }

    #[test]
    fn a_leak_above_zero_is_the_only_alarm_and_is_labelled() {
        let mut c = fixture();
        c[1].systems[0].leaks = vec![crate::results::NamedStat {
            name: "scope".into(),
            stat: Stat::from_events(&(0..71).map(|i| i < 10).collect::<Vec<_>>(), 0),
        }];
        let html = render_html(&c);
        assert!(html.contains(r#"<g class="s alarm">"#));
        assert!(html.contains("LEAK · bm25 · baselines"));
        assert!(html.contains(r#"class="headline alarm""#));
        assert!(html.contains("Leak: 1 of 2 series returned another scope's memory."));
        assert!(html.contains("Highest rate: bm25 · baselines, scope: 10 of 71 trials (14.1%)"));
        // no leak anywhere: no alarm at all
        let calm = render_html(&fixture());
        assert!(!calm.contains(r#"<g class="s alarm">"#));
        assert!(!calm.contains("LEAK ·"));
        assert!(calm.contains("No leak in any trial run"));
    }

    #[test]
    fn each_chart_leads_with_a_one_line_result_with_its_n() {
        let mut c = fixture();
        c[1].systems[0].tokens_by_cycle = vec![CyclePoint {
            cycle: 10,
            mean_tokens: 41.5,
        }];
        let html = render_html(&c);
        let headlines: Vec<&str> = html
            .split(r#"<p class="headline"#)
            .skip(1)
            .map(|h| h.split("</p>").next().unwrap())
            .collect();
        assert!(headlines.len() >= 5, "{}", headlines.len());
        for h in &headlines {
            assert!(h.contains("n=") || h.contains(" of "), "no n in: {h}");
        }
        let recall = headlines
            .iter()
            .find(|h| h.contains("Best local baseline"))
            .unwrap();
        assert!(recall.contains("achiral · noise 0 overall recall 0.750"));
        assert!(recall.contains("Achiral minus bm25: −"), "{recall}");
        assert!(recall.contains("95% CI"));
        assert!(headlines
            .iter()
            .any(|h| h.contains("0 of 24 questions changed outcome across 3 replicates")));
        assert!(headlines
            .iter()
            .any(|h| h.contains("41.5 tokens per question at cycle 10 (n=24 questions)")));
        assert!(headlines.iter().any(|h| h.contains("Brier 0.180")));
    }

    #[test]
    fn the_recall_headline_says_so_when_the_gap_is_not_clear() {
        let line = headline_recall(&all_series(&fixture()).iter().collect::<Vec<_>>());
        assert_eq!(line.len(), 1);
        assert!(
            line[0].1.contains("Achiral minus bm25: −0.125"),
            "{}",
            line[0].1
        );
        assert!(
            line[0].1.ends_with("so no clear difference."),
            "{}",
            line[0].1
        );
        assert_eq!(line[0].0, "", "no outcome colour without a clear gap");
    }

    #[test]
    fn the_recall_headline_names_a_clear_loss_and_a_clear_win() {
        let behind = vec![
            cohort(
                Some("a"),
                "s",
                vec![system("achiral", &[("scope", &[0.0; 12])])],
            ),
            cohort(
                Some("b"),
                "s",
                vec![system("bm25", &[("scope", &[1.0; 12])])],
            ),
        ];
        let line = headline_recall(&all_series(&behind).iter().collect::<Vec<_>>());
        assert_eq!(line[0].0, "loss");
        assert!(
            line[0].1.contains("Achiral minus bm25: −1.000"),
            "{}",
            line[0].1
        );
        assert!(
            line[0].1.ends_with("so Achiral is behind."),
            "{}",
            line[0].1
        );
        let ahead = vec![
            cohort(
                Some("a"),
                "s",
                vec![system("achiral", &[("scope", &[1.0; 12])])],
            ),
            cohort(
                Some("b"),
                "s",
                vec![system("bm25", &[("scope", &[0.0; 12])])],
            ),
        ];
        let line = headline_recall(&all_series(&ahead).iter().collect::<Vec<_>>());
        assert_eq!(line[0].0, "win");
        assert!(line[0].1.ends_with("so Achiral is ahead."), "{}", line[0].1);
    }

    #[test]
    fn the_wording_makes_no_claim_the_numbers_do_not_carry() {
        let html = render_html(&fixture()).to_lowercase();
        for banned in [
            "bayesian",
            "graph retrieval",
            "graph-based",
            "deterministic memory",
            "better than",
            "outperforms",
        ] {
            assert!(!html.contains(banned), "{banned}");
        }
        assert!(html.contains("non-deterministic"));
    }

    #[test]
    fn a_noise_curve_colours_each_point_with_its_own_cohort() {
        let mk = |label: &str, recalls: &[f64]| {
            cohort(
                Some(label),
                "s",
                vec![system("achiral", &[("scope", recalls)])],
            )
        };
        let html = render_html(&[
            mk("activationNoiseScale=0", &BIG),
            mk("activationNoiseScale=0.2", &BIG_B),
        ]);
        let curve = html
            .rsplit("Overall recall against activationNoiseScale")
            .next()
            .unwrap();
        assert!(curve.contains(r#"<g class="s c0">"#) && curve.contains(r#"<g class="s c1">"#));
    }

    #[test]
    fn confidence_lines_carry_a_halo_so_they_read_on_top_of_any_colour() {
        let svg = dot_plot("t", &[row("a", Some(Stat::from_values(&BIG, 0)))], 1.0);
        let halos = svg.matches(r#"class="halo""#).count();
        let whiskers = svg.matches(r#"class="whisker""#).count();
        assert_eq!(
            (halos, whiskers),
            (3, 3),
            "a line and two caps, each with a halo"
        );
        assert!(css().contains(".halo{stroke:var(--card)"));
    }

    #[test]
    fn every_colour_class_the_charts_use_has_a_rule() {
        let css = css();
        for class in [
            "c0", "c1", "c2", "c3", "b0", "b1", "b2", "b3", "b4", "win", "loss", "alarm", "even",
        ] {
            assert!(
                css.contains(&format!(".s.{class}{{--sc:var(--{class})}}")),
                "{class}"
            );
        }
    }

    #[test]
    fn a_headline_names_at_most_three_series_and_counts_the_rest() {
        let systems = [
            "verbatim",
            "bm25",
            "recency",
            "full-context",
            "bounded-window",
        ];
        let mut file = cohort(
            Some("many"),
            "s",
            systems
                .iter()
                .map(|n| system(n, &[("scope", &BIG)]))
                .collect(),
        );
        for (i, s) in file.systems.iter_mut().enumerate() {
            s.tokens_by_cycle = vec![CyclePoint {
                cycle: 10,
                mean_tokens: 30.0 + i as f64,
            }];
        }
        let c = vec![file];
        let series = all_series(&c);
        let refs: Vec<&Series<'_>> = series.iter().collect();
        let line = headline_cycles(&refs);
        assert!(line.contains("and 2 more in the chart"), "{line}");
        assert_eq!(line.matches("tokens per question").count(), 3);
    }

    #[test]
    fn identical_flip_results_in_one_cohort_share_a_sentence() {
        let mut file = cohort(
            Some("baselines"),
            "s",
            vec![
                system("bm25", &[("scope", &BIG)]),
                system("verbatim", &[("scope", &BIG)]),
            ],
        );
        for s in &mut file.systems {
            s.flips = Some(Stat::from_events(&[false; 24], 0));
        }
        let c = vec![file];
        let series = all_series(&c);
        let refs: Vec<&Series<'_>> = series.iter().collect();
        let line = headline_flips(&refs);
        assert_eq!(line.matches("changed outcome").count(), 1, "{line}");
        assert!(
            line.starts_with("bm25, verbatim · baselines: 0 of 24"),
            "{line}"
        );
    }

    #[test]
    fn the_legend_draws_achiral_large_and_each_baseline_small() {
        let html = render_html(&fixture());
        let legend = html
            .split(r#"<ul class="legend">"#)
            .nth(1)
            .unwrap()
            .split("</ul>")
            .next()
            .unwrap();
        assert_eq!(legend.matches("pt big").count(), 1, "only Achiral is large");
        assert_eq!(legend.matches("<li>").count(), 2);
    }
}

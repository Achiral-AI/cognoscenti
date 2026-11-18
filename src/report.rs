//! `cognoscenti report`: one self-contained HTML page per run, drawn from
//! results files (`--results-json`). Inline SVG only, grayscale, no script,
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

struct Series<'a> {
    cohort: &'a RunFile,
    sys: &'a SystemResults,
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
    cohorts
        .iter()
        .flat_map(|cohort| cohort.systems.iter().map(move |sys| Series { cohort, sys }))
        .collect()
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
    pub header: bool,
}

fn xpos(v: f64, max: f64) -> f64 {
    PX0 + (PX1 - PX0) * (v / max).clamp(0.0, 1.0)
}

fn marker(shape: usize, x: f64, y: f64, strong: bool) -> String {
    let class = if strong { "pt" } else { "pt hollow" };
    match shape % 4 {
        0 => format!(r#"<circle class="{class}" cx="{x:.1}" cy="{y:.1}" r="4.5"/>"#),
        1 => format!(
            r#"<rect class="{class}" x="{:.1}" y="{:.1}" width="9" height="9"/>"#,
            x - 4.5,
            y - 4.5
        ),
        2 => format!(
            r#"<polygon class="{class}" points="{x:.1},{:.1} {:.1},{y:.1} {x:.1},{:.1} {:.1},{y:.1}"/>"#,
            y - 5.5,
            x + 5.5,
            y + 5.5,
            x - 5.5
        ),
        _ => format!(
            r#"<polygon class="{class}" points="{x:.1},{:.1} {:.1},{:.1} {:.1},{:.1}"/>"#,
            y - 5.0,
            x + 5.0,
            y + 4.0,
            x - 5.0,
            y + 4.0
        ),
    }
}

fn tick(v: f64) -> String {
    tick_x(v)
}

/// Rows of point estimates with 95% CI whiskers on an axis from zero.
/// A stat with no event and a 3/n bound draws a dashed bar to the bound; a
/// stat under ten questions draws no point and says so.
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
            r#"<text class="lbl{weight}" x="{PX0}" y="{ly:.1}">{}</text>"#,
            esc(&row.label)
        );
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
                    r#"<text class="note" x="{PX0}" y="{:.1}">n too small (n={})</text>"#,
                    py - 4.0,
                    stat.n
                );
            }
            Some(stat) => {
                if let Some(bound) = stat.upper_bound {
                    let bx = xpos(bound, axis_max);
                    let _ = write!(
                        body,
                        r#"<line class="whisker dash" x1="{:.1}" y1="{py:.1}" x2="{bx:.1}" y2="{py:.1}"/><line class="whisker" x1="{bx:.1}" y1="{:.1}" x2="{bx:.1}" y2="{:.1}"/>"#,
                        xpos(0.0, axis_max),
                        py - 5.0,
                        py + 5.0
                    );
                } else if let Some([lo, hi]) = stat.ci {
                    let (x0, x1) = (xpos(lo, axis_max), xpos(hi, axis_max));
                    let _ = write!(
                        body,
                        r#"<line class="whisker" x1="{x0:.1}" y1="{py:.1}" x2="{x1:.1}" y2="{py:.1}"/><line class="whisker" x1="{x0:.1}" y1="{:.1}" x2="{x0:.1}" y2="{:.1}"/><line class="whisker" x1="{x1:.1}" y1="{:.1}" x2="{x1:.1}" y2="{:.1}"/>"#,
                        py - 5.0,
                        py + 5.0,
                        py - 5.0,
                        py + 5.0
                    );
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

/// Predicted probability against observed frequency per bin, with the
/// diagonal of perfect calibration.
pub fn reliability_svg(name: &str, bins: &[crate::results::BinRow]) -> String {
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
        r#"<line class="whisker dash" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"/>"#,
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
        r#"<polyline class="line" points="{}"/>"#,
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

/// Mean tokens injected per question at each cycle checkpoint; x is log
/// scale, y starts at zero.
pub fn cycles_svg(series: &[(String, Vec<(usize, f64)>)]) -> String {
    let (h, left, right, top, bottom) = (260.0, 44.0, 380.0, 12.0, 44.0);
    let xs: Vec<f64> = series
        .iter()
        .flat_map(|(_, pts)| pts.iter().map(|(c, _)| (*c as f64).max(1.0).ln()))
        .collect();
    let (xmin, xmax) = (
        xs.iter().cloned().fold(f64::MAX, f64::min),
        xs.iter().cloned().fold(f64::MIN, f64::max),
    );
    let ymax = series
        .iter()
        .flat_map(|(_, pts)| pts.iter().map(|(_, t)| *t))
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
        .flat_map(|(_, p)| p.iter().map(|(c, _)| *c))
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
    for (i, (name, pts)) in series.iter().enumerate() {
        let line: Vec<String> = pts
            .iter()
            .map(|(c, t)| format!("{:.1},{:.1}", px(*c), py(*t)))
            .collect();
        let _ = write!(
            body,
            r#"<polyline class="line" points="{}"/>"#,
            line.join(" ")
        );
        for (c, t) in pts {
            body.push_str(&marker(i, px(*c), py(*t), i == 0));
        }
        if let Some((c, t)) = pts.last() {
            let _ = write!(
                body,
                r#"<text class="note" x="{:.1}" y="{:.1}">{}</text>"#,
                px(*c) + 8.0,
                py(*t) + 4.0,
                esc(name)
            );
        }
    }
    format!(
        r#"<svg viewBox="0 0 {W} {h}" role="img" aria-label="Mean tokens injected per question by cycle"><title>Mean tokens injected per question by cycle</title>{body}</svg>"#
    )
}

/// A numeric setting on x, overall recall (with CI) on y from zero.
pub fn curve_svg(key: &str, points: &[(f64, Stat)]) -> String {
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
        .map(|(x, s)| format!("{:.1},{:.1}", px(*x), py(s.estimate)))
        .collect();
    let _ = write!(
        body,
        r#"<polyline class="line" points="{}"/>"#,
        line.join(" ")
    );
    for (x, s) in points {
        if let Some([lo, hi]) = s.ci {
            let _ = write!(
                body,
                r#"<line class="whisker" x1="{0:.1}" y1="{1:.1}" x2="{0:.1}" y2="{2:.1}"/>"#,
                px(*x),
                py(lo),
                py(hi)
            );
        }
        body.push_str(&marker(0, px(*x), py(s.estimate), true));
        let _ = write!(
            body,
            r#"<text class="tick" x="{:.1}" y="{:.1}" text-anchor="middle">{}</text>"#,
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

const CSS: &str = r#"
:root{--bg:#f9fafb;--fg:#101828;--fg2:#364153;--muted:#6a7282;--line:#d1d5dc;--card:#fff}
@media (prefers-color-scheme:dark){:root{--bg:#020618;--fg:#f3f4f6;--fg2:#d1d5dc;--muted:#99a1af;--line:#1e2939;--card:#0f172a}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:860px;margin:0 auto;padding:24px 16px 64px}
h1{font-size:22px;margin:0 0 4px}h2{font-size:17px;margin:36px 0 8px}h3{font-size:15px;margin:20px 0 6px}
p,li{color:var(--fg2)}.muted{color:var(--muted);font-size:13px}
section{border-top:1px solid var(--line);margin-top:8px}
.card{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:12px;margin:10px 0;overflow-x:auto}
svg{display:block;width:100%;height:auto;max-width:520px}svg.sq{max-width:260px}
svg text{fill:var(--fg);font-size:13px}svg .note,svg .tick{fill:var(--muted);font-size:12px}svg .grp{font-weight:600}svg .strong{font-weight:700}
svg .rail{stroke:var(--line);stroke-width:1}svg .grid{stroke:var(--line);stroke-width:1}
svg .whisker{stroke:var(--fg);stroke-width:1.5}svg .dash{stroke-dasharray:4 3}
svg .line{fill:none;stroke:var(--muted);stroke-width:1.5}
svg .pt{fill:var(--fg);stroke:var(--fg);stroke-width:1.5}svg .hollow{fill:var(--card)}
table{border-collapse:collapse;font-size:13px;width:100%}th,td{border-bottom:1px solid var(--line);padding:4px 8px;text-align:left;vertical-align:top}
th{color:var(--muted);font-weight:600}code{font-size:12px;word-break:break-all}
.grid2{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:12px}
"#;

fn series_rows(
    series: &[Series<'_>],
    pick: impl Fn(&SystemResults) -> Option<Stat>,
) -> Vec<PlotRow> {
    series
        .iter()
        .enumerate()
        .map(|(i, s)| PlotRow {
            label: s.name(),
            stat: pick(s.sys),
            strong: s.is_achiral(),
            marker: i,
            header: false,
        })
        .collect()
}

fn table_open(headers: &[&str]) -> String {
    let mut out = String::from(r#"<div class="card"><table><thead><tr>"#);
    for h in headers {
        let _ = write!(out, "<th>{}</th>", esc(h));
    }
    out.push_str("</tr></thead><tbody>");
    out
}

pub fn render_html(cohorts: &[RunFile]) -> String {
    let series = all_series(cohorts);
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light dark"><title>Cognoscenti trust report</title><style>{CSS}</style></head><body><main><h1>Cognoscenti trust report</h1><p class="muted">Every rate shows its n and a 95% bootstrap interval over questions. Each cohort is a separate run, named by its label, and is never merged with another.</p>"#
    );
    h.push_str(&manifest_section(cohorts));
    h.push_str(&recall_section(&series, cohorts));
    h.push_str(&flip_section(&series));
    h.push_str(&calibration_section(&series));
    h.push_str(&cycles_section(&series));
    h.push_str(&leak_section(&series));
    h.push_str(&settings_section(cohorts));
    h.push_str(&replicates_section(&series));
    h.push_str("</main></body></html>");
    h
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
            "<tr><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
            esc(&c.cohort_name()),
            esc(&c.run.stream_file),
            esc(&c.run.stream_sha256),
            c.run.seed,
            c.run.top_k,
            c.run.replicates,
            c.run.questions,
            esc(c.run.cognoscenti_commit.as_deref().map_or("unknown", |s| &s[..s.len().min(12)])),
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

fn recall_section(series: &[Series<'_>], cohorts: &[RunFile]) -> String {
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
    let mut rows = vec![PlotRow {
        label: "All questions".into(),
        stat: None,
        strong: false,
        marker: 0,
        header: true,
    }];
    for (i, s) in with_recall.iter().enumerate() {
        rows.push(PlotRow {
            label: s.name(),
            stat: s.sys.recall.clone(),
            strong: s.is_achiral(),
            marker: i,
            header: false,
        });
    }
    for category in &categories {
        rows.push(PlotRow {
            label: format!("Category: {category}"),
            stat: None,
            strong: false,
            marker: 0,
            header: true,
        });
        for (i, s) in with_recall.iter().enumerate() {
            rows.push(PlotRow {
                label: s.name(),
                stat: s
                    .sys
                    .categories
                    .iter()
                    .find(|c| &c.name == category)
                    .map(|c| c.stat.clone()),
                strong: s.is_achiral(),
                marker: i,
                header: false,
            });
        }
    }
    let _ = write!(
        h,
        r#"<p class="muted">Recall@k per question, 0 to 1. Filled marks are Achiral; hollow marks are the local baselines. A category under 10 questions shows "n too small".</p><div class="card">{}</div>"#,
        dot_plot("Recall by category and system", &rows, 1.0)
    );
    h.push_str(&head_to_head(&with_recall, &categories, cohorts));
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
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{:.4}</td>",
                esc(&b.name()),
                m.only_a,
                m.only_b,
                m.p_value
            );
            for c in categories {
                let _ = write!(
                    rows,
                    "<td>{}</td>",
                    losses.get(c.as_str()).copied().unwrap_or(0)
                );
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
    let with: Vec<Series<'_>> = series
        .iter()
        .filter(|s| s.sys.flips.is_some())
        .map(|s| Series {
            cohort: s.cohort,
            sys: s.sys,
        })
        .collect();
    let mut h = String::from(r#"<section id="flips"><h2>Flip rate across replicates</h2>"#);
    if with.is_empty() {
        h.push_str(r#"<p class="muted">No replicated run in these files.</p></section>"#);
        return h;
    }
    let max = axis_max(with.iter().filter_map(|s| s.sys.flips.as_ref()));
    let rows = series_rows(&with, |s| s.flips.clone());
    let _ = write!(
        h,
        r#"<p class="muted">Share of questions whose pass or fail changed between replicates, each run in its own fresh org. With no flip, the bar shows the 3/n upper bound.</p><div class="card">{}</div></section>"#,
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
    h.push_str(r#"<p class="muted">Of memories retrieved at a given probability, how often one was needed. The dashed diagonal is perfect calibration. Probabilities are derived from each system's ranking.</p><div class="grid2">"#);
    for s in with {
        let cal = s.sys.calibration.as_ref().unwrap();
        let _ = write!(
            h,
            r#"<div class="card"><h3>{}</h3>{}<p class="muted">Brier {:.4}. ECE {}. Basis: {}.</p></div>"#,
            esc(&s.name()),
            reliability_svg(&s.name(), &cal.bins),
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
    let data: Vec<(String, Vec<(usize, f64)>)> = series
        .iter()
        .filter(|s| !s.sys.tokens_by_cycle.is_empty())
        .map(|s| {
            (
                s.name(),
                s.sys
                    .tokens_by_cycle
                    .iter()
                    .map(|p| (p.cycle, p.mean_tokens))
                    .collect(),
            )
        })
        .collect();
    if data.is_empty() {
        h.push_str(r#"<p class="muted">No cycle run in these files.</p></section>"#);
        return h;
    }
    let _ = write!(
        h,
        r#"<p class="muted">Mean tokens a question would inject as history grows. Read path only; whitespace-separated words.</p><div class="card">{}</div></section>"#,
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
    for name in &names {
        rows.push(PlotRow {
            label: format!("Category: {name}"),
            stat: None,
            strong: false,
            marker: 0,
            header: true,
        });
        for (i, s) in series
            .iter()
            .enumerate()
            .filter(|(_, s)| s.sys.leaks.iter().any(|l| &l.name == name))
        {
            rows.push(PlotRow {
                label: s.name(),
                stat: s
                    .sys
                    .leaks
                    .iter()
                    .find(|l| &l.name == name)
                    .map(|l| l.stat.clone()),
                strong: s.is_achiral(),
                marker: i,
                header: false,
            });
        }
    }
    let _ = write!(
        h,
        r#"<p class="muted">Share of trials where a memory its asker may not see came back. With none, the dashed bar is the 3/n upper bound: with n trials and no leak, the true rate is at most 3/n with 95% confidence.</p><div class="card">{}</div></section>"#,
        dot_plot("Leak rate by category and system", &rows, max)
    );
    h
}

fn settings_section(cohorts: &[RunFile]) -> String {
    let mut h = String::from(r#"<section id="settings"><h2>Metric against a setting</h2>"#);
    let groups = setting_groups(cohorts);
    let mut drew = false;
    for g in groups {
        let pts: Vec<(String, Stat)> = g
            .members
            .iter()
            .filter_map(|(v, c)| {
                let s = c.systems.iter().find(|s| s.system == "achiral")?;
                Some((v.clone(), s.recall.clone()?))
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
        let numeric: Option<Vec<(f64, Stat)>> = pts
            .iter()
            .map(|(v, s)| v.parse::<f64>().ok().map(|x| (x, s.clone())))
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
                    .map(|(i, (v, s))| PlotRow {
                        label: format!("{}={v}", g.key),
                        stat: Some(s.clone()),
                        strong: true,
                        marker: i,
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
        assert_eq!(svg.matches(r#"class="pt hollow""#).count(), 1);
        assert!(svg.contains("0.750 n=12"));
    }

    #[test]
    fn a_category_too_small_for_a_ci_draws_no_point() {
        let svg = dot_plot("t", &[row("owner", Some(Stat::from_values(&TINY, 0)))], 1.0);
        assert!(svg.contains("n too small (n=4)"));
        assert!(
            !svg.contains(r#"class="pt"#),
            "no marker for a tiny category"
        );
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
        for forbidden in ["<script", "http", "<link", "@import", "src=", "url("] {
            assert!(!html.contains(forbidden), "{forbidden}");
        }
        assert!(ensure_clean(&html).is_ok());
        // Achromatic palette of STANDARDS.md (its grays and slates carry a
        // faint blue tint): no channel strays far from the others.
        let mut colours = 0;
        for hex in html
            .match_indices('#')
            .filter_map(|(i, _)| html.get(i + 1..i + 7))
        {
            if hex.chars().all(|c| c.is_ascii_hexdigit()) {
                colours += 1;
                let ch: Vec<i32> = (0..3)
                    .map(|i| i32::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
                    .collect();
                let spread = ch.iter().max().unwrap() - ch.iter().min().unwrap();
                assert!(spread <= 40, "#{hex} is not near-grayscale");
            }
        }
        assert!(colours >= 8);
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
        let cells: Vec<&str> = row
            .split("</tr>")
            .next()
            .unwrap()
            .split("<td>")
            .skip(1)
            .map(|c| c.trim_end_matches("</td>"))
            .collect();
        // achiral only (index 4), baseline only (2,5,9), p, then scope losses 3, owner losses 0
        assert_eq!(cells[0], "1");
        assert_eq!(cells[1], "3");
        assert_eq!(cells[3], "3");
        assert_eq!(cells[4], "0");
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
        assert!(svg.contains("whisker dash"));
        assert!(svg.contains("predicted probability") && svg.contains("observed frequency"));
    }

    #[test]
    fn the_tokens_chart_starts_at_zero() {
        let svg = cycles_svg(&[("a".into(), vec![(1, 30.0), (10, 31.0)])]);
        assert!(svg.contains(r#"text-anchor="end">0</text>"#));
        assert_eq!(
            svg.matches("<circle").count()
                + svg.matches("<polygon").count()
                + svg.matches("<rect").count(),
            2
        );
    }

    #[test]
    fn the_setting_curve_labels_each_value_on_its_axis() {
        let pts = vec![
            (0.0, Stat::from_values(&BIG, 0)),
            (0.2, Stat::from_values(&BIG_B, 0)),
        ];
        let svg = curve_svg("activationNoiseScale", &pts);
        assert!(svg.contains(">0.2</text>") && svg.contains(">0</text>"));
        assert_eq!(svg.matches("<circle").count(), 2);
    }
}

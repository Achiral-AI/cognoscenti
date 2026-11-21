//! Use-learning curves and the pre-registered decision rule
//! (`docs/preregistration-use-learning-2026-11.md`). Rows are replicate 0's
//! per-question recall, as stored in a results file; a pair is the same
//! question id in two arms.

use crate::results::{QuestionRow, Stat, MIN_N_FOR_CI};
use crate::stats::{bootstrap_statistic_ci_95, mcnemar_exact};
use crate::use_learning::use_bin;

pub const BINS: [&str; 4] = ["0", "1-2", "3-6", "7+"];

/// The primary comparison needs at least this many `pop` questions in the
/// highest bin.
pub const PRIMARY_MIN_N: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct BinStat {
    pub bin: &'static str,
    pub n: usize,
    /// `None` below `MIN_N_FOR_CI` questions: "n too small", not plotted.
    pub stat: Option<Stat>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairedDiff {
    pub n: usize,
    /// Arm minus baseline, mean per-question recall.
    pub mean: f64,
    pub ci: [f64; 2],
    /// Exact McNemar on pass (recall of 1.0); `1.0` with no discordant pair.
    pub p_value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// All three crossing conditions held in the primary population.
    pub improves_with_use: bool,
    /// Fewer than `PRIMARY_MIN_N` `pop` questions in the highest bin.
    pub underpowered: bool,
    /// Categories (`rare-c`, `change`) where the arm's recall minus the
    /// baseline's has a 95% interval entirely below zero.
    pub harms: Vec<String>,
    /// One line per failed condition, for the report.
    pub notes: Vec<String>,
}

/// Recall by use bin for one category, the four bins in order.
pub fn bin_stats(rows: &[QuestionRow], category: &str, seed: u64) -> Vec<BinStat> {
    BINS.iter()
        .map(|&bin| {
            let recalls: Vec<f64> = in_bin(rows, category, bin).map(|r| r.recall).collect();
            let stat = (recalls.len() >= MIN_N_FOR_CI).then(|| Stat::from_values(&recalls, seed));
            BinStat {
                bin,
                n: recalls.len(),
                stat,
            }
        })
        .collect()
}

fn in_bin<'a>(
    rows: &'a [QuestionRow],
    category: &'a str,
    bin: &'a str,
) -> impl Iterator<Item = &'a QuestionRow> {
    rows.iter().filter(move |r| {
        r.category.as_deref() == Some(category) && r.use_count.map(use_bin) == Some(bin)
    })
}

/// `arm` minus `base` over the questions both have and `keep` accepts.
/// `None` when no question is shared.
pub fn paired_diff(
    arm: &[QuestionRow],
    base: &[QuestionRow],
    keep: impl Fn(&QuestionRow) -> bool,
    seed: u64,
) -> Option<PairedDiff> {
    let base_by_id: std::collections::HashMap<&str, &QuestionRow> =
        base.iter().map(|r| (r.id.as_str(), r)).collect();
    let pairs: Vec<(&QuestionRow, &QuestionRow)> = arm
        .iter()
        .filter(|r| keep(r))
        .filter_map(|r| base_by_id.get(r.id.as_str()).map(|b| (r, *b)))
        .collect();
    if pairs.is_empty() {
        return None;
    }
    let diffs: Vec<f64> = pairs.iter().map(|(a, b)| a.recall - b.recall).collect();
    let interval = bootstrap_statistic_ci_95(diffs.len(), seed, |idx| {
        idx.iter().map(|&i| diffs[i]).sum::<f64>() / idx.len() as f64
    });
    let passes = |side: fn(&(&QuestionRow, &QuestionRow)) -> f64| -> Vec<bool> {
        pairs.iter().map(|p| side(p) >= 1.0).collect()
    };
    let test = mcnemar_exact(&passes(|p| p.0.recall), &passes(|p| p.1.recall));
    Some(PairedDiff {
        n: pairs.len(),
        mean: interval.estimate,
        ci: [interval.lo, interval.hi],
        p_value: test.p_value,
    })
}

/// The registered decision rule for `arm` against `base` (`off`, or
/// `usecount`).
pub fn judge(arm: &[QuestionRow], base: &[QuestionRow], seed: u64) -> Verdict {
    let pop_in = |bin: &'static str| {
        move |r: &QuestionRow| {
            r.category.as_deref() == Some("pop") && r.use_count.map(use_bin) == Some(bin)
        }
    };
    let first = paired_diff(arm, base, pop_in("0"), seed);
    let top = paired_diff(arm, base, pop_in("7+"), seed);
    let mut notes = Vec::new();

    let underpowered = top.as_ref().is_none_or(|d| d.n < PRIMARY_MIN_N);
    if underpowered {
        notes.push(format!(
            "underpowered: fewer than {PRIMARY_MIN_N} pop questions in the 7+ bin"
        ));
    }
    let first_ok = first.as_ref().is_some_and(|d| d.ci[0] <= 0.0);
    if !first_ok {
        notes.push("the arm already beats the baseline in the first-ask bin".to_string());
    }
    let top_ok = top
        .as_ref()
        .is_some_and(|d| d.ci[0] > 0.0 && d.p_value < 0.05);
    if !top_ok {
        notes.push("no significant gain over the baseline in the 7+ bin".to_string());
    }
    let own = bin_stats(arm, "pop", seed);
    let rose = match (&own[0].stat, &own[3].stat) {
        (Some(low), Some(high)) => lower(high) > upper(low),
        _ => false,
    };
    if !rose {
        notes.push(
            "the arm's own recall does not clearly rise from the 0 bin to the 7+ bin".to_string(),
        );
    }

    let harms = ["rare-c", "change"]
        .iter()
        .filter(|category| {
            paired_diff(
                arm,
                base,
                |r| r.category.as_deref() == Some(**category),
                seed,
            )
            .is_some_and(|d| d.ci[1] < 0.0)
        })
        .map(|c| c.to_string())
        .collect();

    Verdict {
        improves_with_use: !underpowered && first_ok && top_ok && rose,
        underpowered,
        harms,
        notes,
    }
}

fn lower(stat: &Stat) -> f64 {
    stat.ci.map_or(stat.estimate, |c| c[0])
}

fn upper(stat: &Stat) -> f64 {
    stat.ci.map_or(stat.estimate, |c| c[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` rows in `category` at `use_count`, the first `hits` of them
    /// recalled. Ids are `{category}-{start+i}-q`.
    fn rows(
        category: &str,
        use_count: usize,
        start: usize,
        n: usize,
        hits: usize,
    ) -> Vec<QuestionRow> {
        (0..n)
            .map(|i| QuestionRow {
                id: format!("{category}-{:03}-q", start + i),
                category: Some(category.to_string()),
                recall: if i < hits { 1.0 } else { 0.0 },
                tokens: None,
                use_count: Some(use_count),
            })
            .collect()
    }

    fn pop(first: (usize, usize), top: (usize, usize)) -> Vec<QuestionRow> {
        // (n, hits) in the first-ask bin and the 7+ bin
        let mut out = rows("pop", 0, 0, first.0, first.1);
        out.extend(rows("pop", 8, 100, top.0, top.1));
        out
    }

    #[test]
    fn bins_are_listed_in_order_with_n_and_too_small_ones_have_no_stat() {
        let mut all = rows("pop", 0, 0, 12, 6);
        all.extend(rows("pop", 1, 20, 9, 9));
        all.extend(rows("pop", 7, 40, 10, 10));
        all.extend(rows("change", 7, 60, 50, 50));
        let stats = bin_stats(&all, "pop", 0);
        let seen: Vec<(&str, usize, bool)> = stats
            .iter()
            .map(|b| (b.bin, b.n, b.stat.is_some()))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("0", 12, true),
                ("1-2", 9, false),
                ("3-6", 0, false),
                ("7+", 10, true)
            ]
        );
        assert_eq!(stats[0].stat.as_ref().unwrap().estimate, 0.5);
    }

    #[test]
    fn a_paired_difference_matches_rows_by_id_not_by_position() {
        let arm = rows("pop", 0, 0, 20, 20);
        let mut base = rows("pop", 0, 0, 20, 10);
        base.reverse();
        let d = paired_diff(&arm, &base, |_| true, 0).unwrap();
        assert_eq!(d.n, 20);
        assert!((d.mean - 0.5).abs() < 1e-12);
        assert!(d.ci[0] > 0.0);
        assert!(d.p_value < 0.01);
        assert_eq!(paired_diff(&arm, &[], |_| true, 0), None);
    }

    #[test]
    fn the_curves_cross_when_the_arm_starts_level_and_ends_clearly_ahead() {
        let off = pop((40, 20), (40, 20));
        let arm = pop((40, 20), (40, 36));
        let v = judge(&arm, &off, 0);
        assert!(v.improves_with_use, "{:?}", v.notes);
        assert!(!v.underpowered);
        assert!(v.harms.is_empty());
    }

    #[test]
    fn no_claim_when_the_arm_is_the_baseline() {
        let off = pop((40, 20), (40, 20));
        let v = judge(&off, &off, 0);
        assert!(!v.improves_with_use);
        assert!(!v.notes.is_empty());
    }

    #[test]
    fn no_claim_when_the_arm_already_wins_the_first_ask_bin() {
        let off = pop((40, 20), (40, 20));
        let arm = pop((40, 38), (40, 38));
        let v = judge(&arm, &off, 0);
        assert!(!v.improves_with_use);
    }

    #[test]
    fn a_top_bin_under_thirty_questions_is_underpowered() {
        let off = pop((40, 20), (20, 10));
        let arm = pop((40, 20), (20, 20));
        let v = judge(&arm, &off, 0);
        assert!(v.underpowered);
        assert!(!v.improves_with_use);
    }

    #[test]
    fn a_loss_on_change_is_reported_whatever_pop_shows() {
        let mut off = pop((40, 20), (40, 20));
        off.extend(rows("change", 0, 200, 40, 30));
        let mut arm = pop((40, 20), (40, 36));
        arm.extend(rows("change", 0, 200, 40, 5));
        let v = judge(&arm, &off, 0);
        assert!(v.improves_with_use);
        assert_eq!(v.harms, vec!["change".to_string()]);
    }

    #[test]
    fn no_claim_when_the_arm_beats_the_baseline_but_its_own_recall_falls_with_use() {
        let off = pop((40, 36), (40, 4));
        let arm = pop((40, 36), (40, 20));
        let v = judge(&arm, &off, 0);
        assert!(!v.improves_with_use);
        assert!(v.notes.iter().any(|n| n.contains("own recall")));
    }

    #[test]
    fn a_head_start_in_the_first_ask_bin_alone_blocks_the_claim() {
        let off = pop((40, 10), (40, 20));
        let arm = pop((40, 22), (40, 39));
        let v = judge(&arm, &off, 0);
        assert!(v.notes.iter().any(|n| n.contains("first-ask")));
        assert!(!v.improves_with_use);
    }

    #[test]
    fn a_top_bin_gain_inside_the_noise_alone_blocks_the_claim() {
        let off = pop((40, 10), (40, 34));
        let arm = pop((40, 10), (40, 36));
        let v = judge(&arm, &off, 0);
        assert!(v.notes.iter().any(|n| n.contains("no significant gain")));
        assert!(!v.improves_with_use);
    }
}

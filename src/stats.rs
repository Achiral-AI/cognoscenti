//! Statistics for published figures (Plan 40 Stage 1 item 3 and 9): a
//! seeded percentile bootstrap over questions, an exact McNemar test for
//! same-store paired comparisons, and the way a rate is printed with its
//! sample size. Every random draw comes from `seeded_rng(seed)`, so a CI is
//! reproducible from the run's `--seed`.

use crate::determinism::seeded_rng;
use crate::scoring::zero_leak_upper_bound_95;
use rand::Rng;

/// Bootstrap resamples per interval. 2000 keeps the 2.5th and 97.5th
/// percentiles stable to about the third decimal at n in the tens to
/// hundreds.
pub const BOOTSTRAP_RESAMPLES: usize = 2000;

/// A point estimate with its 95% percentile-bootstrap interval and the
/// number of questions it was resampled over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Interval {
    pub n: usize,
    pub estimate: f64,
    pub lo: f64,
    pub hi: f64,
}

impl Interval {
    /// `0.8571 (95% CI 0.7714-0.9429, n=70)`. When every resample gave the
    /// same value (all questions agree) the interval is degenerate and says
    /// so rather than implying certainty.
    pub fn display(&self) -> String {
        if self.lo == self.hi {
            return format!(
                "{:.4} (n={}, no variance observed: CI undefined)",
                self.estimate, self.n
            );
        }
        format!(
            "{:.4} (95% CI {:.4}-{:.4}, n={})",
            self.estimate, self.lo, self.hi, self.n
        )
    }
}

/// 95% percentile bootstrap of an arbitrary statistic over `n` questions.
/// `statistic` receives one resample's question indices (drawn with
/// replacement) and returns the statistic for them, so metrics that are not
/// a plain mean (ECE, flip rate) resample the same unit.
pub fn bootstrap_statistic_ci_95(
    n: usize,
    seed: u64,
    statistic: impl Fn(&[usize]) -> f64,
) -> Interval {
    assert!(n > 0, "need at least one question to bootstrap");
    let identity: Vec<usize> = (0..n).collect();
    let estimate = statistic(&identity);

    let mut rng = seeded_rng(seed);
    let mut draws = Vec::with_capacity(BOOTSTRAP_RESAMPLES);
    let mut sample = vec![0usize; n];
    for _ in 0..BOOTSTRAP_RESAMPLES {
        for slot in sample.iter_mut() {
            *slot = rng.gen_range(0..n);
        }
        draws.push(statistic(&sample));
    }
    draws.sort_by(|a, b| a.total_cmp(b));
    let lo = draws[(0.025 * BOOTSTRAP_RESAMPLES as f64) as usize];
    let hi = draws[((0.975 * BOOTSTRAP_RESAMPLES as f64) as usize).min(BOOTSTRAP_RESAMPLES - 1)];
    Interval {
        n,
        estimate,
        lo,
        hi,
    }
}

/// 95% bootstrap CI for the mean of a per-question metric (recall@k, a
/// 0/1 leak or flip indicator).
pub fn bootstrap_ci_95(values: &[f64], seed: u64) -> Interval {
    bootstrap_statistic_ci_95(values.len(), seed, |indices| {
        indices.iter().map(|&i| values[i]).sum::<f64>() / indices.len() as f64
    })
}

/// Outcome of an exact McNemar test on paired binary outcomes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct McNemar {
    pub both: usize,
    pub only_a: usize,
    pub only_b: usize,
    pub neither: usize,
    /// Two-sided exact p-value over the discordant pairs; `1.0` when there
    /// are none.
    pub p_value: f64,
}

/// Exact (binomial) McNemar test: `a[i]` and `b[i]` are the two systems'
/// pass/fail on the same question over the same store.
pub fn mcnemar_exact(a: &[bool], b: &[bool]) -> McNemar {
    assert_eq!(a.len(), b.len(), "paired outcomes must be the same length");
    let (mut both, mut only_a, mut only_b, mut neither) = (0, 0, 0, 0);
    for (&x, &y) in a.iter().zip(b) {
        match (x, y) {
            (true, true) => both += 1,
            (true, false) => only_a += 1,
            (false, true) => only_b += 1,
            (false, false) => neither += 1,
        }
    }
    let discordant = only_a + only_b;
    let p_value = if discordant == 0 {
        1.0
    } else {
        let tail = only_a.min(only_b);
        (2.0 * binomial_half_cdf(tail, discordant)).min(1.0)
    };
    McNemar {
        both,
        only_a,
        only_b,
        neither,
        p_value,
    }
}

/// P(X <= k) for X ~ Binomial(n, 0.5), summed in log space so large `n`
/// does not underflow.
fn binomial_half_cdf(k: usize, n: usize) -> f64 {
    let mut ln_fact = vec![0.0_f64; n + 1];
    for i in 1..=n {
        ln_fact[i] = ln_fact[i - 1] + (i as f64).ln();
    }
    (0..=k)
        .map(|j| {
            (ln_fact[n] - ln_fact[j] - ln_fact[n - j] - n as f64 * std::f64::consts::LN_2).exp()
        })
        .sum()
}

/// Reports a per-trial event rate (a leak, a flip). Zero events print as
/// the rule-of-three upper bound, never as "0%"; otherwise the bootstrap CI
/// over trials.
pub fn describe_event_rate(events: &[bool], seed: u64) -> String {
    let n = events.len();
    let hits = events.iter().filter(|&&e| e).count();
    if hits == 0 {
        return format!(
            "0/{n} events; 95% upper bound {:.4} (3/n)",
            zero_leak_upper_bound_95(n).min(1.0)
        );
    }
    let values: Vec<f64> = events.iter().map(|&e| f64::from(u8::from(e))).collect();
    format!(
        "{hits}/{n} events; {}",
        bootstrap_ci_95(&values, seed).display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn half_and_half(n: usize) -> Vec<f64> {
        (0..n).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect()
    }

    #[test]
    fn same_seed_gives_the_same_interval() {
        // Continuous values: 0/1 data on a 1/70 grid can repeat by chance
        // even with an unseeded rng.
        let values: Vec<f64> = (0..25).map(|i| f64::from(i * i % 17) / 17.0).collect();
        assert_eq!(bootstrap_ci_95(&values, 7), bootstrap_ci_95(&values, 7));
    }

    #[test]
    fn different_seeds_give_different_intervals() {
        // Continuous values: 0/1 data on a 1/70 grid can land on the same
        // percentile by chance.
        let values: Vec<f64> = (0..25).map(|i| f64::from(i * i % 17) / 17.0).collect();
        assert_ne!(bootstrap_ci_95(&values, 1), bootstrap_ci_95(&values, 2));
    }

    #[test]
    fn seventy_items_at_one_half_is_about_eleven_points_either_side() {
        // Plan 40's sizing claim: ~70 items gives roughly +/-11 points.
        let ci = bootstrap_ci_95(&half_and_half(70), 0);
        assert_eq!(ci.n, 70);
        assert!((ci.estimate - 0.5).abs() < 1e-9);
        assert!(ci.hi - ci.lo > 0.18 && ci.hi - ci.lo < 0.28, "{ci:?}");
        assert!(ci.lo < 0.5 && 0.5 < ci.hi);
    }

    #[test]
    fn identical_values_give_a_degenerate_interval_that_says_so() {
        let ci = bootstrap_ci_95(&[1.0; 5], 0);
        assert_eq!((ci.lo, ci.hi), (1.0, 1.0));
        assert!(ci.display().contains("no variance observed"));
    }

    #[test]
    fn a_custom_statistic_resamples_the_same_unit() {
        let values = half_and_half(40);
        let ci = bootstrap_statistic_ci_95(values.len(), 3, |idx| {
            idx.iter().map(|&i| values[i]).fold(0.0, f64::max)
        });
        assert_eq!(ci.estimate, 1.0);

        // Distinct indices per resample: all n in the identity pass, about
        // 63% in a with-replacement resample.
        let distinct = bootstrap_statistic_ci_95(40, 3, |idx| {
            let mut seen = idx.to_vec();
            seen.sort_unstable();
            seen.dedup();
            seen.len() as f64 / 40.0
        });
        assert_eq!(distinct.estimate, 1.0);
        assert!(distinct.lo > 0.45 && distinct.hi < 0.85, "{distinct:?}");
    }

    #[test]
    fn mcnemar_counts_pairs_and_tests_the_discordant_ones() {
        let a = [true; 8]
            .iter()
            .chain(&[true; 4])
            .copied()
            .collect::<Vec<_>>();
        let b = [false; 8]
            .iter()
            .chain(&[true; 4])
            .copied()
            .collect::<Vec<_>>();
        let result = mcnemar_exact(&a, &b);
        assert_eq!(
            (result.both, result.only_a, result.only_b, result.neither),
            (4, 8, 0, 0)
        );
        // 2 * 0.5^8
        assert!((result.p_value - 0.0078125).abs() < 1e-9, "{result:?}");
    }

    #[test]
    fn mcnemar_is_one_when_discordant_pairs_split_evenly_or_are_absent() {
        let a = [true, true, false, false];
        let b = [true, false, true, false];
        assert!((mcnemar_exact(&a, &b).p_value - 1.0).abs() < 1e-9);
        assert_eq!(mcnemar_exact(&[true, false], &[true, false]).p_value, 1.0);
    }

    #[test]
    fn mcnemar_does_not_underflow_on_many_discordant_pairs() {
        let a = vec![true; 3000];
        let b = vec![false; 3000];
        assert!(mcnemar_exact(&a, &b).p_value < 1e-100);
    }

    #[test]
    fn zero_events_print_as_the_three_over_n_bound_not_zero_percent() {
        let text = describe_event_rate(&vec![false; 300], 0);
        assert!(text.contains("0/300"), "{text}");
        assert!(text.contains("0.0100"), "{text}");
        assert!(!text.contains("CI"), "{text}");
    }

    #[test]
    fn the_bound_is_capped_at_one_for_fewer_than_three_trials() {
        assert!(describe_event_rate(&[false], 0).contains("bound 1.0000"));
    }

    #[test]
    fn nonzero_events_print_a_bootstrap_ci() {
        let mut events = vec![false; 100];
        events[0] = true;
        events[1] = true;
        let text = describe_event_rate(&events, 0);
        assert!(text.contains("2/100") && text.contains("95% CI"), "{text}");
    }
}

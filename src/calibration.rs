//! Retrieval calibration (Plan 25): "of memories retrieved with probability
//! 0.7, about 70% are used." This is the test behind any "Bayesian" or
//! "trustworthy" claim, and it does not require the system under test to
//! volunteer a probability.
//!
//! **Native** calibration calls the functions below directly on a system's
//! own stated probability (the ACT-R reference's `sigma(A)`, or Achiral
//! with noise). **Derived** calibration calls `derive_probabilities_from_scores`
//! first, then the same functions, for a system that makes no probabilistic
//! claim (Hindsight's cross-encoder score, a plain rank order) - it still
//! gets measured, just never credited with a claim it didn't make. A report
//! built from this module's output (a later phase) must say which path
//! produced each row; conflating them would claim a vendor said something
//! it didn't.

#[derive(Debug, Clone, PartialEq)]
pub struct ReliabilityBin {
    pub predicted_mean: f64,
    pub observed_frequency: f64,
    pub count: usize,
}

/// Groups `(predicted_probability, was_needed)` pairs into `num_bins` equal
///-width probability bins and reports each bin's mean prediction against
/// how often the memory was actually needed. Empty bins are omitted.
pub fn reliability_curve(predictions: &[(f64, bool)], num_bins: usize) -> Vec<ReliabilityBin> {
    assert!(num_bins > 0, "need at least one bin");
    let mut sums = vec![0.0_f64; num_bins];
    let mut hits = vec![0.0_f64; num_bins];
    let mut counts = vec![0usize; num_bins];

    for &(p, observed) in predictions {
        let bin = bin_index(p, num_bins);
        sums[bin] += p;
        counts[bin] += 1;
        if observed {
            hits[bin] += 1.0;
        }
    }

    (0..num_bins)
        .filter(|&bin| counts[bin] > 0)
        .map(|bin| ReliabilityBin {
            predicted_mean: sums[bin] / counts[bin] as f64,
            observed_frequency: hits[bin] / counts[bin] as f64,
            count: counts[bin],
        })
        .collect()
}

/// Mean squared error between predicted probability and the 0/1 outcome.
/// Lower is better; 0 is perfect, 1 is maximally wrong.
pub fn brier_score(predictions: &[(f64, bool)]) -> f64 {
    assert!(
        !predictions.is_empty(),
        "brier score needs at least one prediction"
    );
    let sum: f64 = predictions
        .iter()
        .map(|&(p, observed)| {
            let outcome = if observed { 1.0 } else { 0.0 };
            (p - outcome).powi(2)
        })
        .sum();
    sum / predictions.len() as f64
}

/// Expected calibration error: the count-weighted average gap between each
/// bin's mean prediction and its observed frequency.
pub fn expected_calibration_error(predictions: &[(f64, bool)], num_bins: usize) -> f64 {
    let bins = reliability_curve(predictions, num_bins);
    let total: usize = bins.iter().map(|bin| bin.count).sum();
    if total == 0 {
        return 0.0;
    }
    bins.iter()
        .map(|bin| {
            let weight = bin.count as f64 / total as f64;
            weight * (bin.observed_frequency - bin.predicted_mean).abs()
        })
        .sum()
}

/// Derives a probability for a system that returns a score (or any ranking
/// signal) but makes no probabilistic claim: items are grouped by rank into
/// `num_bins` equal-sized groups (highest score first), and every item in a
/// group is assigned that group's own observed frequency of `true` as its
/// derived probability. Returned in the same order as `scored`.
///
/// This needs nothing from the system beyond a ranking - not even a score
/// in [0, 1] - so a vendor cannot opt out of calibration by simply not
/// returning a probability.
pub fn derive_probabilities_from_scores(scored: &[(f64, bool)], num_bins: usize) -> Vec<f64> {
    assert!(num_bins > 0, "need at least one bin");
    assert!(!scored.is_empty(), "need at least one scored item");

    let mut ranked: Vec<usize> = (0..scored.len()).collect();
    ranked.sort_by(|&a, &b| {
        scored[b]
            .0
            .partial_cmp(&scored[a].0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let bin_count = num_bins.min(scored.len());
    let mut derived = vec![0.0_f64; scored.len()];
    for (group_idx, group) in ranked.chunks(ranked.len().div_ceil(bin_count)).enumerate() {
        let _ = group_idx;
        let observed_true = group.iter().filter(|&&i| scored[i].1).count();
        let frequency = observed_true as f64 / group.len() as f64;
        for &i in group {
            derived[i] = frequency;
        }
    }
    derived
}

fn bin_index(p: f64, num_bins: usize) -> usize {
    let scaled = (p * num_bins as f64).floor() as isize;
    scaled.clamp(0, num_bins as isize - 1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brier_score_matches_hand_computed_value() {
        let predictions = [(0.9, true), (0.9, true), (0.1, false)];
        let score = brier_score(&predictions);
        assert!((score - 0.01).abs() < 1e-9);
    }

    #[test]
    fn brier_score_is_high_when_confidently_wrong() {
        let predictions = [(0.9, false), (0.9, false)];
        let score = brier_score(&predictions);
        assert!((score - 0.81).abs() < 1e-9);
    }

    #[test]
    fn reliability_curve_groups_by_bin_and_reports_observed_frequency() {
        let predictions = [(0.2, false), (0.3, true), (0.7, true), (0.8, true)];
        let bins = reliability_curve(&predictions, 2);

        assert_eq!(bins.len(), 2);
        assert!((bins[0].predicted_mean - 0.25).abs() < 1e-9);
        assert!((bins[0].observed_frequency - 0.5).abs() < 1e-9);
        assert_eq!(bins[0].count, 2);

        assert!((bins[1].predicted_mean - 0.75).abs() < 1e-9);
        assert!((bins[1].observed_frequency - 1.0).abs() < 1e-9);
        assert_eq!(bins[1].count, 2);
    }

    #[test]
    fn a_prediction_of_exactly_one_lands_in_the_last_bin() {
        let bins = reliability_curve(&[(1.0, true)], 4);
        assert_eq!(bins.len(), 1);
        assert!((bins[0].predicted_mean - 1.0).abs() < 1e-9);
    }

    #[test]
    fn expected_calibration_error_matches_hand_computed_value() {
        let predictions = [(0.2, false), (0.3, true), (0.7, true), (0.8, true)];
        let ece = expected_calibration_error(&predictions, 2);
        assert!((ece - 0.25).abs() < 1e-9);
    }

    #[test]
    fn perfectly_calibrated_predictions_have_zero_ece() {
        let predictions = [(0.5, true), (0.5, false), (0.5, true), (0.5, false)];
        let ece = expected_calibration_error(&predictions, 2);
        assert!(ece.abs() < 1e-9);
    }

    #[test]
    fn derived_probabilities_follow_rank_without_any_native_score_claim() {
        // Highest-scoring bin (0.9T, 0.8T, 0.7F) is needed 2/3 of the time;
        // the lowest-scoring bin (0.6F, 0.5T, 0.4F) is needed 1/3 of the
        // time. Every item in a bin gets that bin's own frequency.
        let scored = [
            (0.9, true),
            (0.8, true),
            (0.7, false),
            (0.6, false),
            (0.5, true),
            (0.4, false),
        ];
        let derived = derive_probabilities_from_scores(&scored, 2);

        for &p in &derived[0..3] {
            assert!((p - 2.0 / 3.0).abs() < 1e-9);
        }
        for &p in &derived[3..6] {
            assert!((p - 1.0 / 3.0).abs() < 1e-9);
        }
    }

    #[test]
    fn derived_calibration_needs_no_native_probability_to_be_measured() {
        // A system that returns a bare rank (no score in [0, 1] at all,
        // just a monotonically decreasing "confidence") still produces a
        // usable reliability curve once derived.
        let ranks = [
            (5.0, true),
            (4.0, true),
            (3.0, false),
            (2.0, false),
            (1.0, false),
        ];
        let derived = derive_probabilities_from_scores(&ranks, 5);
        let predictions: Vec<(f64, bool)> = derived
            .iter()
            .zip(ranks.iter())
            .map(|(&p, &(_, observed))| (p, observed))
            .collect();

        // With one item per bin, each derived probability is exactly 0 or 1
        // (that item's own outcome), so ECE against those same outcomes is 0.
        let ece = expected_calibration_error(&predictions, 5);
        assert!(ece.abs() < 1e-9);
    }
}

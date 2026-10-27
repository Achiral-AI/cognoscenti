//! Cognoscenti's five kinds of stability (Plan 25): a memory system is not
//! scored on one run, because a non-deterministic one can look however you
//! want on any single run. Each is a pure measurement over results a
//! runner produces later (replaying a stream, shuffling it, paraphrasing a
//! query, tracking answers after a fact changes) - this module has no
//! runner or adapter of its own yet.
//!
//! 1. Run-to-run: [`flip_rate`] over repeated identical runs, and
//!    [`rank_agreement`] (Kendall's tau, restricted to items both rankings
//!    returned) between two of those runs' top-k lists.
//! 2. Order sensitivity: the same [`rank_agreement`], called instead on a
//!    canonical-order run against a shuffled-order run of the same stream.
//! 3. Paraphrase robustness: [`paraphrase_agreement`] (Jaccard of the
//!    returned evidence sets) between a query and its reworded form - order
//!    doesn't matter here, only whether the same evidence came back.
//! 4. Stability against plasticity: [`unchanged_fact_accuracy`] and
//!    [`recovery_time_in_steps`] after a fact change.
//! 5. Lock-in: [`longest_wrong_streak`] and [`is_locked_in`] over a
//!    sequence of top-1 picks.

use std::collections::HashMap;

/// Fraction of questions whose correctness is not the same across every
/// replicate. `per_question_outcomes[q]` is one question's pass/fail
/// outcome in each replicate.
pub fn flip_rate(per_question_outcomes: &[Vec<bool>]) -> f64 {
    if per_question_outcomes.is_empty() {
        return 0.0;
    }
    let flipped = per_question_outcomes
        .iter()
        .filter(|outcomes| {
            let first = outcomes.first().copied().unwrap_or(true);
            outcomes.iter().any(|&outcome| outcome != first)
        })
        .count();
    flipped as f64 / per_question_outcomes.len() as f64
}

/// Kendall's tau restricted to the items present in both rankings:
/// `(concordant - discordant) / total_pairs` among the common items, in
/// `[-1, 1]`. `1.0` is identical order, `-1.0` is fully reversed. Fewer
/// than two items in common returns `1.0` - there is no pair to disagree
/// on, which [`paraphrase_agreement`] covers as a set-overlap concern, not
/// an order concern.
pub fn rank_agreement(a: &[String], b: &[String]) -> f64 {
    let rank_a: HashMap<&str, usize> = a.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    let rank_b: HashMap<&str, usize> = b.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    let common: Vec<&str> = a
        .iter()
        .map(String::as_str)
        .filter(|id| rank_b.contains_key(id))
        .collect();

    if common.len() < 2 {
        return 1.0;
    }

    let mut concordant = 0i64;
    let mut discordant = 0i64;
    for i in 0..common.len() {
        for j in (i + 1)..common.len() {
            let (x, y) = (common[i], common[j]);
            if (rank_a[x] < rank_a[y]) == (rank_b[x] < rank_b[y]) {
                concordant += 1;
            } else {
                discordant += 1;
            }
        }
    }
    (concordant - discordant) as f64 / (concordant + discordant) as f64
}

/// Jaccard similarity of two returned-evidence sets: `|a ∩ b| / |a ∪ b|`.
/// Order doesn't matter - a paraphrase that returns the same evidence in a
/// different order should not be penalised the way order sensitivity does.
pub fn paraphrase_agreement(a: &[String], b: &[String]) -> f64 {
    let set_a: std::collections::HashSet<&str> = a.iter().map(String::as_str).collect();
    let set_b: std::collections::HashSet<&str> = b.iter().map(String::as_str).collect();
    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    intersection as f64 / union as f64
}

/// Accuracy over a window of questions about a fact that did not change -
/// should stay high as memory grows around it.
pub fn unchanged_fact_accuracy(observations: &[bool]) -> f64 {
    if observations.is_empty() {
        return 1.0;
    }
    observations.iter().filter(|&&correct| correct).count() as f64 / observations.len() as f64
}

/// Number of questions (1-indexed) after a fact change until the system
/// first answers correctly, or `None` if it never does within the given
/// observation window.
pub fn recovery_time_in_steps(observations_after_change: &[bool]) -> Option<usize> {
    observations_after_change
        .iter()
        .position(|&correct| correct)
        .map(|index| index + 1)
}

/// Longest run of consecutive wrong top-1 picks before `correct_id` wins.
pub fn longest_wrong_streak(top1_sequence: &[&str], correct_id: &str) -> usize {
    let mut longest = 0usize;
    let mut current = 0usize;
    for &pick in top1_sequence {
        if pick == correct_id {
            current = 0;
        } else {
            current += 1;
            longest = longest.max(current);
        }
    }
    longest
}

/// True when the sequence never recovers to `correct_id`: a plausible
/// wrong memory ranked first early on keeps winning through reinforcement
/// for the whole remaining window, rather than eventually losing out once
/// the correct memory has had a chance to be used (Plan 25).
pub fn is_locked_in(top1_sequence: &[&str], correct_id: &str) -> bool {
    !top1_sequence.is_empty() && !top1_sequence.contains(&correct_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flip_rate_counts_questions_whose_outcome_changes_across_replicates() {
        let outcomes = vec![
            vec![true, true, true],
            vec![true, false, true],
            vec![false, false, false],
        ];
        assert!((flip_rate(&outcomes) - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn rank_agreement_is_one_for_identical_order() {
        let a = ids(&["x", "y", "z"]);
        assert!((rank_agreement(&a, &a) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn rank_agreement_is_negative_one_for_fully_reversed_order() {
        let a = ids(&["x", "y", "z"]);
        let b = ids(&["z", "y", "x"]);
        assert!((rank_agreement(&a, &b) - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn rank_agreement_matches_a_hand_counted_single_swap() {
        let a = ids(&["x", "y", "z"]);
        let b = ids(&["y", "x", "z"]);
        assert!((rank_agreement(&a, &b) - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn rank_agreement_ignores_items_only_one_side_returned() {
        let a = ids(&["x", "y", "only-in-a"]);
        let b = ids(&["y", "x", "only-in-b"]);
        // Common items (x, y) swap -> one discordant pair out of one.
        assert!((rank_agreement(&a, &b) - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn paraphrase_agreement_matches_hand_computed_jaccard() {
        let a = ids(&["e1", "e2", "e3"]);
        let b = ids(&["e2", "e3", "e4"]);
        // Intersection {e2, e3} = 2, union {e1,e2,e3,e4} = 4.
        assert!((paraphrase_agreement(&a, &b) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn unchanged_fact_accuracy_matches_hand_computed_fraction() {
        assert!((unchanged_fact_accuracy(&[true, true, true, false]) - 0.75).abs() < 1e-9);
    }

    #[test]
    fn recovery_time_counts_questions_until_first_correct_answer() {
        assert_eq!(recovery_time_in_steps(&[false, false, true, true]), Some(3));
        assert_eq!(recovery_time_in_steps(&[false, false]), None);
    }

    #[test]
    fn longest_wrong_streak_matches_a_hand_counted_sequence() {
        let sequence = ["wrong", "wrong", "right", "wrong", "right", "right"];
        assert_eq!(longest_wrong_streak(&sequence, "right"), 2);
    }

    #[test]
    fn lock_in_is_true_only_when_correct_never_wins() {
        let never_recovers = ["wrong", "wrong", "wrong"];
        let eventually_recovers = ["wrong", "wrong", "right"];
        assert!(is_locked_in(&never_recovers, "right"));
        assert!(!is_locked_in(&eventually_recovers, "right"));
    }
}

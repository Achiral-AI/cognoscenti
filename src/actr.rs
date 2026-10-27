//! Cognoscenti's own ACT-R declarative-memory reference, written from the
//! published equations (ACT-R 7.x Reference Manual, Anderson & Lebiere) and
//! independent of Achiral's engine, so any memory system can be compared
//! against ACT-R without using Achiral. Parameters `d`, `tau`, `s` are set
//! per run and recorded in the run manifest (Plan 25).
//!
//! Worked numbers checked against literature fixtures are in
//! `docs/act-r-reference-fixtures.md`.

use crate::determinism::seeded_rng;
use rand::Rng;

/// Standard base-level activation, `B_i = ln(sum_j t_j^-d)`, summed over
/// every recorded use (not a single-access approximation). `uses_ago` is
/// time since each past use, in whatever unit `d` was fit to (Plan 25:
/// fit `d` to the store's own access logs rather than assume one unit).
/// Every use must be strictly positive time ago; ACT-R never evaluates
/// `t = 0` (a reference always has positive age when activation is
/// computed, per the reference manual).
pub fn base_level_activation(uses_ago: &[f64], d: f64) -> f64 {
    assert!(
        uses_ago.iter().all(|&t| t > 0.0),
        "every use must be a strictly positive time ago"
    );
    let sum: f64 = uses_ago.iter().map(|t| t.powf(-d)).sum();
    sum.ln()
}

/// Fan-effect associative strength, `S_ji = S - ln(fan_ji)`, where `fan` is
/// the number of chunks (including itself) the cue is associated with.
pub fn fan_strength(s_max: f64, fan: u32) -> f64 {
    assert!(fan >= 1, "fan effect is undefined for an unreferenced cue");
    s_max - (fan as f64).ln()
}

/// Total activation `A_i = B_i + sum_j W_j S_ji + noise`. `spreading` is
/// the already-weighted sum `sum_j W_j S_ji`; callers combine per-cue fan
/// strengths and weights before calling this.
pub fn activation(base_level: f64, spreading: f64, noise: f64) -> f64 {
    base_level + spreading + noise
}

/// ACT-R's logistic retrieval noise, variance `(pi^2/3) s^2`. Seeded, so
/// the same seed reproduces the same sampled ranking (Plan 25); unseeded
/// production use is a later concern, not this module's.
pub fn sample_noise(seed: u64, s: f64) -> f64 {
    let mut rng = seeded_rng(seed);
    let u: f64 = rng.gen_range(f64::EPSILON..1.0 - f64::EPSILON);
    s * (u / (1.0 - u)).ln()
}

/// Retrieval probability, logistic in activation minus the threshold:
/// `P = 1 / (1 + e^(-(A - tau)/s))`. At `tau = A`, `P = 0.5` (reference
/// manual, Tutorial Unit 4 section 4.6).
pub fn retrieval_probability(activation: f64, tau: f64, s: f64) -> f64 {
    1.0 / (1.0 + (-(activation - tau) / s).exp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    // Fixture 1: ACT-R 7.x Reference Manual `sdp` example (p.319-320),
    // `(sgp :esc t :bll .5 :mas 2 :mp 1)`, chunks A, B, C, D where B and C
    // both have slot1 = a. S_max = 2.
    #[test]
    fn fan_strength_matches_the_reference_manual_sdp_example() {
        // Chunk A: fan 3 (self + referenced by B's and C's slot1).
        assert!((fan_strength(2.0, 3) - 0.901_387_711_331_89).abs() < 1e-9);
        // Chunk B/C: fan 2 (self + referenced once).
        assert!((fan_strength(2.0, 2) - 1.306_852_819_440_05).abs() < 1e-9);
    }

    // Fixture 2: ACT-R Tutorial Unit 5 "Activation and Context" (8-Feb-05),
    // F = .63, S (:mas) = 1.6, B_i = 0 for all chunks.
    #[test]
    fn fan_strength_matches_the_tutorial_fan_effect_example() {
        // "The lawyer is in the store": fan(store) = fan(lawyer) = 2.
        let s_lawyer_store = fan_strength(1.6, 2);
        assert!((s_lawyer_store - 0.906_852_819_440_05).abs() < 1e-9);

        let activation_a = activation(0.0, 0.5 * s_lawyer_store + 0.5 * s_lawyer_store, 0.0);
        assert!((activation_a - 0.907).abs() < 1e-3);
        let time_a = 0.63 * (-activation_a).exp();
        assert!((time_a - 0.254).abs() < 1e-3);

        // "The hippie is in the bank": fan(hippie) = 4, verified against the
        // tutorial's own trace output (0.214). The tutorial's prose labels
        // fan(bank) = 2, but its printed trace (S = 0.501, A = 0.358,
        // RT = 0.441s) is only consistent with fan(bank) = 3
        // (1.6 - ln(3) = 0.5014). Trusting the verified trace numbers over
        // the prose label; flagged in docs/act-r-reference-fixtures.md.
        let s_hippie = fan_strength(1.6, 4);
        let s_bank = fan_strength(1.6, 3);
        assert!((s_hippie - 0.213_705_638_880_11).abs() < 1e-9);
        assert!((s_bank - 0.501_387_711_331_89).abs() < 1e-9);

        let activation_b = activation(0.0, 0.5 * s_hippie + 0.5 * s_bank, 0.0);
        assert!((activation_b - 0.358).abs() < 1e-3);
        let time_b = 0.63 * (-activation_b).exp();
        assert!((time_b - 0.441).abs() < 1e-3);
    }

    #[test]
    fn base_level_activation_sums_power_law_decay_over_every_use() {
        // One use, t=1 (whatever unit d is fit to), d=0.5: B = ln(1^-0.5) = 0.
        assert!((base_level_activation(&[1.0], 0.5) - 0.0).abs() < 1e-12);

        // Two uses at the same t collapse to ln(2 * t^-d).
        let two_uses = base_level_activation(&[4.0, 4.0], 0.5);
        let expected = (2.0 * 4.0_f64.powf(-0.5)).ln();
        assert!((two_uses - expected).abs() < 1e-12);

        // More recent or more frequent use raises B_i.
        let recent = base_level_activation(&[1.0], 0.5);
        let old = base_level_activation(&[100.0], 0.5);
        assert!(recent > old);
    }

    #[test]
    #[should_panic(expected = "strictly positive")]
    fn base_level_activation_rejects_zero_elapsed_time() {
        base_level_activation(&[0.0], 0.5);
    }

    #[test]
    fn retrieval_probability_is_one_half_at_threshold() {
        assert!((retrieval_probability(1.0, 1.0, 0.25) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn retrieval_probability_rises_with_activation_above_threshold() {
        let low = retrieval_probability(0.5, 1.0, 0.25);
        let high = retrieval_probability(2.0, 1.0, 0.25);
        assert!(high > low);
        assert!((0.0..=1.0).contains(&low));
        assert!((0.0..=1.0).contains(&high));
    }

    #[test]
    fn noise_is_reproducible_under_the_same_seed_and_diverges_across_seeds() {
        assert_eq!(sample_noise(42, 0.25), sample_noise(42, 0.25));
        assert_ne!(sample_noise(1, 0.25), sample_noise(2, 0.25));
    }

    #[test]
    fn zero_scale_s_leaves_activation_unchanged() {
        // s = 0 is today's behaviour (Plan 25 code change 5): no noise.
        let noise = sample_noise(1, 0.0);
        assert_eq!(noise, 0.0);
    }

    #[test]
    fn noise_variance_matches_the_logistic_closed_form() {
        // ACT-R's noise is logistic with variance (pi^2/3) s^2 (reference
        // manual, p.192-193). Checked empirically over many draws, not
        // proof for any single sample.
        let s = 0.25;
        let samples: Vec<f64> = (0..20_000).map(|seed| sample_noise(seed, s)).collect();
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let variance =
            samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / samples.len() as f64;
        let expected_variance = (PI * PI / 3.0) * s * s;

        assert!(mean.abs() < 0.01, "logistic noise should be centred at 0");
        assert!(
            (variance - expected_variance).abs() / expected_variance < 0.05,
            "variance {variance} should be within 5% of {expected_variance}"
        );
    }
}

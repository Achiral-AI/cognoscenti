//! Every source of randomness in Cognoscenti must come from a seeded RNG
//! derived from the run's seed, never from thread-local or OS randomness.
//! A harness with its own variance cannot measure a system's (Plan 25).

use rand::rngs::StdRng;
use rand::SeedableRng;

/// Builds the one RNG a run should use. Every later phase that needs
/// randomness (stream event timing and ordering, replicate shuffles, ACT-R
/// retrieval noise) seeds from this, never from `rand::thread_rng()`.
pub fn seeded_rng(seed: u64) -> StdRng {
    StdRng::seed_from_u64(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;

    #[test]
    fn same_seed_reproduces_the_same_sequence() {
        let mut a = seeded_rng(42);
        let mut b = seeded_rng(42);

        let sequence_a: Vec<u64> = (0..8).map(|_| a.gen()).collect();
        let sequence_b: Vec<u64> = (0..8).map(|_| b.gen()).collect();

        assert_eq!(sequence_a, sequence_b);
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = seeded_rng(1);
        let mut b = seeded_rng(2);

        let sequence_a: Vec<u64> = (0..8).map(|_| a.gen()).collect();
        let sequence_b: Vec<u64> = (0..8).map(|_| b.gen()).collect();

        assert_ne!(sequence_a, sequence_b);
    }
}

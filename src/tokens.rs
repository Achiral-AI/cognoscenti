//! Token accounting for the "durable horizon" measurement (Plan 25's fourth
//! axis): what a context-injection architecture actually costs in tokens,
//! not what it costs on one lucky run. A word-based approximation, not a
//! real tokenizer - good enough to compare architectures' shapes against
//! each other, not to predict a vendor's exact bill.

/// Approximate token count: whitespace-delimited words. No subword
/// splitting, no model-specific vocabulary - a word is counted as one
/// token regardless of length.
pub fn approx_token_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Open-ended geometric checkpoints (10, 100, 1000, ...) up to and
/// including `max`, so a durability claim is checked at growing scale
/// instead of on one cycle count chosen after the fact. `max` itself is
/// always the last checkpoint, even when it falls between powers of ten.
pub fn geometric_checkpoints(max: usize) -> Vec<usize> {
    if max == 0 {
        return Vec::new();
    }
    let mut checkpoints = Vec::new();
    let mut step = 10;
    while step < max {
        checkpoints.push(step);
        step *= 10;
    }
    checkpoints.push(max);
    checkpoints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_has_no_tokens() {
        assert_eq!(approx_token_count(""), 0);
    }

    #[test]
    fn counts_one_token_per_whitespace_delimited_word() {
        assert_eq!(approx_token_count("the cat sat"), 3);
    }

    #[test]
    fn collapses_runs_of_whitespace() {
        assert_eq!(approx_token_count("the   cat\n\tsat"), 3);
    }

    #[test]
    fn punctuation_attached_to_a_word_does_not_split_it() {
        assert_eq!(approx_token_count("hello, world!"), 2);
    }

    #[test]
    fn zero_checkpoints_is_empty() {
        assert_eq!(geometric_checkpoints(0), Vec::<usize>::new());
    }

    #[test]
    fn below_the_first_power_of_ten_only_the_max_is_a_checkpoint() {
        assert_eq!(geometric_checkpoints(5), vec![5]);
        assert_eq!(geometric_checkpoints(10), vec![10]);
    }

    #[test]
    fn between_powers_of_ten_the_max_is_appended_after_them() {
        assert_eq!(geometric_checkpoints(50), vec![10, 50]);
    }

    #[test]
    fn spans_multiple_decades_up_to_an_exact_power_of_ten() {
        assert_eq!(geometric_checkpoints(1000), vec![10, 100, 1000]);
    }

    #[test]
    fn appends_a_max_that_falls_between_decades() {
        assert_eq!(geometric_checkpoints(2500), vec![10, 100, 1000, 2500]);
    }
}

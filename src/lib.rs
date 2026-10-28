pub mod actr;
pub mod adapter;
pub mod baselines;
pub mod calibration;
pub mod core;
pub mod determinism;
pub mod eval;
pub mod lint;
pub mod manifest;
pub mod runner;
pub mod scoring;
pub mod stability;
pub mod stream;
pub mod tokens;

pub use actr::{
    activation, base_level_activation, fan_strength, retrieval_probability, sample_noise,
};
pub use adapter::{
    replay_stream, run_baseline_replay, token_usage_by_cycle, MemorySystem, QuestionResult,
};
pub use baselines::{
    build_baseline, Bm25System, BoundedWindowSystem, FullContextSystem, RecencySystem,
    VerbatimSystem,
};
pub use calibration::{
    brier_score, derive_probabilities_from_scores, expected_calibration_error, reliability_curve,
    ReliabilityBin,
};
pub use core::{Context, MemoryChunk, RetrievalResult};
pub use determinism::seeded_rng;
pub use eval::{BenchmarkItem, MemoryEvalMetrics, MemoryEvalReport, SystemEvalReport};
pub use lint::{lint, LintIssue, ANSWER_REVEALING_MARKERS};
pub use manifest::RunManifest;
pub use runner::{BenchmarkConfig, BenchmarkRunner};
pub use scoring::{is_authorized, leak_rate, recall_at_k, zero_leak_upper_bound_95, LeakTrial};
pub use stability::{
    flip_rate, is_locked_in, longest_wrong_streak, paraphrase_agreement, rank_agreement,
    recovery_time_in_steps, unchanged_fact_accuracy,
};
pub use stream::{
    Actor, ActorKind, Scope, ScopeKind, Stream, StreamEvent, StreamItem, StreamQuestion,
    ValidityWindow,
};
pub use tokens::{approx_token_count, geometric_checkpoints};

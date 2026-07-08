use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Core evaluation dimensions for cognitive memory systems
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvaluationDimension {
    Activation(ActivationMetrics),
    Forgetting(ForgettingMetrics),
    Interference(InterferenceMetrics),
    Contextual(ContextualMetrics),
    Consolidation(ConsolidationMetrics),
    Adaptation(AdaptationMetrics),
    Efficiency(EfficiencyMetrics),
}

/// Metrics for evaluating activation precision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationMetrics {
    pub top1_accuracy: f64,
    pub top3_accuracy: f64,
    pub irrelevant_activation_rate: f64,
    pub avg_retrieval_latency_ms: f64,
    pub activation_confidence_mean: f64,
    pub activation_confidence_std: f64,
}

/// Metrics for evaluating selective forgetting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgettingMetrics {
    pub junk_activation_rate: f64,
    pub retrieval_precision: f64,
    pub forgotten_memory_ratio: f64,
    pub decay_rate: f64,
    pub irrelevant_memories_ignored: u32,
}

/// Metrics for evaluating interference resistance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterferenceMetrics {
    pub top1_accuracy: f64,
    pub top3_accuracy: f64,
    pub distractor_count: u32,
    pub avg_retrieval_confidence: f64,
    pub confusion_rate: f64,
}

/// Metrics for evaluating contextual recall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextualMetrics {
    pub context_switch_success_rate: f64,
    pub cross_context_interference: f64,
    pub domain_specific_accuracy: HashMap<String, f64>,
    pub context_drift_score: f64,
}

/// Metrics for evaluating memory consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationMetrics {
    pub retrieval_speed_improvement: f64,
    pub activation_increase_per_access: f64,
    pub consolidation_rate: f64,
    pub repeated_access_accuracy: f64,
}

/// Metrics for evaluating adaptation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptationMetrics {
    pub outdated_info_superseded: f64,
    pub historical_context_preserved: f64,
    pub adaptation_latency_ms: f64,
    pub belief_update_accuracy: f64,
}

/// Metrics for evaluating efficiency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EfficiencyMetrics {
    pub memories_examined_avg: f64,
    pub activated_chunks_avg: f64,
    pub retrieval_latency_p50: f64,
    pub retrieval_latency_p95: f64,
    pub retrieval_latency_p99: f64,
    pub token_cost_per_retrieval: f64,
}

impl ActivationMetrics {
    pub fn new() -> Self {
        Self {
            top1_accuracy: 0.0,
            top3_accuracy: 0.0,
            irrelevant_activation_rate: 0.0,
            avg_retrieval_latency_ms: 0.0,
            activation_confidence_mean: 0.0,
            activation_confidence_std: 0.0,
        }
    }
}

impl ForgettingMetrics {
    pub fn new() -> Self {
        Self {
            junk_activation_rate: 0.0,
            retrieval_precision: 0.0,
            forgotten_memory_ratio: 0.0,
            decay_rate: 0.0,
            irrelevant_memories_ignored: 0,
        }
    }
}

impl InterferenceMetrics {
    pub fn new() -> Self {
        Self {
            top1_accuracy: 0.0,
            top3_accuracy: 0.0,
            distractor_count: 0,
            avg_retrieval_confidence: 0.0,
            confusion_rate: 0.0,
        }
    }
}

impl ContextualMetrics {
    pub fn new() -> Self {
        Self {
            context_switch_success_rate: 0.0,
            cross_context_interference: 0.0,
            domain_specific_accuracy: HashMap::new(),
            context_drift_score: 0.0,
        }
    }
}

impl ConsolidationMetrics {
    pub fn new() -> Self {
        Self {
            retrieval_speed_improvement: 0.0,
            activation_increase_per_access: 0.0,
            consolidation_rate: 0.0,
            repeated_access_accuracy: 0.0,
        }
    }
}

impl AdaptationMetrics {
    pub fn new() -> Self {
        Self {
            outdated_info_superseded: 0.0,
            historical_context_preserved: 0.0,
            adaptation_latency_ms: 0.0,
            belief_update_accuracy: 0.0,
        }
    }
}

impl EfficiencyMetrics {
    pub fn new() -> Self {
        Self {
            memories_examined_avg: 0.0,
            activated_chunks_avg: 0.0,
            retrieval_latency_p50: 0.0,
            retrieval_latency_p95: 0.0,
            retrieval_latency_p99: 0.0,
            token_cost_per_retrieval: 0.0,
        }
    }
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a single memory chunk in the cognitive system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: String,
    pub content: String,
    pub activation_level: f64,
    pub creation_time: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub context: Context,
    pub tags: Vec<String>,
    pub importance: f64,
}

/// Contextual information associated with memories
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Context {
    pub project: Option<String>,
    pub domain: Domain,
    pub conversation_id: Option<String>,
    pub participants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Domain {
    Engineering,
    Marketing,
    Finance,
    Design,
    General,
}

/// Result of a memory retrieval operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub retrieved_chunks: Vec<MemoryChunk>,
    pub memories_examined: u32,
    pub retrieval_latency_ms: u64,
    pub confidence_scores: Vec<f64>,
    pub timestamp: DateTime<Utc>,
}

impl MemoryChunk {
    pub fn new(id: String, content: String, context: Context) -> Self {
        let now = Utc::now();
        Self {
            id,
            content,
            activation_level: 0.5,
            creation_time: now,
            last_accessed: now,
            access_count: 0,
            context,
            tags: Vec::new(),
            importance: 0.5,
        }
    }

    pub fn accessed(&mut self) {
        self.access_count += 1;
        self.last_accessed = Utc::now();
        // Activation-based strengthening
        self.activation_level = (self.activation_level + 0.1).min(1.0);
    }

    pub fn decay(&mut self, decay_factor: f64) {
        self.activation_level = (self.activation_level * decay_factor).max(0.0);
    }
}

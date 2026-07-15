# Cognoscenti Roadmap

## v0.2 - Enhanced Evaluation Framework

### Tangential Use Cases from TWIST Benchmark

Based on analysis of [TWIST benchmark](https://github.com/subratpanda/twist-benchmark), we can incorporate:

- **Belief Supersession**: Track when newer information correctly replaces outdated beliefs while preserving historical context
- **Safe Recall**: Ensure retrieved memories don't contain sensitive or deprecated information that could cause harm
- **Tension Detection**: Identify when retrieved memories conflict with each other or with current context
- **Draft Alignment**: Verify that generated responses align with stored memories without hallucination

### Structured Data Format

Adopt JSON-per-line format for benchmark items:
```json
{
  "id": "item-001",
  "type": "activation|forgetting|interference|contextual|consolidation|adaptation|efficiency",
  "query": "...",
  "context": {...},
  "gold_memory": "...",
  "distractors": [...],
  "rationale": "..."
}
```

### Clear System API

Define standard interface for systems under test:
```rust
trait CognitiveMemorySystem {
    fn ingest(&mut self, events: Vec<MemoryEvent>) -> Result<()>;
    fn retrieve(&self, query: &Query) -> Result<RetrievalResult>;
    fn check_belief_supersession(&self, old_belief: &Memory, new_belief: &Memory) -> Result<bool>;
    fn check_tension(&self, memories: Vec<Memory>) -> Result<Vec<Tension>>;
}
```

### Paired Metrics

Following TWIST's approach, report paired metrics:
- Detection + Attribution (for interference resistance)
- Precision + Recall (for activation)
- Forgetting Rate + False Negative Rate (for selective forgetting)
- Supersession Accuracy + Historical Preservation (for adaptation)

## v0.3 - Reference Implementations

Add reference implementations for:
- Baseline RAG system
- ACT-R-inspired system
- Vector database with decay
- Simple LRU cache

## v0.4 - Human Evaluation

- Human double-annotation with adjudication
- Disputes and errata process
- Versioned item keys
- Public errata file

## v1.0 - Production Release

**Ship Criterion**: Reference baselines and cognitive systems must show statistically significant separation on key dimensions.

### Requirements
- ≥100 benchmark items per dimension
- Human-annotated gold standard
- Reproducible results
- Comprehensive documentation
- Multiple reference implementations

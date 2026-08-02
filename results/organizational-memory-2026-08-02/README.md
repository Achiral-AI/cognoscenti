# Organizational Memory Benchmark Run

Run date: 2026-08-02

This run evaluates three reference memory architectures against the same Cognoscenti JSONL workload:

- `rag`: lexical retrieval over stored memories. This represents a vanilla retrieval baseline.
- `agent-memory`: lexical retrieval plus project, domain, and conversation scope. This represents a stateful agent-memory baseline.
- `organic-memory`: scoped retrieval plus lifecycle signals for current, validated, reinforced, stale, superseded, and low-signal memories. This represents an ACT-R-inspired organic memory baseline.

The run does not claim to benchmark Mem0, Zep, Letta, Glean, LangGraph, or any other vendor product. Vendor adapters exist through Cognoscenti's HTTP eval mode, but vendor results should be published only when endpoints, keys, seeds, commands, and raw outputs are recorded.

## Workload

Input file:

```bash
examples/organizational_memory.jsonl
```

The workload contains 12 organizational-memory items across six cognitive behaviors:

- activation
- selective forgetting
- interference resistance
- contextual recall
- consolidation
- adaptation

Some memories intentionally recur across items. That repetition tests whether a memory system can handle reused organizational context without confusing current, stale, and cross-project memories.

## Reproduction Command

```bash
cargo run --release -- \
  --items-file examples/organizational_memory.jsonl \
  --systems rag,agent-memory,organic-memory \
  --top-k 3 \
  --output-dir results/organizational-memory-2026-08-02
```

## Results

| System | Top-1 Accuracy | Recall@3 | Precision@3 | Distractor Activation | Error Rate |
| --- | ---: | ---: | ---: | ---: | ---: |
| RAG reference baseline | 50.00% | 83.33% | 27.78% | 25.00% | 0.00% |
| Agent-memory reference baseline | 75.00% | 100.00% | 33.33% | 38.89% | 0.00% |
| Organic-memory reference baseline | 91.67% | 100.00% | 33.33% | 16.67% | 0.00% |

## Interpretation

The organic-memory reference baseline had the highest Top-1 accuracy and the lowest distractor activation rate while matching agent-memory on Recall@3. In this run, the advantage came from lifecycle-aware scoring: current, validated, and reinforced memories were promoted, while stale, superseded, deprecated, and low-signal memories were suppressed.

This supports the benchmark standard Cognoscenti is meant to set: state-of-the-art memory systems should not be judged only by whether they can retrieve a stored fact. They should also be judged by whether the right memory becomes active, the wrong memory stays quiet, and repeated organizational experience becomes easier to use.

## Artifacts

- `memory_eval_results.json`: full item-level results.
- `memory_eval_metrics.csv`: system-level metrics.
- `memory_eval_report.pdf`: generated report.
- `examples/organizational_memory.jsonl`: benchmark workload.

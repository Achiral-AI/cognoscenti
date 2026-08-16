# 2026-08-02 AI Memory Benchmark Run

Cognoscenti now includes a public organizational-memory benchmark run for evaluating whether AI memory can be trusted under use.

The run compares three reference architectures on the same 12-item JSONL workload:

- `rag`: lexical retrieval over stored memories.
- `agent-memory`: lexical retrieval plus project, domain, and conversation scope.
- `organic-memory`: scoped retrieval plus lifecycle signals for current, validated, reinforced, stale, superseded, and low-signal memories.

## Headline Results

| System | Top-1 Accuracy | Recall@3 | Precision@3 | Distractor Activation |
| --- | ---: | ---: | ---: | ---: |
| RAG reference baseline | 50.00% | 83.33% | 27.78% | 25.00% |
| Agent-memory reference baseline | 75.00% | 100.00% | 33.33% | 38.89% |
| Organic-memory reference baseline | 91.67% | 100.00% | 33.33% | 16.67% |

The organic-memory reference baseline led the tested architectures on Top-1 accuracy and distractor suppression while matching agent memory on Recall@3.

## Reproduce The Run

```bash
cargo run --release -- \
  --items-file examples/organizational_memory.jsonl \
  --systems rag,agent-memory,organic-memory \
  --top-k 3 \
  --output-dir results/organizational-memory-2026-08-02
```

## Public Artifacts

- Public report: https://achiral.ai/benchmarks/ai-memory-benchmark
- Workload: `examples/organizational_memory.jsonl`
- Methodology and run notes: `results/organizational-memory-2026-08-02/README.md`
- Results JSON: `results/organizational-memory-2026-08-02/memory_eval_results.json`
- Metrics CSV: `results/organizational-memory-2026-08-02/memory_eval_metrics.csv`

## Claim Boundary

This release does not claim to benchmark Mem0, Zep, Letta, Glean, LangGraph, or any other vendor product. Vendor adapters exist through Cognoscenti's HTTP eval mode, but vendor results should be published only when endpoints, keys, seeds, commands, and raw outputs are recorded.

The current claim is narrower: Cognoscenti defines a reproducible standard for testing whether memory systems activate useful context, suppress stale or misleading context, and adapt through use.

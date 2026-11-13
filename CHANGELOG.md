# Changelog

All notable changes to Cognoscenti will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-11-13

Replaces the single JSONL item eval's framing with the trust-axis
measurement framework that now anchors the README and the public
benchmark page: calibration, stability, and scope.

### Added
- Stream eval mode (`src/stream.rs`, dataset lint `src/lint.rs`): a
  chronologically ordered JSONL event/question stream, the current
  build-out (Plan 25) alongside the original JSONL item eval.
- ACT-R reference (`src/actr.rs`): base-level activation, fan effect,
  logistic retrieval probability and noise.
- Calibration (`src/calibration.rs`: reliability curve, Brier score,
  expected calibration error), scoring (`src/scoring.rs`: recall@k,
  scope-based leak rate), and five stability tests (`src/stability.rs`:
  run-to-run, order sensitivity, paraphrase robustness, stability against
  plasticity, lock-in).
- Token accounting (`src/tokens.rs`), two new baselines
  (`FullContextSystem`, `BoundedWindowSystem`), and a checkpointed
  `--cycles` runner for the tokens-vs-cycles hypothesis.
- Deterministic, seeded replay harness (`src/determinism.rs`,
  `src/manifest.rs`) with run manifests and dataset checksums;
  `--replay-stream` and `--replicates` CLI modes.
- Three stream dataset categories under the mesh nomenclature
  (`individual-mesh/`, `multiplayer-mesh/`, `mixed-player-mesh/`),
  including the `atlas-01`/`atlas-02`/`atlas-03` Plan 26 traversal tests
  and `consolidation-01` (a direct test of memory emergence).
- Gap-disclosure metric for Plan 26 Phase 5.
- Adapter contract v2 pieces (`AchiralHttpSystem`, `HttpStreamSystem`),
  `settle()`-based replay wiring against achiral.ai, per-replicate
  independent orgs for real stability testing, and `ScopeKind::User`
  wiring to achiral.ai's user-scoped memory route.
- README "Measurement framework" section with four axis diagrams
  (calibration, stability, scope, tokens-vs-cycles).

### Fixed
- Resolve the original `StreamEvent` id from metadata rather than
  achiral's raw Weaviate id; send event id and explicit `supersedes` as
  top-level achiral ingest fields.
- Replace `settle()`'s blind sleep with an active write-confirmation poll.
- Stop double-ingesting replicate 0's org via the preliminary single pass.

### Changed
- README restructured around non-deterministic/probabilistic memory as
  the core distinction, with Implementation/Usage/Citing split into
  `docs/`.

### Known gaps
- No live run yet against a vendor or Achiral system over the stream
  eval; baselines are local references only.
- Stream datasets are small (five datasets across three categories);
  none is benchmark-scale.
- Tier 2 of the adapter contract (actor/scope-aware
  `answer`/`use`/`settle`) is not started; leak rate has only been
  unit-tested, never run against real retrieval output.
- The token-savings axis and all four Measurement framework diagrams
  remain schematics/hypotheses — no published number yet.

## [0.2.0] - 2026-08-16

### Added
- PDF report generation with printpdf library
- CLI flag `--generate-pdf` for PDF report control (default: true)
- Comprehensive developer documentation (DEVELOPERS.md)
- Sponsorship documentation (SPONSORSHIP.md) with multiple tiers
- Prominent sponsorship section in README with clear CTAs
- GitHub issue templates (bug report, feature request)
- GitHub pull request template
- CI workflow with multi-platform testing and security audit
- Funding configuration for GitHub Sponsors
- Code quality configuration (rustfmt.toml, clippy.toml)
- Enhanced .gitignore with Rust-specific patterns and hygiene rules
- Citation format and acknowledgments in README
- Quote: "The problem isn't how much is remembered, but what gets chosen as memory."

### Changed
- Enhanced README with badges, sponsorship section, and developer documentation links
- Improved repository hygiene with best-in-class open source practices
- Updated .gitignore to exclude benchmark results and IDE files

### Dependencies
- Added printpdf 0.6 for PDF generation
- Removed lopdf dependency (switched to printpdf)

## [0.1.0] - 2024-01-XX

### Added
- Initial release of Cognoscenti benchmark
- Workload simulation for cognitive memory systems
- Core evaluation dimensions: activation, selective forgetting, interference resistance, contextual recall, memory consolidation, adaptation, efficiency
- Metrics collection with statistical analysis
- Visualization with plotters library
- JSON and CSV output formats
- Five workload types: strategic, technical, creative, episodic, analytical
- CLI interface with clap

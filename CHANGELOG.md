# Changelog

All notable changes to Cognoscenti will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- The `achiral` adapter asks an agent's question on the org route as well as the agent route
  and interleaves the two lists rank by rank (agent first) up to top-k. A score merge let the
  dense org pool push a private agent note out of the top 3. Before, an org-scope gold fact could
  never be returned to an agent, so the `agent` category scored 0 (and its zero leak was
  vacuous). Agent-category results from earlier runs are not comparable.

### Added
- `cognoscenti rebuild-results` rebuilds a complete cohort results file from a replicate log and
  the stream, for a run whose own results were lost to a crash. It fills per-question, pooled and
  per-category recall and the flip rate, checks printed summary lines against the log, refuses
  anything it cannot reproduce honestly, and leaves tokens, calibration and the leak table absent.
  `QuestionRow.tokens` is now optional.
- `COGNOSCENTI_ACHIRAL_RETRIEVAL_MODE` and `COGNOSCENTI_ACHIRAL_RETRIEVAL_PARAMS` send
  `x-achiral-retrieval-mode` and `x-achiral-retrieval-params` on every `achiral` retrieve.
  Both are validated at startup, unset sends nothing, and the first retrieve of each replicate
  must echo them back or the run stops. The setting is written to the results file and into the
  cohort name and manifest table of the report.
- `--results-json` writes a replay's numbers as JSON, and `cognoscenti report` turns one or more
  of those files (one per labelled cohort) into a self-contained HTML page with `results.json`
  and `results.csv` beside it. Inline SVG in colour that carries meaning (a hue per Achiral
  cohort, quieter hues per baseline, green ahead, orange-red behind, red leak, grey hatch for
  a small n), a one-line result above each chart, light and dark, phone width.
- The `achiral` adapter retries transient failures (connection errors, HTTP 502/503/504,
  non-JSON error pages) with capped exponential backoff and jitter, default 8 retries,
  configurable with `COGNOSCENTI_ACHIRAL_MAX_RETRIES` (0 to 10),
  `COGNOSCENTI_ACHIRAL_RETRY_BASE_MS` and `COGNOSCENTI_ACHIRAL_RETRY_CAP_MS`. Real API error
  bodies still stop the run at once. Every write now sends an `Idempotency-Key`.
- `--replicate-log` and `--resume`: each replicate prints a summary line and is logged as it
  finishes; a resumed run skips finished replicates and refuses an org the log already used.
- `--generate-profile natural-names`: a second generated dataset with natural two-word
  project names and varied wording, so a unique coined token is not a lexical giveaway. The
  default profile is unchanged byte for byte.
- `COGNOSCENTI_ACHIRAL_CONFIRM_MS`: poll budget for org-scope write confirmation,
  separate from `COGNOSCENTI_ACHIRAL_SETTLE_MS`. An event still unconfirmed after it
  now stops the run instead of printing a warning.
- `--skip-user-scope`: drop generated trials that hold a user-scope event and
  report them as not run, for deployments with per-member routes disabled.
- `--label`: free-text run label printed at the top of a replay, for settings
  outside the harness such as a noise scale.
- `src/stats.rs`: seeded 95% bootstrap CIs over questions, exact McNemar
  test, and rule-of-three reporting for zero-event results. Replay output
  now prints n and a CI on every rate; its leak and flip lines changed
  wording (see `docs/usage.md`).
- `src/generator.rs` and `--generate-stream`: seeded synthetic streams
  (scope trials, owner-retrieval trials, individual, agent-handoff,
  supersession and retrieval items, paired paraphrases and long-history
  stable facts), sized for 95% CIs of about +/-11 points. Replay prints
  paraphrase agreement and unchanged-fact accuracy by history length. `datasets/generated/trust-seed-0/`.

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

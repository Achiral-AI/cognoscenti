# Changelog

All notable changes to Cognoscenti will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

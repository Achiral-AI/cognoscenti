# Contributing to Cognoscenti

Thank you for your interest in contributing to Cognoscenti! We welcome contributions from the community and are excited to have you help us improve this benchmark for cognitive memory systems.

## Getting Started

### Prerequisites

- Rust 1.70 or later
- Cargo (comes with Rust)
- Git

### Setting Up the Development Environment

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/your-username/cognoscenti.git
   cd cognoscenti
   ```
3. Create a new branch for your feature or bugfix:
   ```bash
   git checkout -b feature/your-feature-name
   ```
4. Install dependencies and build:
   ```bash
   cargo build
   cargo test
   ```

## Development Workflow

### Making Changes

1. Write clear, concise commit messages following [Conventional Commits](https://www.conventionalcommits.org/)
2. Ensure your code passes all tests:
   ```bash
   cargo test
   cargo clippy
   cargo fmt
   ```
3. Add tests for new functionality
4. Update documentation as needed

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Use `cargo clippy` to catch common mistakes
- Write documentation for public APIs
- Keep functions focused and small
- Add meaningful comments for complex logic

### Testing

- Write unit tests for new functionality
- Ensure all existing tests pass
- Test edge cases and error conditions
- Maintain test coverage above 80%

## Submitting Changes

### Pull Request Process

1. Ensure your branch is up to date with the main branch
2. Push your changes to your fork
3. Create a pull request with:
   - Clear title and description
   - Reference to related issues
   - Explanation of changes
   - Screenshots for UI changes (if applicable)

### Pull Request Checklist

- [ ] Code follows the project's style guidelines
- [ ] Tests pass locally
- [ ] Documentation is updated
- [ ] Commit messages are clear
- [ ] PR description explains the "why" not just the "what"

## Types of Contributions

We welcome the following types of contributions:

- **Bug fixes**: Help us fix issues
- **New features**: Propose and implement new benchmark dimensions
- **Documentation**: Improve README, API docs, or examples
- **Performance improvements**: Optimize existing code
- **Test coverage**: Add tests for uncovered code paths

## Reporting Issues

When reporting issues, please include:

- Clear description of the problem
- Steps to reproduce
- Expected behavior
- Actual behavior
- Environment details (OS, Rust version)
- Relevant logs or error messages

## Community Guidelines

- Be respectful and inclusive
- Provide constructive feedback
- Focus on what is best for the community
- Show empathy towards other community members

## Licensing

By contributing to Cognoscenti, you agree that your contributions will be licensed under the Apache License, Version 2.0.

## Questions?

Feel free to open an issue for questions or discussions about contributions.

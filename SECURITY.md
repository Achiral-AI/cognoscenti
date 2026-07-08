# Security Policy

## Supported Versions

Currently, only the latest version of Cognoscenti is supported with security updates.

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in Cognoscenti, please report it to us responsibly.

### How to Report

**Please do NOT report security vulnerabilities through public GitHub issues.**

Instead, please send an email to [INSERT SECURITY EMAIL]. Your email should include:

- A description of the vulnerability
- Steps to reproduce the vulnerability
- Potential impact of the vulnerability
- Any suggested fixes (if available)

### What to Expect

- We will acknowledge receipt of your report within 48 hours
- We will provide a detailed response within 7 days indicating the next steps
- We will work with you to understand and validate the report
- We will coordinate a fix and release plan with you

### Disclosure Policy

We follow responsible disclosure practices:

1. We will confirm the vulnerability and determine its severity
2. We will develop a fix and coordinate a release timeline
3. We will request that you keep the vulnerability confidential until the fix is released
4. We will publicly disclose the vulnerability after the fix is released
5. We will credit you in the release notes (unless you prefer to remain anonymous)

## Security Best Practices

When using Cognoscenti, consider the following security best practices:

- Keep dependencies up to date
- Review and validate benchmark data before use
- Be cautious when running benchmarks on untrusted data
- Use the latest stable version of Cognoscenti

## Dependency Security

We regularly update dependencies to address security vulnerabilities. We use:

- `cargo audit` to check for known security vulnerabilities in dependencies
- Automated dependency updates where possible
- Manual review of security advisories for Rust crates

## Private Key Management

Cognoscenti does not handle private keys or sensitive credentials. If you extend the tool to handle such data, ensure proper key management practices are followed.

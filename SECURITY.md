# Security Policy

## Supported versions

Woop is in early development. Security fixes are applied to the latest release only.

## Reporting a vulnerability

Please do not report security issues in public issues or discussions.

Use GitHub's [private vulnerability reporting](https://github.com/GonzaEC/woop/security/advisories/new) instead. Include a description of the issue, steps to reproduce and the affected platforms. You will receive an acknowledgement within a few days, and a fix or mitigation plan once the report has been assessed.

## How Woop protects you

Security is Woop's first priority ([ADR 0012](docs/adr/0012-security-tests-performance-priorities.md)). Among the measures in place:

- Output from programs is treated as untrusted ([ADR 0013](docs/adr/0013-bound-the-cost-of-program-output.md)):
  - programs cannot read or write the clipboard;
  - output waits in a bounded queue, so a program that floods the terminal slows itself down instead of exhausting memory or freezing the window;
  - shell integration marks cost bounded work, whatever a program prints;
  - escape sequences that never end are cut off, and output can never be held back from the screen;
  - window titles are limited in length and stripped of control and invisible formatting characters.
- Pasted text cannot carry escape sequences that end bracketed paste.
- Shell integration files are private to the user on Unix.
- Woop's own code contains no `unsafe`; the compiler rejects it across the workspace.
- Every pull request, and a weekly scheduled run, checks all dependencies (direct and transitive) against the RustSec advisory database for vulnerabilities, unsound code, unmaintained crates and yanked versions, and checks their licences and sources, with the policy in [`deny.toml`](deny.toml). Accepted exceptions are listed there, each with its reason; today they all come from upstream crates that Woop cannot update on its own.
- Dependency and CI updates are proposed automatically after a one-week waiting period. Security fixes for dependencies are proposed as soon as GitHub's advisory database lists the issue and a compatible fixed version exists.
- CI runs with a read-only token and no secrets. Third-party CI actions are pinned to exact commits, although some download tools when they run.
- Changes reach `main` only through pull requests that pass the required CI checks on Windows, macOS and Linux.

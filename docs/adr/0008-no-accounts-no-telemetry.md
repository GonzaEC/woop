# 0008. No accounts, no telemetry, optional bring-your-own AI

- **Status:** Accepted
- **Date:** 2026-11-21

## Context

A terminal sees everything a developer types and runs: credentials, internal hostnames, source code. Requiring sign-in or collecting usage data adds friction, breaks offline use and asks users to trust a remote service with sensitive information. At the same time, AI assistance is useful for many users and should be available to those who want it.

## Decision

- Woop will never require an account or sign-in to use any feature.
- Woop will not collect telemetry or send data anywhere unless the user explicitly configures a feature that needs it.
- AI features live in `woop-ai` behind a provider interface and are **disabled by default**. Users enable them by configuring their own provider: an API key for a hosted model, or a local model endpoint. API keys are stored in the operating system's credential store, never in plain-text configuration.
- Any feature that sends terminal content to a remote service must make that visible to the user before sending.

## Alternatives considered

- **Optional anonymous telemetry.** Useful for prioritising work, but erodes trust and requires infrastructure; feedback will come from issues and discussions instead.
- **A hosted AI service operated by the project.** Requires accounts, billing and data handling, contradicting the core principle.

## Consequences

- Woop works fully offline and is safe to use in restricted environments.
- There is no usage data to guide decisions; the project relies on community feedback.
- Supporting several AI providers requires a stable abstraction and per-provider maintenance.

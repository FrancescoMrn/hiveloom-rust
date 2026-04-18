# Changelog

All notable changes to Hiveloom are documented in this file.

## [0.1.1] - 2026-04-18

### Added

- Added a Slack workspace installation flow with `GET /slack/install` and
  `GET /slack/oauth/callback` so operators can authorize a real Slack app
  against a tenant without manually copying tokens into the environment.
- Added `GET /slack/setup`, an operator-facing readiness endpoint that reports
  the observed public base URL, Slack events URL, install URL, HTTPS status,
  and whether real Slack delivery is fully configured.
- Added support for resolving Slack access tokens from tenant credentials first
  and environment configuration second, which allows per-tenant installs while
  preserving local-development fallbacks.

### Changed

- Reworked Slack server configuration to prefer `SLACK_ACCESS_TOKEN`, while
  still accepting the legacy `SLACK_BOT_TOKEN` and `SLACK_USER_TOKEN`
  variables.
- Classified Slack token kinds (`xoxb`, `xoxp`, `xapp`) so setup diagnostics
  and operator logs can explain whether the current configuration uses a bot,
  user, or app token.
- Updated Slack event dispatch so outbound replies use the resolved tenant
  access token instead of a single process-wide bot token.

### Documentation

- Expanded the local testing guide with a full Slack workspace setup flow,
  including the redirect URL, install URL, events URL, and channel-binding
  steps for real delivery.

## [0.1.0] - 2026-04-18

### Initial implementation

- Built the initial multi-tenant Hiveloom platform foundation: tenant-aware
  stores, vault-backed credentials, admin APIs, runtime health checks, and the
  core serve lifecycle.
- Implemented the first Slack delivery path so Slack events can enter the
  platform, dispatch into agents, and post responses back into channels.
- Added scheduler, events, workflow, OAuth, and MCP foundations to support
  richer agent automation and external integrations.
- Completed the context compaction feature to keep long-running conversations
  manageable.
- Implemented per-bot MCP credentials and spec-compliant OAuth 2.0 support
  (spec 003).
- Replaced per-capability MCP exposure with the MCP chat layer and the
  `chat`, `memory`, and `list_conversations` tool model (spec 004).
- Added the first-time `/setup` wizard plus the major interactive CLI overhaul
  for chat, markdown skills, and the TUI experience (spec 005).
- Added the menu-driven interactive CLI redesign and corresponding CLI
  documentation refresh (spec 006).
- Hardened deployment with TLS helpers, install flags, and `bin.hiveloom.cloud`
  release/distribution support (spec 007).

### Release and project polish

- Added release automation, builder/uploader scripts, version checks, and R2
  verification in CI/CD for shipping public releases.
- Prepared the repository for open-source release with refreshed project
  metadata and contributor-facing documentation for architecture, security, and
  MCP setup.

# Changelog

All notable changes to Hiveloom are documented in this file.

## [0.2.0] - 2026-04-27

### Added

- Routed scheduled jobs and inbound event subscriptions through the real agent loop so automations can use the same memory, skills, tools, and persistence path as chat.
- Added guarded autonomous memory curation with configurable cadence through `HIVELOOM_MEMORY_CURATION_INTERVAL_TURNS`.
- Added progressive markdown skill loading: prompts include skill summaries by default, and full markdown skill bodies load on demand through the internal `hiveloom_load_skill` tool.
- Added OpenAI-compatible endpoint support through `HIVELOOM_OPENAI_BASE_URL` and `HIVELOOM_OPENAI_COMPAT_BASE_URL`.
- Added CLI support for replacing markdown capability content with `hiveloom capability edit --from-file`.

### Changed

- Improved Anthropic message handling by merging multiple system messages and mapping unknown roles into user messages for API compatibility.
- Replayed persisted tool results as user-visible context so continued sessions keep important tool output.
- Started the scheduler from `hiveloom serve` by default, with `--no-scheduler` available for deployments that need to disable it.
- Normalized CLI help text for agent references to clarify that agent IDs or names are accepted.
- Updated the README quick start to match the current credential and agent creation commands.

### Fixed

- Fixed capability `show`, `edit`, and `remove` so the CLI can use either capability IDs or names while the server resolves them within the selected agent scope.
- Preserved existing capability schemas and markdown bodies when editing only selected fields.
- Routed Slack events through the vault-backed agent loop instead of a separate response path.

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

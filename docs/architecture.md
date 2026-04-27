# Architecture

This document orients contributors in the codebase. The focus is *how the pieces
fit together* — not API contracts, which belong in the hosted docs and the
`--help` output.

## One binary, many roles

The `hiveloom` binary is both a long-running HTTP service and a CLI. `main.rs`
routes to `cli::interactive` on a bare invocation with a TTY, otherwise to
`cli::dispatch` for subcommand handling. There is no separate server binary.

## Storage: two-tier SQLite

All persistent state lives in two tiers under `--data-dir`:

- **Platform store** — `<data-dir>/platform.db`. Shared across tenants. Holds
  tenants, routing, platform admin tokens, and MCP OAuth clients.
  (`src/store/platform.rs`)
- **Tenant stores** — `<data-dir>/tenants/<tenant-uuid>/store.db`. One isolated
  SQLite database per tenant. Holds agents, capabilities, conversations, turns,
  memory, credentials, dedup entries, scheduled jobs, event subscriptions, OAuth
  requests, MCP identities, reflection reports, and compaction state.
  (`src/store/tenant.rs`)

Tenant resolution on incoming requests: the admin API's `resolve_tenant_id()`
accepts a slug or UUID, looks it up in the platform store via
`Tenant::get_by_slug()`, and `AppState::open_tenant_store()` opens the
corresponding tenant DB. Migrations live under `src/store/migrations/`, split by
tier.

## Domain model

Found under `src/store/models/`:

- **Tenant** (platform) — slug, name, timezone, status (active/disabled/deleted).
- **Agent** (tenant) — system prompt, model id, scope mode
  (`dual` / `tenant-only` / `user-only`), reflection settings, version history.
- **Capability** (agent) — a tool the agent may call. Has `auth_type`
  (`none` / `api_key` / `oauth` / `markdown`), endpoint URL, and optional JSON
  input/output schemas. `markdown` capabilities are skills (prompt-only).
- **Credential** (tenant) — encrypted secret for capability auth. Either a static
  token or an OAuth-delegated user token. Scoped to a capability/provider.
- **McpIdentity** (tenant) — an MCP client identity, optionally bound to a single
  agent and mapped to a human via `mapped_person_id`.

## Request surfaces

Four HTTP entry points, all served by the same Axum router
(`src/server/mod.rs`):

- **Admin API** (`src/server/admin_api/`) — REST endpoints for every CRUD
  operation. Bearer-token authenticated. **The CLI is a client of this API**,
  not a direct SQLite consumer.
- **MCP endpoint** (`src/server/mcp/`) — JSON-RPC 2.0 at
  `/mcp/:tenant_slug/:agent_slug`. Accepts MCP clients (Claude Desktop, Cursor,
  etc.) and dispatches into the agent loop.
- **OAuth 2.0 server** (`src/server/oauth/`) — standard authorize/token/callback/
  register endpoints plus protected-resource metadata. Used when a capability
  requires delegated user auth.
- **Slack adapter** (`src/server/slack/`) — `/slack/events` webhook with
  HMAC-SHA256 signature verification, routes Slack `message_event`s through the
  engine's chat surface.

## The engine

`src/engine/agent_loop.rs` is the core. One invocation:

1. Append the user message as a conversation turn.
2. Load memory entries, scoped per the agent's `scope_mode`.
3. `CompactionEngine::check_and_compact()` compresses history when token usage
   crosses the configured threshold.
4. Build the LLM input: system prompt → compaction summary (if any) →
   conversation turns.
5. Build tool definitions from capabilities, excluding `markdown` skills.
6. Tool-call loop (cap 10 iterations): call provider → execute tool calls via
   `CapabilityExecution` → log results → repeat until a text response.
7. Return text + list of tools called.

Supporting modules plug in around this loop:

| Module | Role |
|---|---|
| `capability_exec.rs` | Executes tool calls; handles OAuth token refresh and scope validation |
| `memory.rs` | Reads/writes scoped memory with coercion and expiry policies |
| `reflection.rs` | Post-conversation analysis over capability logs and memory stats |
| `scheduler.rs` | Background task scanning all tenants for due `ScheduledJob`s (cron or one-time) |
| `workflow.rs` | Pauses/resumes multi-step workflows by serializing into `conversations.workflow_state` |
| `event_router.rs` | Routes inbound events from `/events/:tid/inbound` to matching `EventSubscription`s |

## LLM providers

`src/llm/provider.rs` defines the `LlmProvider` trait with two methods:
`chat_complete(messages, tools) -> LlmResponse` and `count_tokens(text)`.

- `anthropic.rs` uses the Messages API (v2023-06-01); system prompt goes
  top-level, tool calls come from response content blocks.
- `openai.rs` uses Chat Completions and accepts custom base URLs for
  OpenAI-compatible endpoints (vLLM, Ollama, Together AI). Set
  `HIVELOOM_OPENAI_BASE_URL` to override the default `https://api.openai.com/v1`
  endpoint.

Resolution in `llm/mod.rs` picks the provider by `model_id` prefix:
`claude-*` → Anthropic, everything else → OpenAI-compatible.

Token counting on both sides uses `tiktoken_rs::cl100k_base`.

## CLI ↔ service boundary

The CLI **always goes over HTTP** to the admin API. It never touches SQLite
directly. `src/cli/client.rs::ApiClient` wraps `reqwest` with a base URL and
Bearer token. Endpoint discovery (in order):

1. `$HIVELOOM_ENDPOINT`
2. `<data-dir>/run/endpoint`
3. `<data-dir>/config.json`
4. Fallback: `http://127.0.0.1:3000`

This means the CLI works identically against a local dev service and a remote
production one — just set `--endpoint` and `--token`.

## Data directory layout

After `hiveloom serve --data-dir <path>`:

```
<data-dir>/
├── platform.db             # Platform store
├── config.json             # Local config (endpoint, default tenant, host, port)
├── run/
│   ├── endpoint            # "http://host:port" — used by CLI auto-discovery
│   ├── service.json        # {pid, endpoint, host, port, data_dir}
│   └── service.pid
├── logs/
├── backups/
├── manifests/              # YAML manifests applied via `hiveloom apply`
└── tenants/
    └── <tenant-uuid>/
        └── store.db        # Per-tenant database
```

`src/cli/local.rs::write_local_config()` creates these on service startup so a
sibling CLI invocation on the same machine can find the service without
configuration.

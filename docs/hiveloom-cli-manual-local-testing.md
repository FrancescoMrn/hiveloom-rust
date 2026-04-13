# HiveLoom CLI — Manual Local Testing

This guide walks through a real local operator flow using the repo-local
`.hiveloom` directory, and it also shows how to expose an agent over MCP for
use from outside the VPS.

---

## Important current behavior

- There is currently no first-class `hiveloom chat` or `hiveloom agent chat`
  command. After you create an agent, you interact with it through a bound
  surface (Slack) or through the MCP HTTP surface.
- The current MCP implementation is an **authenticated MCP tool surface**:
  `initialize`, `tools/list`, and `tools/call`.
- It is **not yet** a generic "chat with the agent over MCP" transport.
  A future `hiveloom agent chat <agent>` command would let you talk to an
  agent directly from the CLI.

---

## What this uses

| Item              | Value                                       |
|-------------------|---------------------------------------------|
| Binary            | `./hiveloom/target/debug/hiveloom`          |
| Local data dir    | `./.hiveloom` (auto-detected from cwd)      |
| Default endpoint  | `http://127.0.0.1:3000`                     |
| Install URL       | `https://get.hiveloom.cloud`                |

If `.hiveloom` exists in the current working directory, the CLI uses it
automatically for local testing.

---

## 0. Setup Rust

```bash
rustup toolchain install 1.94.0
rustup override set 1.94.0
cargo --version
```

## 1. Build

```bash
cd /root/github/hiveloom-app/hiveloom
cargo build
```

## 2. Start the local service

From the repo root:

```bash
cd /root/github/hiveloom-app
./hiveloom/target/debug/hiveloom serve
```

Expected behavior:

- `.hiveloom/config.json` is created
- `.hiveloom/run/service.json` and `.hiveloom/run/service.pid` are created
- `.hiveloom/master.key` is created (AES-256-GCM key, 0600 permissions)
- `.hiveloom/platform.db` is created (SQLite, WAL mode)
- The default tenant is auto-provisioned (FR-032)

In another terminal:

```bash
cd /root/github/hiveloom-app
./hiveloom/target/debug/hiveloom health
./hiveloom/target/debug/hiveloom status
./hiveloom/target/debug/hiveloom tenant list --json
```

## 3. Seed a local test agent

```bash
cd /root/github/hiveloom-app

export ANTHROPIC_API_KEY='your-real-or-test-key'

./hiveloom/target/debug/hiveloom credential set anthropic-key \
  --kind static \
  --from-env ANTHROPIC_API_KEY

./hiveloom/target/debug/hiveloom agent create \
  --name support-bot \
  --model claude-sonnet-4-5-20250514 \
  --system-prompt "You are a helpful support assistant for local testing." \
  --scope-mode dual

./hiveloom/target/debug/hiveloom capability add support-bot \
  --name search-kb \
  --description "Search the local knowledge base" \
  --cap-endpoint "https://kb.example.test/search" \
  --auth-type api_key \
  --credential-ref anthropic-key
```

Verify:

```bash
./hiveloom/target/debug/hiveloom agent list --json
./hiveloom/target/debug/hiveloom credential list --json
./hiveloom/target/debug/hiveloom capability list support-bot --json
./hiveloom/target/debug/hiveloom agent show support-bot
```

## 3a. How to interact with the agent you just created

Today there are two practical paths:

1. **Slack surface**: bind the agent to Slack and talk to it from Slack.
2. **MCP surface**: expose the agent over `/mcp/<tenant>/<agent>` and invoke
   it from an MCP client or from `curl`.

There is not yet a direct CLI chat session for an agent.

---

## 3b. MCP quick recipe (local or remote)

If your goal is "make this agent reachable over MCP", follow these steps.

### Step 1 — Start Hiveloom on the VPS

For local-only testing:

```bash
./hiveloom/target/debug/hiveloom serve
```

For remote access (bind to all interfaces):

```bash
./hiveloom/target/debug/hiveloom serve --host 0.0.0.0 --port 3000
```

### Step 2 — Put HTTPS in front (remote only)

Put Nginx or Caddy in front and point a public hostname at the VPS, for
example `https://loom.example.com`. See section 5c below for config examples.

### Step 3 — Create an MCP identity

```bash
./hiveloom/target/debug/hiveloom mcp-identity create \
  --tenant default \
  --name my-laptop
```

This prints the MCP identity ID.

### Step 4 — Get a setup code

```bash
./hiveloom/target/debug/hiveloom mcp-identity reissue-setup-code \
  <mcp-identity-id> \
  --tenant default
```

This prints a one-time setup code and its expiry.

### Step 5 — Exchange setup code for tokens

From your laptop or external client:

```bash
# Exchange setup code for an authorization code
curl -sS -X POST https://loom.example.com/mcp/authorize \
  -H 'Content-Type: application/json' \
  -d '{
    "setup_code": "<setup-code>",
    "tenant_slug": "default",
    "client_id": "manual-test-client"
  }' | jq

# Exchange authorization code for access + refresh tokens
curl -sS -X POST https://loom.example.com/mcp/token \
  -H 'Content-Type: application/json' \
  -d '{
    "grant_type": "authorization_code",
    "code": "<authorization-code>",
    "client_id": "manual-test-client"
  }' | jq
```

### Step 6 — Use the bearer token against your agent

```bash
# Initialize the MCP session
curl -sS -X POST https://loom.example.com/mcp/default/support-bot \
  -H "Authorization: Bearer <access-token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | jq

# List tools
curl -sS -X POST https://loom.example.com/mcp/default/support-bot \
  -H "Authorization: Bearer <access-token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | jq

# Call a tool
curl -sS -X POST https://loom.example.com/mcp/default/support-bot \
  -H "Authorization: Bearer <access-token>" \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc":"2.0","id":3,"method":"tools/call",
    "params":{"name":"search-kb","arguments":{"query":"test"}}
  }' | jq
```

If you only want to confirm the exposure is working, `tools/list` is the best
first check because it does not call any external endpoint.

---

## 4. Interactive mode

The interactive CLI is launched by running `hiveloom` with no subcommand:

```bash
cd /root/github/hiveloom-app
./hiveloom/target/debug/hiveloom
```

This opens a ratatui-based TUI shell with:

- A transcript pane showing command output
- A composer bar with Tab-completion for slash commands
- A suggestions strip showing ranked matches as you type

### Available interactive commands

| Command          | Description                                                     |
|------------------|-----------------------------------------------------------------|
| `/start`         | Launch the local Hiveloom service in the background             |
| `/health`        | Check whether the local instance responds on `/healthz`         |
| `/status`        | Summarize tenants, agents, credentials, backups, endpoint state |
| `/agents`        | List the current agents in the default tenant                   |
| `/credentials`   | List stored provider credentials (names only, never values)     |
| `/backups`       | List backup archives recorded for the local instance            |
| `/doctor`        | Run local filesystem and store checks against the data dir      |
| `/create-agent`  | Show the next recommended agent command based on current setup  |
| `/top`           | Open `hiveloom top` for the live terminal dashboard             |
| `/help`          | Show command examples and launcher shortcuts                    |
| `/quit`          | Leave the interactive CLI                                       |

### How input resolution works

- Typing `/health` executes the health command directly.
- Typing `sta` and pressing Tab completes to `/start` (or `/status` — use
  Up/Down to select from the suggestions strip).
- Typing a plain-text phrase like "show agents" is fuzzy-matched to the
  closest command (in this case `/agents`).
- If the match is uncertain, the shell says "interpreted as /agents" so you
  know what ran.
- There are **no single-letter shortcuts**. Every command uses its full name.

### Key bindings

| Key       | Action                              |
|-----------|-------------------------------------|
| Tab       | Autocomplete to the top suggestion  |
| Up/Down   | Cycle through suggestions           |
| Enter     | Execute the current input           |
| Esc       | Clear input, or quit if empty       |
| Ctrl-C    | Quit                                |
| Ctrl-L    | Clear transcript                    |

---

## 5. Test scheduling

Standard 5-field cron is accepted:

```bash
./hiveloom/target/debug/hiveloom schedule create support-bot \
  --cron "0 7 * * 1-5" \
  --timezone "America/New_York" \
  --context "Check the inbox and post a summary to #daily-digest"
```

One-time schedule also works:

```bash
./hiveloom/target/debug/hiveloom schedule create support-bot \
  --one-time-at "2026-04-14T07:00:00Z" \
  --timezone "UTC" \
  --context "Run a one-time digest"
```

Verify:

```bash
./hiveloom/target/debug/hiveloom schedule list support-bot --json
```

## 5a. Manual MCP flow with curl (detailed)

This is the easiest way to verify the agent is exposed over MCP even if you do
not yet have a separate MCP desktop client connected.

Assume:

- Tenant slug: `default`
- Agent name: `support-bot`
- External base URL: `https://loom.example.com` (or `http://127.0.0.1:3000` for local)
- Setup code from step 4: `<setup-code>`

Inspect MCP metadata:

```bash
curl -sS https://loom.example.com/.well-known/oauth-authorization-server | jq
curl -sS https://loom.example.com/mcp/default/.well-known/oauth-protected-resource | jq
```

Exchange the one-time setup code for an authorization code:

```bash
curl -sS -X POST https://loom.example.com/mcp/authorize \
  -H 'Content-Type: application/json' \
  -d '{
    "setup_code": "<setup-code>",
    "tenant_slug": "default",
    "client_id": "manual-test-client"
  }' | jq
```

Exchange that authorization code for access and refresh tokens:

```bash
curl -sS -X POST https://loom.example.com/mcp/token \
  -H 'Content-Type: application/json' \
  -d '{
    "grant_type": "authorization_code",
    "code": "<authorization-code>",
    "client_id": "manual-test-client"
  }' | jq
```

Use the returned bearer token:

```bash
# initialize
curl -sS -X POST https://loom.example.com/mcp/default/support-bot \
  -H "Authorization: Bearer <access-token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | jq

# tools/list
curl -sS -X POST https://loom.example.com/mcp/default/support-bot \
  -H "Authorization: Bearer <access-token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | jq

# tools/call
curl -sS -X POST https://loom.example.com/mcp/default/support-bot \
  -H "Authorization: Bearer <access-token>" \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc":"2.0","id":3,"method":"tools/call",
    "params":{"name":"echo-httpbin","arguments":{"message":"hello from mcp"}}
  }' | jq
```

Notes:

- `initialize`, `tools/list`, and `tools/call` are the three supported
  JSON-RPC methods on the MCP surface.
- Unauthenticated MCP requests return `401`.
- `tools/call` only succeeds if the underlying capability endpoint is real
  and reachable.
- If you only configured fake example endpoints, `tools/list` is the safest
  verification step.

## 5b. Add a simple MCP test tool

If you want a capability that can be exercised safely from MCP, add a simple
echo endpoint:

```bash
./hiveloom/target/debug/hiveloom capability add support-bot \
  --name echo-httpbin \
  --description "Echo request payload for MCP testing" \
  --cap-endpoint https://httpbin.org/anything \
  --auth-type none
```

Then the `tools/call` example above will return the echoed JSON payload.

## 5c. Expose MCP outside the VPS

For remote use, bind Hiveloom on the VPS and put TLS in front of it.

Start the service so it is reachable from the reverse proxy:

```bash
cd /root/github/hiveloom-app
./hiveloom/target/debug/hiveloom serve --host 0.0.0.0 --port 3000
```

Recommended production shape:

1. Run Hiveloom on `127.0.0.1:3000` or `0.0.0.0:3000`.
2. Put Nginx or Caddy in front of it on port 443.
3. Terminate TLS at the proxy.
4. Forward `Host` and `X-Forwarded-Proto` headers.
5. Only expose the proxy publicly.

Minimal Caddy example:

```caddy
loom.example.com {
    reverse_proxy 127.0.0.1:3000 {
        header_up Host {host}
        header_up X-Forwarded-Proto https
        header_up X-Forwarded-Host {host}
    }
}
```

Minimal Nginx example:

```nginx
server {
    listen 443 ssl http2;
    server_name loom.example.com;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header X-Forwarded-Proto https;
    }
}
```

Open the relevant firewall and security-group rules for port 443.

For a quick personal test from your laptop without exposing the service
publicly, use an SSH tunnel:

```bash
ssh -L 3000:127.0.0.1:3000 <user>@<your-vps>
```

Then test locally against `http://127.0.0.1:3000`.

---

## 6. Test compaction config

```bash
./hiveloom/target/debug/hiveloom agent compaction support-bot

./hiveloom/target/debug/hiveloom agent compaction support-bot \
  --threshold 70 \
  --protected-turns 6 \
  --show-indicator true

./hiveloom/target/debug/hiveloom compaction-log --json
```

## 7. Test auth tokens

```bash
./hiveloom/target/debug/hiveloom auth token-create \
  --scope platform:admin \
  --json

./hiveloom/target/debug/hiveloom auth token-list --json
```

## 8. Test export and backup

```bash
mkdir -p .hiveloom/manifests .hiveloom/backups

./hiveloom/target/debug/hiveloom agent export support-bot \
  > .hiveloom/manifests/support-bot.yaml

./hiveloom/target/debug/hiveloom backup create \
  --tenant default \
  --output default-backup.tar.gz \
  --json

./hiveloom/target/debug/hiveloom backup list --json
```

Restore from that backup:

```bash
./hiveloom/target/debug/hiveloom backup restore \
  --input /root/github/hiveloom-app/.hiveloom/backups/default-backup.tar.gz
```

---

## 9. Files you should see

Typical local files after the flow:

```text
.hiveloom/config.json
.hiveloom/run/service.json
.hiveloom/run/service.pid
.hiveloom/master.key
.hiveloom/platform.db
.hiveloom/tenants/<tenant-id>/store.db
.hiveloom/manifests/support-bot.yaml
.hiveloom/backups/default-backup.tar.gz
.hiveloom/backups/backup-index.json
.hiveloom/logs/service.log
```

---

## 10. Current limitations

| Limitation | Status | Workaround |
|------------|--------|------------|
| No `hiveloom agent chat` CLI command | Not yet built | Use MCP surface with `curl` or an MCP client |
| MCP exposes tools only, not conversational chat | By design for launch | `tools/list` + `tools/call` are the interface |
| Tenant ID vs slug resolution | Fixed; CLI sends slug, API resolves | Use `--tenant default` (slug) |
| MCP requires HTTPS for external MCP clients | OAuth spec requirement | Use Caddy/Nginx for TLS termination |
| Interactive mode requires a real TTY | By design (FR-049) | Use `ssh -t` when connecting remotely |

---

## 11. Notes

- Run commands from the repo root so `.hiveloom` auto-detection works.
- `backup restore` is safest when the service is stopped.
- The local CLI supports explicit remote use via `--endpoint` and `--token`.
- MCP metadata reflects the external host when your proxy forwards `Host`
  and `X-Forwarded-Proto`.
- The MCP endpoint requires a bearer token; unauthenticated requests return
  `401`.

---

## Appendix: interactive mode CLI changes

The interactive shell (`hiveloom` with no subcommand) was updated:

- **Full command names**: suggestions strip shows `/health`, `/status`,
  `/create-agent`, `/top`, etc. No single-letter shortcuts.
- **Tab completion**: completes to `/command` (e.g., typing `sta` + Tab
  yields `/start`), not a shorthand alias.
- **Help output**: `/help` lists full command names with descriptions. No
  one-letter aliases advertised.
- **Fuzzy matching**: typing plain text like "show agents" resolves to
  `/agents` with an "interpreted as" confirmation.
- **Startup hint**: the composer placeholder shows
  `try: /health, /status, /agents, /create-agent, /top`.

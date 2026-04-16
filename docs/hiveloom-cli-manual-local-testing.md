# HiveLoom CLI — User Guide

From zero to a working agent — installed, chatting, and accessible over MCP —
in a handful of commands.

---

## Contents

1. [Install](#1-install)
2. [5-minute quick start](#2-5-minute-quick-start)
3. [Interactive mode](#3-interactive-mode)
4. [Chat with your agent](#4-chat-with-your-agent)
5. [MCP setup (Claude Desktop, Cursor, etc.)](#5-mcp-setup)
6. [Markdown skills](#6-markdown-skills)
7. [All CLI commands](#7-all-cli-commands)
8. [Remote deployment](#8-remote-deployment)
9. [Files and config](#9-files-and-config)

---

## 1. Install

Build from source:

```bash
git clone https://github.com/FrancescoMrn/hiveloom-rust.git
cd hiveloom-rust
cargo build --release
```

The binary is at `./target/release/hiveloom`. Optionally symlink it:

```bash
sudo ln -s $(pwd)/target/release/hiveloom /usr/local/bin/hiveloom
```

---

## 2. 5-minute quick start

### Option A: Interactive wizard (recommended for first-time users)

```bash
hiveloom
```

This opens a menu-driven TUI. Select **Setup** and follow the 5 steps:
service → API key → agent → MCP → test chat.

### Option B: Four commands (for scripting / CI)

```bash
# 1. Start the service (in a separate terminal, or background it with & )
hiveloom serve

# 2. Store your LLM API key
echo "sk-ant-your-key-here" | hiveloom credential set anthropic

# 3. Create an agent
hiveloom agent create --name support-bot

# 4. Chat with it
hiveloom chat support-bot
```

That's it. You're chatting with your agent in 4 commands.

---

## 3. Interactive mode

Launch the TUI with no arguments:

```bash
hiveloom
```

### Main menu

```
╭──────────────────────────────────────────────────────╮
│  hiveloom                                            │
│  ● online   1 agents   1 credentials   default       │
╰──────────────────────────────────────────────────────╯

  ▸ Setup          Get started with guided setup     →
    Agents         Create and manage AI agents
    Chat           Talk to your agents
    Credentials    API keys and secrets
    MCP            External client access
    System         Health, backups, logs
```

### Categories

| Category       | Context panel    | Actions                              |
|----------------|------------------|--------------------------------------|
| **Setup**      | —                | 5-step onboarding wizard             |
| **Agents**     | Agent table      | Create, Add Skill, Export            |
| **Chat**       | —                | Opens chat with first agent          |
| **Credentials**| Credential list  | Set, Rotate, Remove                  |
| **MCP**        | Identity list    | Create Identity, Reissue Code        |
| **System**     | —                | Health, Status, Doctor, Backup, Logs |

### Key bindings

| Key         | Action                                         |
|-------------|------------------------------------------------|
| ↑ / ↓       | Navigate menu / context items                  |
| Enter       | Select item / submit form / send chat          |
| Tab         | Switch panel focus / next form field           |
| Esc         | Go back one level                              |
| `:`         | Open command bar (power users)                 |
| Ctrl-C      | Quit                                           |

### Inline forms

Create/edit operations render forms **within the TUI** — no need to exit to a
terminal. Fields include text inputs, masked inputs (for API keys), and
selection lists (for model choice).

### Command bar (power users)

Press `:` from any screen to open a vim-style command bar with autocomplete.
Type any CLI command (e.g., `agent list`, `credential set anthropic`).

---

## 4. Chat with your agent

### CLI chat command

```bash
hiveloom chat support-bot
```

A stdin/stdout conversation loop. Type messages, get responses. Conversation
context is maintained across messages. `Ctrl-C`, `Ctrl-D`, or `/exit` to quit.

### Interactive mode chat

From the TUI, select **Chat** from the main menu, or press Enter on an agent
in the Agents submenu and select **Chat** from the popup.

---

## 5. MCP setup

Connect external MCP clients (Claude Desktop, Cursor, etc.) to your agent.

### One command to get the MCP URL and setup code

```bash
hiveloom mcp-identity create --tenant default --name my-desktop --agent support-bot
```

Output:

```
Created MCP identity 'my-desktop' (abc123...)

  Setup code:  7f3ab2c19e4d0815...
  MCP URL:     http://127.0.0.1:3000/mcp/default/support-bot

  Add the URL to your MCP client (Claude Desktop, Cursor, etc.).
  Enter the setup code in the browser when prompted.
```

### Connect from Claude Desktop

1. Copy the MCP URL into Claude Desktop's MCP server settings
2. Claude Desktop discovers the OAuth endpoints automatically
3. When prompted, enter the setup code in the browser
4. Claude Desktop now shows 3 tools from your agent:
   - **chat** — send messages to the agent
   - **memory** — search stored memories
   - **list_conversations** — list prior conversations

### Issue a new setup code (if the first one was used or expired)

```bash
hiveloom mcp-identity reissue-setup-code <identity-id> --tenant default
```

Setup codes expire after 24 hours.

### Manual MCP flow with curl

For testing or non-standard clients:

```bash
# 1. OAuth discovery
curl -s http://127.0.0.1:3000/.well-known/oauth-authorization-server | jq
curl -s http://127.0.0.1:3000/mcp/default/support-bot/.well-known/oauth-protected-resource | jq

# 2. Register a client
curl -s -X POST http://127.0.0.1:3000/oauth/register \
  -H 'Content-Type: application/json' \
  -d '{
    "client_name": "curl-test",
    "redirect_uris": ["http://127.0.0.1:9999/callback"],
    "grant_types": ["authorization_code", "refresh_token"],
    "response_types": ["code"],
    "token_endpoint_auth_method": "client_secret_post"
  }' | jq

# 3. Authorize with setup code (returns 302 redirect with ?code=...)
curl -s -D - -X POST http://127.0.0.1:3000/oauth/authorize \
  -d "response_type=code&client_id=<client_id>&redirect_uri=http%3A%2F%2F127.0.0.1%3A9999%2Fcallback&state=test&code_challenge=<S256_challenge>&code_challenge_method=S256&scope=mcp&setup_code=<setup_code>"

# 4. Exchange authorization code for tokens
curl -s -X POST http://127.0.0.1:3000/oauth/token \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d "grant_type=authorization_code&code=<auth_code>&redirect_uri=http%3A%2F%2F127.0.0.1%3A9999%2Fcallback&client_id=<client_id>&client_secret=<client_secret>&code_verifier=<code_verifier>" | jq

# 5. Use the access token
curl -s -X POST http://127.0.0.1:3000/mcp/default/support-bot \
  -H "Authorization: Bearer <access_token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq
```

---

## 6. Markdown skills

Attach markdown files as agent knowledge — content is injected into the
agent's system prompt at invocation time, no external HTTP endpoint needed.

```bash
cat > product-faq.md << 'EOF'
# Product FAQ

## What is Hiveloom?
An open-source AI agent platform.

## Pricing
Free. Open source. Self-hosted.
EOF

hiveloom capability add support-bot \
  --name product-faq \
  --description "Product FAQ knowledge" \
  --from-file product-faq.md
```

The agent now uses this knowledge when answering questions.

---

## 7. All CLI commands

### Server

| Command | Description |
|---------|-------------|
| `hiveloom serve` | Start the HTTP service (default `127.0.0.1:3000`) |
| `hiveloom health` | Check if the service is responding |
| `hiveloom status` | Show service state and tenant summary |
| `hiveloom doctor` | Diagnose data directory and store integrity |

### Credentials

Secrets are never passed as CLI arguments. Use one of:

| Command | Description |
|---------|-------------|
| `hiveloom credential set <name> --from-env VAR` | Read from environment variable |
| `hiveloom credential set <name> --from-file PATH` | Read from file |
| `echo "secret" \| hiveloom credential set <name>` | Read from stdin |
| `hiveloom credential list` | List credentials (values never shown) |
| `hiveloom credential rotate <name>` | Replace the secret |
| `hiveloom credential remove <name>` | Delete the credential |

### Agents

| Command | Description |
|---------|-------------|
| `hiveloom agent create --name <name>` | Create a new agent |
| `hiveloom agent list` | List all agents in the tenant |
| `hiveloom agent show <id>` | Show agent details |
| `hiveloom agent edit <id>` | Edit name, model, system prompt |
| `hiveloom agent delete <id>` | Delete an agent |
| `hiveloom agent versions <id>` | List version history |
| `hiveloom agent rollback <id> --to-version N` | Roll back to version N |
| `hiveloom agent export <id>` | Export as YAML manifest |
| `hiveloom agent compaction <id>` | View or edit compaction settings |
| `hiveloom agent reflect <id>` | Trigger self-reflection |
| `hiveloom agent bind <id> --surface slack --channel <ch>` | Bind to a surface |

### Capabilities (tools + skills)

| Command | Description |
|---------|-------------|
| `hiveloom capability add <agent> --cap-endpoint URL` | Add HTTP capability |
| `hiveloom capability add <agent> --from-file PATH.md` | Add markdown skill |
| `hiveloom capability list <agent>` | List capabilities |
| `hiveloom capability show <agent> <name>` | Show details |
| `hiveloom capability edit <agent> <name>` | Edit capability |
| `hiveloom capability remove <agent> <name>` | Remove capability |

### Chat

| Command | Description |
|---------|-------------|
| `hiveloom chat <agent>` | Interactive stdin/stdout chat with an agent |
| `hiveloom` (interactive mode → Chat) | Chat within the TUI |

### MCP (external client access)

| Command | Description |
|---------|-------------|
| `hiveloom mcp-identity create --tenant T --name N --agent A` | Create identity, issues setup code |
| `hiveloom mcp-identity list --tenant T` | List identities |
| `hiveloom mcp-identity show <id> --tenant T` | Show details |
| `hiveloom mcp-identity reissue-setup-code <id> --tenant T` | Issue a new setup code |
| `hiveloom mcp-identity map <id> --tenant T --person-id P` | Map to a person |
| `hiveloom mcp-identity unmap <id> --tenant T` | Remove person mapping |
| `hiveloom mcp-identity revoke <id> --tenant T` | Revoke the identity |

### Scheduling

| Command | Description |
|---------|-------------|
| `hiveloom schedule create <agent> --cron "0 7 * * *"` | Create scheduled job |
| `hiveloom schedule create <agent> --one-time-at "2026-04-16T07:00:00Z"` | One-time job |
| `hiveloom schedule list <agent>` | List jobs |
| `hiveloom schedule pause <agent> <job>` | Pause a job |
| `hiveloom schedule resume <agent> <job>` | Resume a paused job |
| `hiveloom schedule delete <agent> <job>` | Delete a job |

### Events

| Command | Description |
|---------|-------------|
| `hiveloom event subscribe <agent> --event-type T --auth-token AT` | Subscribe to events |
| `hiveloom event list <agent>` | List subscriptions |
| `hiveloom event disable <agent> <sub>` | Disable subscription |
| `hiveloom event enable <agent> <sub>` | Re-enable subscription |
| `hiveloom event delete <agent> <sub>` | Delete subscription |

### Tenants

| Command | Description |
|---------|-------------|
| `hiveloom tenant list` | List all tenants |
| `hiveloom tenant create --name N --slug S` | Create a tenant |
| `hiveloom tenant show <id>` | Show tenant |
| `hiveloom tenant disable <id>` | Disable a tenant |
| `hiveloom tenant delete <id>` | Soft-delete |

### Auth tokens

| Command | Description |
|---------|-------------|
| `hiveloom auth token-create --scope platform:admin` | Create a bearer token |
| `hiveloom auth token-list` | List tokens |
| `hiveloom auth token-revoke <id>` | Revoke a token |

### Operations

| Command | Description |
|---------|-------------|
| `hiveloom logs` | View recent logs |
| `hiveloom tail` | Stream logs |
| `hiveloom top` | Live TUI dashboard |
| `hiveloom compaction-log` | View context compaction events |
| `hiveloom backup create --output FILE` | Create a backup |
| `hiveloom backup list` | List backups |
| `hiveloom backup restore --input FILE` | Restore from backup |
| `hiveloom apply -f manifest.yaml` | Apply an agent manifest |

### Global flags

Available on most commands:

| Flag | Description |
|------|-------------|
| `--tenant <slug>` | Tenant (default: `default`) |
| `--endpoint <url>` | API endpoint (default: auto-detected) |
| `--token <token>` | Bearer token for remote access |
| `--json` | Output as JSON instead of a human-readable table |

---

## 8. Remote deployment

For remote access, bind to all interfaces and put TLS in front:

```bash
hiveloom serve --host 0.0.0.0 --port 3000
```

### Minimal Caddy config

```caddy
loom.example.com {
    reverse_proxy 127.0.0.1:3000 {
        header_up Host {host}
        header_up X-Forwarded-Proto https
        header_up X-Forwarded-Host {host}
    }
}
```

### Minimal Nginx config

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

### Quick testing without public exposure

```bash
ssh -L 3000:127.0.0.1:3000 user@your-vps
```

Then connect to `http://127.0.0.1:3000` on your laptop.

---

## 9. Files and config

Data directory discovery priority:

1. `HIVELOOM_DATA_DIR` env var
2. `./.hiveloom/` in the current working directory
3. `~/.hiveloom/` in home directory
4. `/var/lib/hiveloom/` (system default)

Files created by `hiveloom serve`:

```text
<data-dir>/config.json              # Local config (endpoint, port)
<data-dir>/run/service.json         # Process info (pid, endpoint)
<data-dir>/run/service.pid          # Process ID
<data-dir>/run/endpoint             # Service URL for CLI auto-detection
<data-dir>/master.key               # AES-256 key for credential vault (chmod 600)
<data-dir>/platform.db              # Global platform store (SQLite)
<data-dir>/tenants/<uuid>/store.db  # Per-tenant store (SQLite, WAL mode)
<data-dir>/logs/service.log         # Service logs
<data-dir>/backups/                 # Backup archives
```

### Environment variables

| Variable | Purpose |
|----------|---------|
| `HIVELOOM_DATA_DIR` | Override data directory location |
| `HIVELOOM_ENDPOINT` | Override CLI API endpoint |
| `HIVELOOM_TENANT` | Override default tenant slug |
| `SLACK_SIGNING_SECRET` | Slack webhook signing secret (optional) |
| `SLACK_BOT_TOKEN` | Slack bot OAuth token (optional) |

# HiveLoom CLI — Manual Local Testing

This guide walks through a real local operator flow: install, configure, create
an agent, chat with it, and expose it over MCP for external clients.

---

## What this uses

| Item              | Value                                       |
|-------------------|---------------------------------------------|
| Binary            | `./target/release/hiveloom`                 |
| Local data dir    | `./.hiveloom` (auto-detected from cwd)      |
| Default endpoint  | `http://127.0.0.1:3000`                     |

If `.hiveloom` exists in the current working directory, the CLI uses it
automatically for local testing. You can also set `HIVELOOM_DATA_DIR` explicitly.

---

## 0. Build

```bash
cd /root/github/hiveloom-app/hiveloom
cargo build --release
```

## 1. Quick start with interactive mode

The fastest way to get going — the interactive shell has a menu-driven UI:

```bash
./target/release/hiveloom
```

On launch you see a main menu with 6 categories:

```
╭──────────────────────────────────────────────────────╮
│  hiveloom                                            │
│  ● online   0 agents   0 credentials   default       │
╰──────────────────────────────────────────────────────╯

  ▸ Setup          Get started with guided setup     →
    Agents         Create and manage AI agents
    Chat           Talk to your agents
    Credentials    API keys and secrets
    MCP            External client access
    System         Health, backups, logs
```

Navigate with arrow keys and Enter. On a fresh install, select **Setup** to
run the 5-step onboarding wizard:

1. **Service** — checks status, starts if offline
2. **API Key** — masked input for your Anthropic or OpenAI key
3. **Agent** — create your first agent (name, system prompt)
4. **MCP** — creates identity, shows MCP URL + setup code
5. **Test Chat** — sends a message and shows the agent's response

After setup, go to **Chat** to talk to your agent, or **Agents** to manage
agents with inline forms and context panels

---

## 2. Manual setup (non-interactive)

### Start the service

```bash
./target/release/hiveloom serve --host 127.0.0.1 --port 3000
```

In another terminal:

```bash
./target/release/hiveloom health
./target/release/hiveloom status
./target/release/hiveloom tenant list --json
```

### Store credentials

Secrets are never passed as CLI arguments. Use one of:

```bash
# From environment variable
export ANTHROPIC_API_KEY='sk-ant-...'
./target/release/hiveloom credential set anthropic --from-env ANTHROPIC_API_KEY

# From file
./target/release/hiveloom credential set anthropic --from-file /path/to/key

# From stdin
echo 'sk-ant-...' | ./target/release/hiveloom credential set anthropic
```

### Create an agent

```bash
./target/release/hiveloom agent create \
  --name support-bot \
  --model claude-sonnet-4-20250514 \
  --system-prompt "You are a helpful support assistant." \
  --scope-mode dual
```

### Add capabilities

HTTP endpoint capability:

```bash
./target/release/hiveloom capability add support-bot \
  --name echo-httpbin \
  --description "Echo request payload for testing" \
  --cap-endpoint https://httpbin.org/anything \
  --auth-type none
```

Markdown skill (knowledge injected into system prompt):

```bash
./target/release/hiveloom capability add support-bot \
  --name product-faq \
  --description "Product FAQ knowledge" \
  --from-file skills/product-faq.md
```

Verify:

```bash
./target/release/hiveloom agent list
./target/release/hiveloom credential list
./target/release/hiveloom capability list support-bot
```

---

## 3. Chat with the agent

### CLI chat command

```bash
./target/release/hiveloom chat support-bot
```

This starts a stdin/stdout conversation loop. Type messages, see responses.
Maintains conversation context across messages. Ctrl-C or `/exit` to quit.

### Interactive shell chat

From inside `hiveloom` interactive mode:

```
> chat support-bot
Chatting with support-bot. Type /exit or Esc to return.
you: What can you help me with?
support-bot: I can help you with...
```

---

## 4. MCP setup (for Claude Desktop, Cursor, etc.)

### Create MCP identity

```bash
./target/release/hiveloom mcp-identity create \
  --tenant default \
  --name my-desktop \
  --agent support-bot
```

### Get a setup code

```bash
./target/release/hiveloom mcp-identity reissue-setup-code <identity-id> \
  --tenant default
```

This prints a one-time setup code (valid 24 hours).

### Connect from Claude Desktop

1. Add the MCP server URL to Claude Desktop:
   ```
   http://127.0.0.1:3000/mcp/default/support-bot
   ```
2. Claude Desktop will discover the OAuth endpoints automatically
3. Enter the setup code when prompted in the browser
4. Once authorized, Claude Desktop connects and shows three tools:
   - **chat** — send messages to the agent
   - **memory** — search stored memories
   - **list_conversations** — list prior conversations

### Manual MCP flow with curl

Verify OAuth discovery:

```bash
curl -s http://127.0.0.1:3000/.well-known/oauth-authorization-server | jq
curl -s http://127.0.0.1:3000/mcp/default/support-bot/.well-known/oauth-protected-resource | jq
```

Register a client:

```bash
curl -s -X POST http://127.0.0.1:3000/oauth/register \
  -H 'Content-Type: application/json' \
  -d '{
    "client_name": "curl-test",
    "redirect_uris": ["http://127.0.0.1:9999/callback"],
    "grant_types": ["authorization_code", "refresh_token"],
    "response_types": ["code"],
    "token_endpoint_auth_method": "client_secret_post"
  }' | jq
```

Authorize (submit setup code — in practice, this happens in a browser):

```bash
curl -s -X POST http://127.0.0.1:3000/oauth/authorize \
  -d "response_type=code&client_id=<client_id>&redirect_uri=http%3A%2F%2F127.0.0.1%3A9999%2Fcallback&state=test&code_challenge=<challenge>&code_challenge_method=S256&scope=mcp&setup_code=<code>" \
  -D -
```

Exchange authorization code for tokens:

```bash
curl -s -X POST http://127.0.0.1:3000/oauth/token \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d "grant_type=authorization_code&code=<auth_code>&redirect_uri=http%3A%2F%2F127.0.0.1%3A9999%2Fcallback&client_id=<client_id>&client_secret=<client_secret>&code_verifier=<verifier>" | jq
```

Use the bearer token:

```bash
# Initialize
curl -s -X POST http://127.0.0.1:3000/mcp/default/support-bot \
  -H "Authorization: Bearer <access_token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | jq

# List tools (returns: chat, memory, list_conversations)
curl -s -X POST http://127.0.0.1:3000/mcp/default/support-bot \
  -H "Authorization: Bearer <access_token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | jq

# Chat with the agent (returns SSE stream)
curl -s -X POST http://127.0.0.1:3000/mcp/default/support-bot \
  -H "Authorization: Bearer <access_token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"chat","arguments":{"message":"Hello!"}}}'

# Search memory
curl -s -X POST http://127.0.0.1:3000/mcp/default/support-bot \
  -H "Authorization: Bearer <access_token>" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory","arguments":{"query":"preferences"}}}' | jq
```

---

## 5. Interactive mode

The interactive CLI is a menu-driven TUI launched by running `hiveloom` with
no subcommand:

```bash
./target/release/hiveloom
```

### Menu-driven navigation

The main screen shows 6 categories. Navigate with arrow keys, press Enter
to open a category. Each category has:

- **Left panel**: Available actions (Create, Add Skill, etc.)
- **Right panel**: Context table showing existing items (agents, credentials, etc.)
- **Tab**: Switch focus between action panel and context panel
- **Enter on context item**: Opens an action popup (Chat, Edit, Delete, etc.)

### Categories

| Category       | Context Panel           | Actions                              |
|----------------|-------------------------|--------------------------------------|
| **Setup**      | —                       | 5-step onboarding wizard             |
| **Agents**     | Agent table             | Create, Add Skill, Export            |
| **Chat**       | —                       | Opens chat with first agent          |
| **Credentials**| Credential list         | Set, Rotate, Remove                  |
| **MCP**        | Identity list           | Create Identity, Reissue Code        |
| **System**     | —                       | Health, Status, Doctor, Backup, Logs |

### Inline forms

Create/edit operations render forms within the TUI — no need to exit to a
terminal. Forms support text fields, masked input (for API keys), and
selection lists (for model choice).

### Command bar (power users)

Press `:` from any screen to open a vim-style command bar with autocomplete.
Type any CLI command (e.g., `agent list`, `credential set anthropic`).

### Key bindings

| Key       | Action                                    |
|-----------|-------------------------------------------|
| ↑ / ↓     | Navigate menu items / context panel rows  |
| Enter     | Select menu item / submit form / send chat |
| Tab       | Switch panel focus / next form field      |
| Esc       | Go back one level                         |
| `:`       | Open command bar                          |
| Ctrl-C    | Quit                                      |

---

## 6. Markdown skills

Agents can have markdown-based knowledge files that enrich their system prompt:

```bash
cat > skills/support-runbook.md << 'EOF'
# Support Runbook

## Password Reset
1. Verify customer identity
2. Send password reset link
3. Confirm reset within 15 minutes

## Pricing
- Starter: $29/mo
- Pro: $99/mo
- Annual discount: 20% off
EOF

./target/release/hiveloom capability add support-bot \
  --name support-runbook \
  --description "Internal support procedures" \
  --from-file skills/support-runbook.md
```

The markdown content is injected into the agent's system prompt at invocation
time. The agent uses this knowledge when answering questions — no external HTTP
endpoint required.

---

## 7. Other operations

### Scheduling

```bash
./target/release/hiveloom schedule create support-bot \
  --cron "0 7 * * 1-5" \
  --timezone "America/New_York" \
  --context "Check the inbox and post a summary"

./target/release/hiveloom schedule list support-bot
```

### Compaction

```bash
./target/release/hiveloom agent compaction support-bot
./target/release/hiveloom agent compaction support-bot --threshold 70
./target/release/hiveloom compaction-log --tenant default
```

### Auth tokens

```bash
./target/release/hiveloom auth token-create --scope platform:admin --json
./target/release/hiveloom auth token-list
```

### Export and backup

```bash
./target/release/hiveloom agent export support-bot > manifest.yaml
./target/release/hiveloom backup create --output backup.tar.gz
./target/release/hiveloom backup list
```

### Doctor (diagnostics)

```bash
./target/release/hiveloom doctor
```

---

## 8. Expose MCP outside the VPS

For remote access, bind to all interfaces and put TLS in front:

```bash
./target/release/hiveloom serve --host 0.0.0.0 --port 3000
```

Minimal Caddy config:

```caddy
loom.example.com {
    reverse_proxy 127.0.0.1:3000 {
        header_up Host {host}
        header_up X-Forwarded-Proto https
        header_up X-Forwarded-Host {host}
    }
}
```

For quick testing from your laptop without exposing publicly:

```bash
ssh -L 3000:127.0.0.1:3000 user@your-vps
```

---

## 9. Files after setup

```text
.hiveloom/config.json
.hiveloom/run/service.json
.hiveloom/run/service.pid
.hiveloom/run/endpoint
.hiveloom/master.key
.hiveloom/platform.db
.hiveloom/tenants/<tenant-id>/store.db
.hiveloom/logs/service.log
```

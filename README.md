# Hiveloom

A multi-tenant AI agent platform. One Rust binary, one SQLite file per tenant,
one CLI. Self-host it on a small VPS, manage it from the terminal, and expose
agents over HTTP and MCP to clients like Claude Desktop and Cursor.

> **Status:** early public release. The binary is stable enough to run your own
> agents, but APIs and CLI shapes may shift before 1.0.

## Install

Linux (x86_64 / aarch64) or macOS (Intel / Apple Silicon):

```bash
curl -fsSL https://bin.hiveloom.cloud/install.sh | bash
```

The script detects your platform, drops the `hiveloom` binary into
`/usr/local/bin`, and on Linux also creates a `hiveloom` system user and a
systemd unit (it does not start the service). Pin a version with
`-s -- --version 0.2.0`, skip the service with `-s -- --no-service`, or install
elsewhere with `-s -- --install-dir ~/.local/bin`.

Verify:

```bash
hiveloom --version
```

## Build from source

```bash
git clone https://github.com/FrancescoMrn/hiveloom-rust
cd hiveloom-rust
cargo build --release
./target/release/hiveloom --version
```

Requires a stable Rust toolchain (edition 2021).

## Quick start

```bash
# 1. Start the service (foreground, local SQLite at ./data/)
hiveloom serve --data-dir ./data

# 2. In another shell, store your LLM provider key
echo "$ANTHROPIC_API_KEY" | hiveloom credential add anthropic --from-stdin

# 3. Create an agent that uses it
hiveloom agent create my-agent \
  --provider anthropic \
  --model claude-sonnet-4-6 \
  --system "You are a terse helpful assistant."

# 4. Chat with it
hiveloom chat my-agent
```

For a full VPS-backed setup (TLS via Caddy, systemd, MCP clients), see
[docs.hiveloom.cloud](https://docs.hiveloom.cloud).

## CLI overview

`hiveloom` groups its subcommands roughly as follows. Each has `--help` with
full flag documentation.

| Group | Commands |
|---|---|
| Service & operator | `serve`, `health`, `status`, `doctor`, `logs`, `tail`, `top`, `tls` |
| Tenants & identity | `tenant`, `auth`, `mcp-identity` |
| Agents & config | `agent`, `capability`, `credential`, `chat`, `apply` |
| Scheduling & events | `schedule`, `event` |
| Backup & upgrade | `backup`, `upgrade`, `rollback`, `compaction-log` |

Global flags available on most commands:

| Flag | Meaning |
|---|---|
| `--tenant <slug>` | Target tenant (default: `default`) |
| `--endpoint <url>` | API endpoint (default: auto-detected) |
| `--token <token>` | Bearer token for remote access |
| `--json` | Emit JSON instead of a human-readable table |

Secrets are never passed as CLI args. Credential-accepting commands read from
`--from-env VAR`, `--from-file PATH`, or stdin.

## Docs

User-facing docs and the guided install-to-chat journey live at
[docs.hiveloom.cloud](https://docs.hiveloom.cloud). In this repo:

- [`docs/architecture.md`](docs/architecture.md) — how the binary, two-tier
  SQLite, engine, and HTTP surfaces fit together. Start here before touching
  `src/`.
- [`docs/claude-mcp-setup.md`](docs/claude-mcp-setup.md) — shortest path to HTTPS
  on your VPS so Claude Code / Desktop / Cursor can use it as an MCP connector.
  Uses a free Cloudflare Tunnel; no open ports, no cert renewal.
- [`docs/deployment-hardening.md`](docs/deployment-hardening.md) — advanced:
  self-hosted TLS via Caddy + Let's Encrypt, firewall rules, and systemd
  hardening. For operators who don't want Cloudflare in the path.
- [`docs/hiveloom-cli-manual-local-testing.md`](docs/hiveloom-cli-manual-local-testing.md) —
  walkthrough for exercising the CLI end-to-end on a local machine.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — build, test, commit, and PR conventions.
- [`SECURITY.md`](SECURITY.md) — how to report vulnerabilities privately.

For machine-readable docs, `https://docs.hiveloom.cloud/llms.txt` indexes every
page and `llms-full.txt` concatenates them.

## MCP clients

Hiveloom agents expose themselves over the Model Context Protocol, so any
MCP-capable client can use them as a tool source. Configured and documented
walkthroughs for Claude Desktop, Cursor, and raw MCP endpoints are in the hosted
docs.

## License

Apache-2.0. See [`Cargo.toml`](Cargo.toml) for crate metadata.

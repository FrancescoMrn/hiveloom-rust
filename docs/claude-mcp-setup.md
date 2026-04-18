# Connect your Hiveloom VPS to Claude (via MCP)

This is the short path to get a Hiveloom agent working as an MCP connector in
Claude Code, Claude Desktop, or any other MCP client. It uses
**[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/)**,
which gives you a free HTTPS URL with no open inbound ports on your VPS, no
certificate to renew, and no reverse proxy to configure.

Assumes you already have `hiveloom serve` running and an agent created. If
not, walk the [quick start in the README](../README.md#quick-start) first.

## Option A: throwaway URL (no domain required)

Good for "does this work at all" testing. The URL disappears when you stop the
command.

On the VPS:

```bash
# Install
curl -L https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 \
  -o /usr/local/bin/cloudflared
chmod +x /usr/local/bin/cloudflared

# Tunnel to hiveloom (default port 3000)
cloudflared tunnel --url http://127.0.0.1:3000
```

It prints a URL like `https://random-words.trycloudflare.com`. Your MCP
endpoint is now:

```
https://random-words.trycloudflare.com/mcp/<tenant-slug>/<agent-slug>
```

Leave the command running. If you close the terminal, the URL dies.

## Option B: your own domain (persistent)

This is the setup you actually want for daily use.

### 1. Requirements

- A domain added to a free Cloudflare account (change its nameservers to
  Cloudflare's — they walk you through this when you add the domain).
- `cloudflared` installed on the VPS (see Option A for the install command).

### 2. Authenticate

On the VPS:

```bash
cloudflared tunnel login
```

Open the printed URL in a browser, pick your domain. `cloudflared` writes a
certificate to `~/.cloudflared/cert.pem`.

### 3. Create a named tunnel

```bash
cloudflared tunnel create hiveloom
```

Note the tunnel UUID — you'll need it next.

### 4. Point your hostname at the tunnel

```bash
cloudflared tunnel route dns hiveloom hiveloom.yourdomain.com
```

Cloudflare creates a `CNAME` from `hiveloom.yourdomain.com` to
`<tunnel-uuid>.cfargotunnel.com`. HTTPS is handled automatically.

### 5. Configure the tunnel

Create `/etc/cloudflared/config.yml`:

```yaml
tunnel: hiveloom
credentials-file: /root/.cloudflared/<tunnel-uuid>.json

ingress:
  - hostname: hiveloom.yourdomain.com
    service: http://127.0.0.1:3000
  - service: http_status:404
```

### 6. Install as a system service

```bash
sudo cloudflared service install
sudo systemctl enable --now cloudflared
```

Your MCP endpoint is now:

```
https://hiveloom.yourdomain.com/mcp/<tenant-slug>/<agent-slug>
```

## Point Claude at the MCP endpoint

Generate MCP credentials for the agent:

```bash
hiveloom mcp-identity create \
  --agent <agent-slug> \
  --label "claude-code"
```

The command prints a bearer token. In Claude Code:

1. Open **Settings → Connectors → Add MCP connector**.
2. URL: `https://hiveloom.yourdomain.com/mcp/<tenant-slug>/<agent-slug>`
3. Authorization: `Bearer <token-from-above>`
4. Save and enable it.

Claude Desktop and Cursor use the same URL + bearer pattern — look up their MCP
connector settings for the exact UI.

## Troubleshooting

- **`cloudflared` can't reach the service.** The service must be listening on
  `127.0.0.1:3000` (or wherever your `config.yml` points). Check with
  `curl http://127.0.0.1:3000/healthz` on the VPS.
- **HTTP 502 from Cloudflare.** The tunnel is up but Hiveloom isn't responding.
  `systemctl status hiveloom` and `journalctl -u hiveloom -f` will tell you why.
- **MCP client says "unauthorized".** The bearer token doesn't match any
  `mcp-identity` on that agent. Regenerate with `hiveloom mcp-identity create`.
- **I want HTTPS without Cloudflare.** See
  [`docs/deployment-hardening.md`](deployment-hardening.md) for the full Caddy +
  Let's Encrypt + firewall path.

## What this does and doesn't buy you

**Does:**

- Browser-valid HTTPS on your hostname.
- No open inbound port on the VPS (tunnel is outbound-only).
- DDoS mitigation and WAF from Cloudflare.

**Doesn't:**

- End-to-end encryption from client to origin (Cloudflare terminates TLS and
  re-opens it to your VPS). Fine for MCP; not fine for regulated data.
- Hide your VPS IP from Cloudflare themselves.
- Replace the hardening steps (systemd user, data-dir perms, backup strategy)
  covered in the advanced guide.

# Deployment Hardening: HTTPS, DNS, Firewall

This page is the recipe for taking a plain `hiveloom serve` on a VPS and turning it into a
production-ish deployment that:

- is reachable over `https://<your-host>` with an automatically-renewed Let's Encrypt cert,
- does not expose its admin / MCP port on the public internet, and
- only keeps the ports open that are actually needed.

**Hiveloom does not install anything for you.** The one thing Hiveloom provides is a CLI
command that prints a ready-to-use Caddyfile for your hostname — the rest is on you
(install Caddy, point your DNS, apply firewall rules). The recipe below is the short
version.

---

## What Hiveloom contributes

```bash
hiveloom tls render --host hiveloom.example.com --email you@example.com
```

Prints a Caddyfile to stdout. That is the entire built-in tooling for this feature. Pipe
it where you want it:

```bash
hiveloom tls render --host hiveloom.example.com --email you@example.com \
  | sudo tee /etc/caddy/Caddyfile.d/hiveloom.caddy
```

Everything else below is operator action.

---

## 1. Point your DNS at the VPS

Create an `A` record (and optionally `AAAA`) for the hostname you want to use, pointing
at your VPS's public IP(s). Wait for propagation. Verify with:

```bash
dig +short hiveloom.example.com
# Must print your VPS public IP
```

If this does not match, certificate issuance will fail. Hiveloom does not check DNS for
you — you are responsible for this step.

---

## 2. Install Caddy

Caddy terminates TLS, gets a free Let's Encrypt certificate, and proxies traffic to
Hiveloom running on localhost. Installation is one command on Debian/Ubuntu:

```bash
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
  | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
  | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update
sudo apt install -y caddy
```

For other distros, see <https://caddyserver.com/docs/install>.

Confirm:

```bash
caddy version        # should print v2.x
systemctl is-active caddy   # should print "active"
```

---

## 3. Rebind Hiveloom to loopback

You do not want Caddy *and* Hiveloom both listening on `:3000`, and you do not want
Hiveloom's port reachable from the public internet once Caddy sits in front.

If you used `scripts/install.sh`, edit `/etc/systemd/system/hiveloom.service` and change
the `ExecStart=` line to bind to `127.0.0.1` only:

```ini
ExecStart=/usr/local/bin/hiveloom serve --host 127.0.0.1 --port 3000 --data-dir /var/lib/hiveloom
```

Reload systemd and restart Hiveloom:

```bash
sudo systemctl daemon-reload
sudo systemctl restart hiveloom
ss -tlnp | grep :3000
# Must show "127.0.0.1:3000", not "0.0.0.0:3000"
```

---

## 4. Drop in the Caddyfile

```bash
sudo mkdir -p /etc/caddy/Caddyfile.d
hiveloom tls render --host hiveloom.example.com --email you@example.com \
  | sudo tee /etc/caddy/Caddyfile.d/hiveloom.caddy
sudo systemctl reload caddy
```

The first request to `https://hiveloom.example.com/` will trigger Caddy's ACME flow.
Give it 10–30 seconds, then verify:

```bash
curl -s https://hiveloom.example.com/healthz
# {"status":"ok"}
curl -s https://hiveloom.example.com/.well-known/oauth-authorization-server | jq .issuer
# "https://hiveloom.example.com"
```

The OAuth metadata URLs must start with `https://`. If they start with `http://`, the
proxy is not forwarding the right headers — see §6 below.

**Testing tip:** Let's Encrypt rate-limits heavily on repeat attempts for the same
hostname. When iterating, use staging:

```bash
hiveloom tls render --host hiveloom.example.com --email you@example.com --acme-env staging
```

The resulting cert won't be publicly trusted; add `-k` to curl. Swap back to production
with a fresh render + reload once things look right.

---

## 5. Firewall — minimum open ports

Once Caddy terminates TLS, the only public ports you need are SSH, TCP/80 (for ACME
HTTP-01 challenges and the HTTPS redirect), and TCP/443.

### UFW recipe (Ubuntu/Debian default)

```bash
# Detect your actual SSH port first — don't assume 22
sudo grep -i '^Port' /etc/ssh/sshd_config /etc/ssh/sshd_config.d/*.conf 2>/dev/null
# Replace 22 below with whatever Port is set to.

sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow 22/tcp   comment "ssh"          # use YOUR actual ssh port
sudo ufw allow 80/tcp   comment "caddy acme+redirect"
sudo ufw allow 443/tcp  comment "caddy https"
sudo ufw enable
sudo ufw status numbered
```

**Before running `ufw enable` on a remote box:**

- Double-check the SSH port number. An off-by-one here will cut your session.
- Keep a second SSH session open in another terminal as insurance.
- Confirm no other service you care about is listening on a port you're about to block
  (`ss -tlnp`).

### firewalld (RHEL/Fedora)

```bash
sudo firewall-cmd --permanent --add-service=ssh
sudo firewall-cmd --permanent --add-service=http
sudo firewall-cmd --permanent --add-service=https
sudo firewall-cmd --reload
```

### Cloud-provider security groups

Whatever firewall the VPS provider runs *in front of* your host (AWS Security Groups,
Hetzner Cloud Firewall, DigitalOcean Cloud Firewall, etc.) needs the same rule shape:
allow inbound 22 (or your SSH port), 80, 443 from `0.0.0.0/0` and `::/0`; deny the rest.
UFW / firewalld only covers the host-level layer — if port 80 is blocked at the cloud
edge, ACME challenges cannot complete.

---

## 6. Bring-your-own-proxy contract

If you already run Nginx, Traefik, Cloudflare Tunnel, or a Kubernetes ingress, you do
not need the Caddyfile. Point your proxy at Hiveloom bound to loopback (§3) on port
3000, and make sure the following forwarded headers are set:

| Header | Required value | Consumed by |
|---|---|---|
| `X-Forwarded-Proto` | `https` | Hiveloom uses this to construct `https://` URLs in OAuth discovery metadata. Without it, the metadata returns `http://` URLs and MCP clients reject the auth server. |
| `X-Forwarded-Host` | `<your-public-host>` | Same; used to construct the hostname portion of public URLs. |
| `X-Forwarded-For` | `<client-ip>` (or appended) | Optional but recommended; logged on the Hiveloom side. |
| `Host` | `<your-public-host>` | Fallback if `X-Forwarded-Host` is absent. |

The precedence in Hiveloom's URL builder is: `X-Forwarded-Host` → `Host`, with
`X-Forwarded-Proto` → default `http`. Source: `src/server/mcp/auth.rs`.

### Public paths that must be forwarded

The proxy must forward the entire origin — do not path-gate it. The admin API, MCP
endpoints, and health endpoint are all under `/`. If you really must restrict, these
are the public paths that need to reach the upstream for MCP OAuth to function:

- `/healthz`
- `/.well-known/oauth-authorization-server`
- `/mcp/<tenant>/<agent>` and `/mcp/<tenant>/<agent>/.well-known/oauth-protected-resource`
- `/oauth/register`, `/oauth/authorize`, `/oauth/token`

### Minimal Nginx snippet

```nginx
server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name hiveloom.example.com;

    # ssl_certificate / ssl_certificate_key — via certbot or your own CA

    location / {
        proxy_pass         http://127.0.0.1:3000;
        proxy_set_header   Host                $host;
        proxy_set_header   X-Forwarded-Host    $host;
        proxy_set_header   X-Forwarded-Proto   https;
        proxy_set_header   X-Forwarded-For     $proxy_add_x_forwarded_for;
        proxy_read_timeout 300s;
    }
}
```

### Cloudflare Tunnel

Set the `originRequest` to `http://127.0.0.1:3000` and add `X-Forwarded-Proto: https`
and `X-Forwarded-Host: <hostname>` via the tunnel config. Cloudflare strips `Host` by
default in some configurations — explicitly set `httpHostHeader: hiveloom.example.com`
in the ingress rule to keep the URL building correct.

---

## 7. Smoke test after TLS is up

```bash
# 1. Health over HTTPS
curl -sf https://hiveloom.example.com/healthz

# 2. OAuth metadata — every URL must be https://
curl -s https://hiveloom.example.com/.well-known/oauth-authorization-server | jq

# 3. Port scan from another host — only SSH, 80, 443 should answer
nmap -Pn -p 1-10000 hiveloom.example.com
```

Then reissue an MCP setup code so new clients get URLs that embed the `https://` base:

```bash
hiveloom mcp-identity reissue-setup-code <identity-id> --tenant <tenant>
```

(Previously-issued codes continue to work — the MCP URL in Caddy's metadata is derived
from the incoming `Host` header at resolution time, not baked in.)

---

## FAQ

**Does Hiveloom install Caddy for me?** No. It prints the Caddyfile; you install Caddy.

**Does Hiveloom rebind itself to loopback after TLS setup?** No. You edit the systemd
unit (§3). This is intentional — modifying the service user's unit file is the kind of
action that should be visible in your config-management history.

**Does Hiveloom check that DNS points at the VPS before printing the Caddyfile?** No.
`dig +short <host>` and confirm before running the command.

**Do my existing MCP setup codes break after TLS is up?** No. The public URL that the
server reports is derived from the incoming request's `Host` / `X-Forwarded-*` headers,
so clients that connect via `https://<host>` will receive `https://<host>` URLs in the
OAuth metadata automatically. New setup codes will embed the new base too.

**Should I expose the Hiveloom upstream port (`:3000`) on the public internet?** No,
not once Caddy (or any proxy) is in front. Bind to `127.0.0.1:3000` and let the proxy
be the only public entry point.

**My ACME challenge fails — `caddy` reports `context deadline exceeded`.** Two common
causes: (1) DNS doesn't actually resolve to this VPS from Let's Encrypt's vantage, or
(2) TCP/80 is blocked upstream (cloud security group, not UFW). Port 80 must be
reachable *from the internet*, not just from the VPS itself.

**Why does `hiveloom tls render` not write the file itself?** You asked Hiveloom to
stay out of your system configuration. Pipe to `tee` when you want the file.

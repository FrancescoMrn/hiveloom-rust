# Security policy

## Reporting a vulnerability

Please **do not file a public GitHub issue** for security vulnerabilities.

Instead, use one of:

1. **GitHub private security advisory** — preferred. Open one at
   <https://github.com/FrancescoMrn/hiveloom-rust/security/advisories/new>.
   This creates a private thread with the maintainers and lets us coordinate a
   fix and CVE assignment before disclosure.

Please include:

- A description of the issue and its impact.
- Reproduction steps or a minimal proof-of-concept.
- The commit SHA or released version you tested against.
- Whether you intend to publish your own write-up, and a proposed timeline.

We aim to acknowledge reports within **72 hours** and issue a fix or mitigation
advisory within **30 days** for high-severity issues. Lower-severity issues may
ride along with the next regular release.

## Supported versions

Hiveloom is pre-1.0. Only the latest release gets security fixes. There is no
long-term support branch yet.

| Version | Supported |
|---|---|
| latest release | Yes |
| anything older | No — upgrade first |

## Scope

In scope:

- The `hiveloom` binary (CLI + service).
- The HTTP admin API, MCP endpoint, OAuth server, and Slack adapter.
- The default install script at `https://bin.hiveloom.cloud/install.sh`.

Out of scope:

- Third-party MCP clients (Claude Desktop, Cursor, etc.).
- User-provided LLM providers and their API keys.
- Misconfigurations of self-hosted deployments (e.g. Caddy, firewalls) —
  [`docs/deployment-hardening.md`](docs/deployment-hardening.md) covers the
  defaults we recommend.

## Disclosure

Once a fix is released, we publish a GitHub Security Advisory describing the
issue, affected versions, and credit. Reporters who request anonymity will be
credited as "anonymous".

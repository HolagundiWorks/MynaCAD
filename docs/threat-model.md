# MynaCAD threat model (desktop automation and supply chain)

**Status:** Implemented documentation of the current trust boundary and must-not rules.  
**Scope:** Control channel, MCP, packaging/signing, and dependency licence hygiene.  
**Not in scope here:** Optional remote TLS + AuthZ for non-loopback automation (proposed; see Planned).

Canonical protocol detail: [control-protocol.md](control-protocol.md). Packaging: [RELEASE_AND_PACKAGING.md](RELEASE_AND_PACKAGING.md), [releasing.md](releasing.md). Agent rules: [ai-agent.md](ai-agent.md).

## Assets

| Asset | Why it matters |
|---|---|
| Open drawing (in-memory session) | Control / MCP / agent tools can run `engine.execute`, open/save paths, and scripts |
| Drawing files on disk | `app.open` / `app.save` and MCP `open` / `save` use caller-supplied paths |
| Installer / portable binaries | Unsigned builds trigger SmartScreen; tampered binaries are a supply-chain risk |
| Third-party Rust crates | Licence and advisory posture; no GPL/LGPL/AGPL |

## Trust boundary: loopback control

The JSON-lines control channel binds **`127.0.0.1` only** (hard-coded in `crates/aad-host` and `apps/aad`). There is **no** TLS, shared secret, or per-user AuthZ on that socket today.

| Assumption | Consequence |
|---|---|
| Only processes on the same machine that can open loopback TCP may talk to `--control PORT` | Any local user or malware that can reach that port can drive the session like the UI (commands, open/save) |
| The operator chose the port (`--control` / `AAD_CONTROL_PORT`) | Treat the port as a local capability, not a network service |
| MCP bridged with `--connect 127.0.0.1:PORT` inherits the same boundary | Prefer MCP **stdio** when the client can spawn `aad-cli mcp` (process ACL instead of a TCP port) |

This default is appropriate for **single-user desktop** automation (CI scripts, local agents, MCP on the same host). It is **not** “enterprise remote automation.”

### Request size

Each control request is one JSON line. Both the WinUI host and the egui control server reject a line longer than **1 048 576 bytes** (`request is too long`) and do not allocate unbounded line buffers for that purpose. Concurrent-connection and rate limits are not implemented yet (follow-up).

## MCP

| Mode | Trust |
|---|---|
| `aad-cli mcp` (stdio, in-process session) | Same privilege as the `aad-cli` process; no network listen |
| `aad-cli mcp --connect 127.0.0.1:PORT` | Same as loopback control above |
| Tool arguments | Schema-validated; invalid args return JSON-RPC `-32602` |

The model never computes geometry or silent repairs; it only proposes allow-listed tools ([ai-agent.md](ai-agent.md)).

## Authenticode / signing keys

MynaCAD is a desktop app. Code-signing material is used only for Windows Authenticode (and later-phase Apple notarization), not for encrypting drawings.

| Mode | Where secrets live | Repo |
|---|---|---|
| Production PFX | CI / vault env `WINDOWS_CERTIFICATE` (+ optional password) | Never committed |
| Azure Trusted Signing | `AZURE_*` env (see `packaging/windows/sign.ps1`, `ossign.config.example.yaml`) | Example YAML only |
| Dev self-sign | `SIGN_SELF_SIGNED=1` → local cert under `%LOCALAPPDATA%\O-Draft\dev-codesign\` | Never committed |

SmartScreen still warns for self-signed builds. Rotate by reissuing Trusted Signing / PFX; delete local dev certs when done.

## Must not

- Bind control (or MCP over TCP) to `0.0.0.0` or any non-loopback address **without** authentication **and** TLS.
- Commit private keys, `.pfx` / `.p12`, Azure client secrets, model/API tokens, or filled `ossign.config.yaml` (use the example file only).
- Treat loopback-without-auth as remote enterprise AuthZ.
- Disable hostile-input caps or DWG process isolation for convenience.
- Link GPL/LGPL/AGPL code or proprietary third-party CAD assets.

## Supply chain (current vs follow-up)

| Control | Today | Follow-up |
|---|---|---|
| Licence allow-list | `cargo xtask licenses` in every `cargo xtask ci` | Keep |
| Advisory audit | `cargo audit` on nightly CI | Triage SLA / optional blocking on main |
| SBOM | `cargo xtask sbom` writes CycloneDX JSON (no secrets); release workflow attaches it | Keep format stable; optional SPDX twin |
| NOTICE in packages | Staged into Windows MSI/zip (`package.ps1`, `aad.wxs`) | Re-verify on next signed release candidate |
| Signed releases | Hooks exist; many builds still unsigned without secrets | Require Trusted Signing (or org PFX) for published Windows artifacts |

## Planned AuthZ (not implemented)

For multi-user machines or remote automation: explicit opt-in bind beyond loopback only with TLS **and** a shared secret or OS ACL (for example named-pipe) plus a per-session token; refuse plaintext remote bind. Acceptance tests would cover unauthorized and wrong-token rejection while keeping the loopback default. Until that lands, do not expose the control port outside the local host.

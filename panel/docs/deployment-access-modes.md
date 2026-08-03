# Panel Deployment Access Modes

## Purpose

Hydra must support operators who own a domain and operators who only have a VPS IP address.

The preferred production mode is still:

```text
domain -> trusted TLS certificate -> panel
```

But the product must also support a practical 3x-ui-like quick setup mode:

```text
IP address -> panel port
```

This is important for users who:

- do not own a domain;
- cannot buy or delegate a domain yet;
- want to test the panel quickly on a cheap VPS;
- run the panel behind a private network, VPN, jump host, or local tunnel;
- need a recovery/emergency access mode before DNS is ready.

The implementation must not pretend that all modes have equal security. The UI and installer should clearly explain the tradeoffs and apply stricter defaults when no domain is used.

## Modes

### `domain_tls`

Recommended production mode.

Operator provides:

- domain name, for example `panel.example.com`;
- public server IP;
- optional email for ACME/Let's Encrypt;
- panel listen port or reverse-proxy choice.

Expected behavior:

- installer checks DNS points to the server;
- installer opens only required ports;
- panel is served through HTTPS with a public trusted certificate;
- certificate issuance/renewal is automated where possible;
- certificate status is visible in the panel;
- certificate operations are audited.

Advantages:

- browser trusts the certificate;
- clean URL;
- best UX;
- best default security posture;
- easiest future integration with reverse proxies/CDN/WAF if needed.

Risks/requirements:

- requires a domain;
- requires correct DNS;
- requires ACME reachability;
- operator must understand certificate renewal and backups.

### `ip_http`

Quick setup mode without a domain.

Example:

```text
http://203.0.113.10:2053
```

Operator provides:

- panel port;
- admin credentials;
- optional firewall allowlist.

Expected behavior:

- panel listens on a chosen port;
- installer prints the exact URL;
- UI shows a warning that traffic is not protected by TLS;
- installer enables strict security defaults by default.

Required hardening defaults:

- generated strong admin password during first install unless operator overrides it;
- 2FA strongly recommended during first login;
- login protection enabled;
- smart ban enabled;
- history/audit enabled;
- random admin path or secret base path should be supported in the installer plan;
- optional firewall allowlist should be offered immediately;
- panel should never bind publicly without showing the final risk summary.

Advantages:

- simplest possible install;
- no domain needed;
- no certificate workflow needed;
- useful for test servers and emergency access.

Risks:

- credentials travel over plaintext HTTP;
- browser has no transport authenticity;
- unsafe on hostile networks;
- should not be recommended for long-term public production use.

### `ip_self_signed_tls`

Hardened IP mode without a domain.

Example:

```text
https://203.0.113.10:2053
```

Operator provides:

- panel port;
- optional generated self-signed certificate details;
- optional firewall allowlist.

Expected behavior:

- installer generates a local self-signed certificate;
- panel serves HTTPS directly or through a local reverse proxy;
- UI explains that the browser warning is expected because the certificate is not publicly trusted;
- certificate fingerprint is printed during installation so the operator can compare it.

Advantages:

- encrypted transport without a domain;
- better than plain HTTP for public networks;
- still one-line-install friendly.

Risks:

- browser warning is normal;
- user must manually trust/check the certificate;
- does not provide the same identity assurance as a public CA certificate;
- renewal/replacement must be explicit and visible.

### `reverse_proxy`

Advanced mode.

Operator provides:

- local bind address and port, for example `127.0.0.1:8080`;
- external reverse proxy config managed by operator;
- proxy trust settings if using forwarded headers.

Expected behavior:

- panel binds to loopback or private interface by default;
- installer does not blindly trust `X-Forwarded-For`;
- trusted proxy IP/CIDR must be configured explicitly;
- UI shows whether forwarded headers are trusted and from which proxy ranges.

Advantages:

- works with existing nginx/Caddy/Traefik setups;
- operator can use custom TLS/cert automation;
- panel can avoid direct public exposure.

Risks:

- incorrect `X-Forwarded-For` trust can break login protection and IP bans;
- reverse-proxy config mistakes can expose unintended routes;
- installer must not silently enable proxy trust.

## Installer Flow

The one-line installer should ask a domain question early:

```text
Do you have a domain for the panel?
1. Yes, use a domain and HTTPS certificate (recommended)
2. No, use IP:PORT quick setup
3. No, use IP:PORT with self-signed HTTPS
4. I will use my own reverse proxy
```

### Flow A: Domain + Let's Encrypt

Steps:

1. Ask for domain.
2. Ask for ACME email or allow empty if supported by chosen ACME flow.
3. Resolve domain and compare A/AAAA records with detected server IP.
4. Ask whether to continue if DNS is not ready.
5. Install panel service.
6. Configure HTTPS listener or reverse proxy.
7. Issue certificate.
8. Store certificate/key paths explicitly.
9. Enable renewal timer/service.
10. Print final URL.
11. Show certificate status and next renewal time.

Fail-closed behavior:

- if certificate issuance fails, do not silently fall back to HTTP production mode;
- offer explicit choices:
  retry DNS/cert,
  switch to IP quick mode,
  switch to self-signed IP mode,
  exit without exposing the panel publicly.

### Flow B: IP:PORT HTTP

Steps:

1. Detect public IP and local interfaces.
2. Ask for panel port, default a random high port or a documented default.
3. Ask whether to bind publicly or loopback/private interface only.
4. Offer firewall allowlist:
   current SSH client IP,
   custom IP/CIDR,
   open to internet with warning.
5. Generate first admin secret or ask operator to set it.
6. Enable login protection, smart ban, audit/history.
7. Strongly recommend 2FA on first login.
8. Print final URL.
9. Show security warning in installer output and panel UI.

Fail-closed behavior:

- do not enable `trust_x_forwarded_for`;
- do not mark this mode as `production_secure`;
- if operator chooses public HTTP, require an explicit confirmation.

### Flow C: IP:PORT Self-Signed HTTPS

Steps:

1. Detect public IP and local interfaces.
2. Ask for panel port.
3. Generate self-signed certificate for the IP address.
4. Print certificate fingerprint.
5. Store cert/key paths explicitly.
6. Configure HTTPS listener or local reverse proxy.
7. Offer firewall allowlist.
8. Enable strict panel protections.
9. Print final URL and browser warning explanation.

Fail-closed behavior:

- private key must be `0600` where supported;
- certificate replacement must be explicit and audited;
- installer must not tell the user that the browser warning means the install is broken.

### Flow D: Existing Reverse Proxy

Steps:

1. Ask for local bind address and port.
2. Ask whether proxy is local-only or remote.
3. Ask for trusted proxy IP/CIDR if forwarded headers are required.
4. Print sample nginx/Caddy/Traefik snippets later when supported.
5. Keep direct panel listener private by default.
6. Show final local URL and expected external URL.

Fail-closed behavior:

- `trust_x_forwarded_for=false` unless trusted proxy ranges are explicitly configured;
- if proxy trust is enabled without ranges, reject the config.
- `X-Forwarded-For` is accepted only when the direct TCP peer is trusted.
- malformed `X-Forwarded-For` chains are ignored.
- valid chains are resolved from right to left, stripping trusted proxy hops and selecting the nearest untrusted hop as the client IP; this avoids trusting a spoofed leftmost value when a proxy appends instead of overwrites the header.

## Backend Model To Add

Introduce a panel access mode model. Exact names may change, but the contract should be explicit:

```text
PanelAccessMode:
  domain_tls
  ip_http
  ip_self_signed_tls
  reverse_proxy
```

Suggested settings:

```json
{
  "access_mode": "ip_self_signed_tls",
  "public_url": "https://203.0.113.10:2053",
  "bind_host": "0.0.0.0",
  "bind_port": 2053,
  "tls": {
    "enabled": true,
    "source": "self_signed",
    "certificate_path": "/etc/hydra-panel/certs/panel.crt",
    "private_key_path": "/etc/hydra-panel/certs/panel.key",
    "fingerprint_sha256": "..."
  },
  "domain": null,
  "reverse_proxy": {
    "enabled": false,
    "trusted_proxy_cidrs": []
  },
  "security_posture": "limited_without_domain"
}
```

Required API/UI surfaces:

- read current panel access mode;
- update access mode through a guided operation, not silent raw config editing;
- show security posture:
  `recommended`, `limited_without_domain`, `danger_plain_http_public`, `custom_reverse_proxy`;
- expose certificate status without returning private key material;
- expose public URL for UI copy buttons and installer output;
- audit every mode/certificate/listener change.

## Security Rules

Domain mode:

- recommended by default;
- public trusted TLS is preferred;
- certificate keys are high-sensitivity secrets;
- renewal state must be operator-visible.

IP HTTP mode:

- allowed, because it is important for accessibility and quick installs;
- must be visibly marked as weaker;
- should strongly enable 2FA/login protection/smart bans;
- should encourage firewall allowlist;
- must not claim secure transport.

IP self-signed TLS mode:

- better than plain HTTP;
- must explain browser warnings;
- must expose certificate fingerprint;
- must store private key securely;
- must not claim public CA trust.

Reverse proxy mode:

- must require explicit trusted proxy IP/CIDR before trusting forwarded headers;
- must not trust arbitrary `X-Forwarded-For`;
- should bind panel to loopback/private interface by default.

All modes:

- admin sessions must remain cookie/token safe;
- login protection must use the correct client IP source;
- sensitive setup output must not include long-lived secrets in shell history;
- generated config files must use restrictive permissions where supported;
- all setup changes must be auditable.

## Implementation Roadmap

### Phase 1: Documentation and model

- define access modes and installer UX;
- add config/domain types for panel access mode;
- add validation rules for each mode;
- add read-only API view for current mode and security posture.

Current backend status:

- `GET /api/installer/access-modes` returns supported modes and operator warnings.
- `POST /api/installer/plan` returns a dry-run plan with security posture, URL, bind address, certificate plan, reverse-proxy plan, hardening defaults, ordered steps, warnings, and required confirmations.
- The plan endpoint validates domains, public IP, bind host, firewall allowlist, and trusted proxy CIDRs fail-closed.
- The plan endpoint is intentionally non-mutating; it does not install services, write certificates, open firewall ports, or restart listeners.
- `POST /api/installer/bootstrap` returns Linux/Windows one-line command snippets, a supported platform matrix, release channel/architecture selection, release manifest artifact selection, artifact verification metadata, and secret-free environment variables.
- The bootstrap endpoint validates release inputs fail-closed and returns `ready_to_run=false` when no trusted installer script URL or SHA-256 digest is provided.
- The bootstrap endpoint can derive URL, version, SHA-256, signature URL, and signing key fingerprint from a typed release manifest.
- Release manifest artifacts are typed as `installer_script`, `panel_binary`, or `node_binary`.
- The one-line bootstrap command selects only an `installer_script` matching the requested `target_os`, `target_arch`, and channel.
- Linux installer scripts must be Linux `.sh` artifacts; Windows installer scripts must be Windows `.ps1` artifacts.
- Linux binary artifacts must not use Windows extensions; Windows binary artifacts must be `.exe`.
- This means a Linux operator must not download Windows installer/binary artifacts, and a Windows operator must not download Linux installer/binary artifacts.
- Bootstrap command snippets download the installer to a temporary file, verify SHA-256, and only then execute it.
- `POST /api/installer/session` wraps the validated plan into command envelopes, a loop contract, per-command acceptance contracts, and explicit target context:
  `target_os`, `target_arch`, `package_channel`, and selected artifact metadata.
- each command envelope includes typed `operations[]`.
- operations with `program` and `args` must be executed as direct argv, never through shell-string interpolation.
- write config, write service, certificate issue/generation, and security-default operations are declarative executor operations with target paths/templates instead of hidden shell snippets.
- Linux service operations write `/etc/systemd/system/hydra-panel.service` and start it through `systemctl enable --now`.
- Windows service operations create/start `HydraPanel` through `sc.exe` and the selected Windows `.exe` path.
- `POST /api/installer/session/result` validates executor results fail-closed: empty, duplicate, unknown, or missing command results are rejected, non-zero exits are rejected, and each command must provide the required attestation fields.
- When operation ids are expected, each command attestation must include matching `operation_results[]`; missing, duplicate, unexpected, incomplete, failed, or explicitly unverified operation results are rejected.
- `POST /api/installer/jobs` creates a bounded-lifetime tracked installer job and returns a one-time executor token while storing only its hash.
- `POST /api/installer/jobs/heartbeat` and `POST /api/installer/jobs/result` accept executor-token-authenticated job updates without an admin session.
- Job result validation derives expected command ids and expected operation ids from the saved panel session, so the executor cannot weaken its own acceptance contract.
- Installer jobs are persisted to disk with token hashes only; plaintext executor tokens are returned once and are not stored.
- Installer jobs are bounded by `max_panel_installer_jobs_buffered` and compacted before persistence for the `512 MB RAM` target.
- Panel bootstrap/session/result endpoints are non-mutating. The separate Rust
  `panel-installer-executor` performs target-host mutations only after fetching a
  persisted job with its one-time token.
- `POST /api/installer/jobs/executor-session` is the read-only executor fetch
  route. It rejects invalid tokens, expired jobs, and terminal jobs.
- Managed jobs select a matching `panel_binary` separately from the
  `installer_script`; a job is rejected if the release manifest does not contain
  both artifacts for the exact OS, architecture, channel, and version.
- Linux and Windows packaging scripts emit only native artifacts plus SHA-256
  sidecars and typed manifest fragments. Linux uses MUSL release targets to avoid
  a dependency on a recent distribution glibc.
- The panel listener serves native HTTPS when both certificate and key paths are
  configured. A partial TLS configuration fails startup.
- Detached signature verification and release publication remain future release
  gates; the executor rejects signature metadata until verification is implemented.
- Windows release artifacts can be staged, but the managed executor rejects the
  platform before any mutation until Windows service environment propagation,
  ACL hardening, and certificate lifecycle are complete.

### Phase 2: Installer flow

- managed install/reinstall executes `domain_tls`, `ip_http`,
  `ip_self_signed_tls`, and `reverse_proxy` plans through the typed executor;
- Linux first-host bootstrap asks the domain question and builds the same
  validated plan locally, because no panel exists yet to issue a managed job;
- first-host bootstrap does not start a temporary setup endpoint at all, so it
  adds no unauthenticated public API or additional listener;
- first-host dry-run returns the complete secret-free executor session without
  downloading the panel binary or changing the host;
- domain TLS prepares only the allowlisted `certbot` dependency through
  `apt-get`, `dnf`, or `yum`; port 80 must be available/reachable for the
  standalone ACME challenge and failure is fail-closed;
- installer output must show the final URL, warnings, and self-signed fingerprint;
- firewall changes must use the exact validated bind port and allowlist.

### Phase 3: Certificate and listener lifecycle

- self-signed certificate generation and Linux Let's Encrypt issuance are implemented in both managed and first-host executor modes;
- finish persisted ACME renewal status and safe renewal rollback;
- persist cert/key paths and fingerprint;
- add renewal/replacement status and audit events;
- add safe rollback if listener/cert update fails;
- harden Windows certificate and service environment handling before declaring Windows production-ready.

### Phase 4: UI integration

- first-run setup screen shows the same choices as installer;
- settings page shows access mode, public URL, TLS/cert status, proxy trust status;
- dangerous public HTTP state is highlighted;
- user can migrate from IP mode to domain mode later.

### Phase 5: Hardening and tests

- test config validation for every mode;
- test trusted proxy fail-closed behavior;
- test no private key material is returned by API;
- test installer dry-run summaries;
- test memory impact stays negligible on `1 vCPU / 512 MB RAM / 10 GB disk`.

## Product Positioning

Recommended label:

```text
Domain + HTTPS
Recommended for production
```

Quick setup label:

```text
IP-only quick setup
Works without a domain, but has reduced security unless you restrict access.
```

Self-signed label:

```text
IP + self-signed HTTPS
Encrypted without a domain. Browser warning is expected; verify fingerprint.
```

Reverse proxy label:

```text
Custom reverse proxy
For operators who manage TLS/proxying themselves.
```

The product should support all four modes. The UI should recommend the safer mode without blocking users who cannot use a domain.

# Architecture

## Goal

Build a Rust control plane that fully replaces the current Python-based Marzban/Hydra panel while staying within a cheap deployment envelope and raising the security bar.

## Non-Negotiable Constraints

- target deployment:
  `1 vCPU / 512 MB RAM / 10 GB disk`
- security-first handling of secrets and operator actions
- bounded background work and bounded telemetry/log ingestion
- no implicit cross-module mutable state

## Initial Technical Direction

### Backend

- `axum` for HTTP API
- `tokio` for async runtime
- modular monolith layout
- versioned JSON APIs internally prepared for a later shared protocol package

### Runtime Deployment Model

Primary mode:

- `Panel Standalone`
- the main server runs `Hydra-Panel` and does not require a separate `Hydra-node`;
- the panel owns the single-server runtime flow:
  generated config -> `xray.json` -> internal validation -> optional real Xray validation -> core apply/restart/update -> runtime state;
- this mode must remain enough for a normal one-server installation and must not depend on node-agent heartbeats.

Extension mode:

- `Panel + Remote Nodes`
- additional servers run `Hydra-node`;
- remote nodes fetch node-scoped least-knowledge runtime config and route credentials from the panel;
- remote nodes are for scaling, multi-server routing, and cluster relays, not for making the main server usable.

Installer implication:

- setup should ask whether the operator is installing the main panel server or a remote node;
- main-panel installation should default to standalone runtime support;
- remote-node installation should require panel URL and a one-time node token.

### Frontend

- target frontend is `Leptos` in CSR/static-assets mode
- no Leptos SSR for the panel UI
- no server functions for normal dashboard rendering
- `panel-app` should serve built static assets
- realtime RAM/CPU/system updates should start with bounded polling against API endpoints
- SSE can be added later for telemetry/alerts if polling is not sufficient

### Panel Access Modes

The panel must support both domain-based and domain-less deployments.

Recommended production mode:

- `domain_tls`
- operator uses a domain;
- panel is served over trusted HTTPS;
- Let's Encrypt/ACME issuance and renewal should be a normal guided path.

Domain-less modes:

- `ip_http`:
  quick `http://IP:PORT` access for operators without a domain.
- `ip_self_signed_tls`:
  `https://IP:PORT` with a generated self-signed certificate and visible fingerprint.
- `reverse_proxy`:
  operator-managed proxy/TLS, with explicit trusted proxy IP/CIDR rules.

Architectural rules:

- domain-less access is a supported product path, not an unsupported workaround;
- the UI and installer must clearly label security posture differences;
- IP HTTP mode must not be described as secure transport;
- IP self-signed mode must explain browser warnings and expose certificate fingerprint;
- reverse proxy mode must not trust `X-Forwarded-For` unless trusted proxy ranges are explicitly configured;
- all mode/listener/certificate changes must be auditable;
- no API response may return TLS private keys or long-lived setup secrets.

Detailed installer and product plan:

- `docs/deployment-access-modes.md`

### Domain Boundaries

- `auth`
- `security`
- `admins`
- `users`
- `subscriptions`
- `xray_core`
- `nodes`
- `clusters`
- `provisioning`
- `monitoring`
- `notifications`
- `audit`
- `system_settings`
- `certificates`
- `scan_defense`
- `routing_presets`

### Memory Discipline

Every feature should define:

- expected steady-state memory use;
- burst behavior;
- retention limits;
- queue/buffer limits;
- operator-visible degradation mode.

Acceptance procedure:

- `docs/load-memory-validation.md`

This is especially important for:

- login/ban history
- logs viewers
- provisioning logs
- node telemetry
- future online sessions / device tracking

### Data Protection

Data should be classified before implementation:

- public
- operator-internal
- sensitive
- high-sensitivity secret

High-sensitivity data includes at minimum:

- 2FA secrets
- JWT/session keys
- node trust materials
- TLS certificates/private keys
- SSH credentials

Rules:

- no plaintext secret logging
- no silent persistence of request-time secrets
- explicit rotation and revocation flows
- auditable privileged actions

## First Delivery Stages

1. security/admin foundation
2. system/core management
3. user/subscription baseline
4. node management and provisioning
5. monitoring and notifications

## Cluster Orchestration Architecture

Cluster orchestration is a first-class product direction, not a UI-only feature.

The panel should model multi-hop traffic paths explicitly:

- entry node
- zero or more relay nodes
- exit node
- routing policy
- failover policy
- applied revision

The expected path shape is:

`client -> entry -> relay(s) -> exit -> internet`

Architectural rules:

- a node must never become an implicit open relay
- every cluster edge must describe authenticated source/destination identities
- every cluster change must produce a revision and audit event
- cluster apply must support partial failure reporting and rollback metadata
- cluster health checks must be bounded by concurrency and interval limits
- route visualization must read summarized state, not raw unbounded telemetry
- the future Leptos UI should expose cluster paths as a visual graph/map, but graph validation and least-knowledge projection must remain backend responsibilities
- subscription/client node access policies must be visible in the graph without leaking unrelated topology to relay nodes

The product language should frame this as:

- multi-hop routing
- controlled egress
- privacy
- resilience
- routing policy

Do not frame it as guaranteed invisibility or legal evasion.

## Subscription Catalog Architecture

The product should move toward an operator-managed subscription catalog instead of a flat global user list.

Current backend foundation:

- `SubscriptionCatalogPlan`, `SubscriptionCatalogClient`, explicit node/cluster access policy, and usage window contracts exist in the domain layer
- panel API exposes plan/client CRUD, admin-side catalog-client subscription rendering, node-access updates, usage detail, usage reset, and revoke
- public `/sub/{subscription_token}` resolves both legacy users and catalog clients, with catalog clients failing closed when disabled, expired, or revoked
- catalog state is persisted separately from the legacy flat user list
- inbound/protocol endpoints and host endpoints can be bound to `node_id` and/or `cluster_id`
- rendered catalog-client output carries access policy metadata and filters inbounds/hosts through node/cluster policy
- production subscription delivery is schema-versioned and supports safe structured JSON, newline URI lists, and Base64 URI subscriptions
- VLESS, VMess, Trojan, Shadowsocks, and Hysteria2 use interoperable client URIs; WireGuard uses structured JSON because it has no interoperable subscription URI
- production subscription bodies never expose raw profile JSON, server TLS key paths, WireGuard interface private keys, or the subscription bearer token
- client credentials are generated once into the runtime profile projection and reused by standalone Panel Xray, remote Node runtime, and subscription rendering; independent credential formulas are forbidden
- active catalog clients are bound into generated core/Xray client configuration using stable technical principal `catalog/{client_id}`; editable display names are not runtime credential identity
- catalog client Xray bindings are emitted only for allowed inbounds and disappear when a client becomes disabled, expired, or revoked
- `GET /api/subscription-clients/{client_id}/access-preview` exposes allowed/denied inbounds and hosts with reasons plus renderability warnings
- unbound/global inbounds and hosts are excluded for explicit catalog-client allowlists; legacy user rendering remains unchanged
- device/HWID registry and admission endpoints exist on `SubscriptionCatalogClient`; max-device admission is applied when a new device is registered
- device fingerprint input is not persisted or returned; only a keyed HMAC is stored, with key material separated through `HYDRA_SUBSCRIPTION_DEVICES_KEY_PATH`
- self-service device enrollment is capability-based: an administrator issues a 256-bit, 60-1800 second, one-time token; the public exchange atomically consumes it and creates a previously unknown device
- enrollment tokens and device subscription credentials are persisted only as separately domain-bound HMAC-SHA256 values; neither raw value is logged, audited, listed, or recoverable from the catalog
- every enrolled device owns a separate `/sub/device/{device_credential}` bearer path; it cannot fall back to the catalog-client parent token, and device/client revocation invalidates it without rotating credentials for unrelated devices
- grant consumption and device creation occur under one subscription-catalog write lock and one atomic catalog replacement, so concurrent replay admits at most one device; persistence failure returns no credential
- enrollment grant storage is bounded globally and to eight active grants per client; terminal grants may be compacted, while an absent or compacted token still fails closed
- the subscription catalog contains bearer material and is written with mode `0600` on Unix
- HTTP tracing must redact the complete `/sub/...` path and omit query strings because both legacy and device-scoped subscription URLs contain bearer material
- node-agent session observations are checked against that node's projected `catalog/{client_id}` principals before any client policy evaluation, so least-knowledge relay nodes cannot query arbitrary catalog clients through the policy API
- session verdicts enforce registered-device presence when a device limit is configured and optionally enforce bounded simultaneous source IPs through `max_simultaneous_ips`
- every blocked observation yields a typed `terminate_session` command; node-agent reports `applied` or `failed` for that specific action/session binding, with results visible in the bounded admin session view and audit stream
- exact termination proof is bound to an opaque node-local `runtime_session_ref`: the panel retains only a keyed HMAC, requires `node_managed_exact_session` plus post-action absence evidence for `applied`, and does not disclose the raw local reference to operator APIs
- node session reports negotiate capability explicitly: an exact action requires `opaque_session_reference`, `exact_session_termination`, and `post_action_absence_verification`; `principal_wide_termination_only` is recorded as insufficient and never escalated into a destructive fallback command
- raw source IP and device fingerprint inputs are not returned; source IP comparisons use keyed HMAC material only inside bounded in-memory session state
- the existing Rust node-agent now has a protected lease-bound local adapter handshake with bounded exact-action deadlines and proof forwarding; non-WireGuard runtime observation and Telegram management parity are still pending layers
- do not assume the Xray process provides exact per-connection termination just because it exposes runtime/stat APIs; the Rust Node adapter must prove its locator/termination semantics before enabling successful enforcement acknowledgements

Target shape:

- subscription groups/plans are created in the panel
- each subscription contains its own clients
- each client can be assigned an explicit node/cluster access policy
- subscription rendering filters available servers, protocols, hosts, and cluster paths through that access policy

Client settings and operations:

- maximum simultaneously connected devices
- traffic limit
- expiration date
- operator note
- reset traffic
- revoke subscription
- delete client
- usage detail with server/node traffic breakdown

Usage detail must support fixed windows and custom ranges:

- 12 hours
- 1 day
- 3 days
- 1 week
- 1 month
- 3 months
- custom start/end timestamps

The same client operations must be available through API and Telegram bot workflows.

Security rules:

- a client must not implicitly receive access to every node
- node access changes must be auditable
- subscription tokens and client secrets must not appear in runtime configs, logs, telemetry, or UI bootstrap payloads
- relay nodes should not receive subscription/client lists unless a future local enforcement role explicitly requires it
- device/HWID/session controls should attach to the subscription client layer so enforcement remains explainable and bounded
- the device registry must remain bounded and must never expose raw device fingerprints or persisted fingerprint HMAC values through API output
- enrollment endpoints must preserve one-time capability semantics: short TTL, cryptographic randomness, hash-only persistence, generic replay failure, no parent-token fallback, and no secret-bearing audit detail
- destructive Telegram bot operations must require confirmation and write audit events
- usage history queries must be bounded and paged/aggregated; do not keep unbounded traffic history in process memory

## Certificate Automation

Certificate automation should support optional Let's Encrypt/ACME issuance and renewal.

Architecture expectations:

- ACME account state is explicit and persisted safely
- certificate private keys are high-sensitivity secrets
- node local API tokens are write-only through the API and encrypted in panel persistence
- node local API token changes and privileged node runtime/Xray actions are audited without secret values
- Telegram bot tokens are write-only through the API and encrypted in panel persistence
- admin 2FA TOTP secrets are encrypted in panel persistence
- Telegram delivery state is persisted with bounded attempts/backoff instead of an unbounded in-memory queue
- certificate issue/renew/revoke actions are auditable
- renewal jobs are bounded and visible
- failures create operator alerts before expiry
- node certificate deployment is revisioned where it affects runtime config

## Scan Defense

Scan defense should be implemented as a defensive hardening subsystem.

Input sources:

- structured xray/runtime logs
- node-reported invalid handshake events
- future firewall counters where available

Actions:

- temporary block
- permanent block only by explicit operator policy
- dry-run/no-op mode for unsupported systems

Rules:

- no unbounded log parsing into memory
- no hidden firewall mutation without audit
- every block must be reversible from the panel
- backend support should abstract over `iptables`, `nftables`, and no-op backends

## Routing Presets

Routing presets should be versioned, auditable config objects.

Expected behavior:

- operators can preview a diff before applying
- applying a preset creates a config revision
- presets can be exported/imported
- custom operator presets are separate from built-in presets

Initial preset families:

- direct domestic routing
- proxy foreign routing
- block private/reserved ranges
- block ads/malware domains where appropriate
- later streaming/game/social presets

## Protocol Expansion

Protocol support must be modeled as capabilities, not as loose strings.

Planned protocol targets:

- `Hysteria2`
- `WireGuard`
- `VLESS + TLS + WebSocket`
- `Trojan + TLS`
- `Shadowsocks + obfs/v2ray-plugin`

Design requirements:

- expose a protocol/mode only after validation, renderer output, apply-flow, and subscription rendering are defined
- reject unsupported protocol/transport/security combinations before apply
- expose runtime component metadata beside protocol capability metadata so operators and UI can see whether a protocol is disabled because its runtime lifecycle is missing
- protocol capability rows must expose runtime owner, required binaries, required secret classes, supported transports, supported security modes, and disabled reason
- non-Xray runtime owners such as sidecar or node-native must remain disabled in write/apply paths until lifecycle, supervision, validation, and update behavior are implemented
- keep key material out of generated public config and normal logs
- treat WireGuard private keys and peer keys as high-sensitivity material
- decide explicitly whether Hysteria2 is controlled through Xray, sidecar process management, or node-agent-native orchestration
- model plugin lifecycle for Shadowsocks obfs/v2ray-plugin instead of assuming the binary is present

Default product direction:

- prefer modern secure defaults such as VLESS/TLS/WebSocket, Trojan/TLS, and future WireGuard/Hysteria2 modes
- keep VMess for compatibility, but do not make it a recommended default unless a concrete deployment requires it

## Node Runtime Apply Safety

The panel-rendered `xray.json` is not enough to declare a node safe to restart.

Required apply chain:

- panel projects least-knowledge `node-runtime-config.json`
- panel or node-agent renders candidate `xray.json`
- internal schema/shape validation passes
- required route credentials are present and not revoked
- required runtime components are installed and healthy
- node-agent runs the real Xray binary validation, for example `xray run -test -config ...`
- node-agent reports the external validation result through sync/apply-result
- only a reported `passed` external validation can make `runtime_validation_report.safe_to_restart=true`

Missing external validation is intentionally treated as a warning plus restart blocker. This prevents the panel from approving a runtime restart based only on panel-side JSON generation.

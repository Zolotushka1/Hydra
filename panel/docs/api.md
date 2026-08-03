# API Reference

## Scope

This document describes the current HTTP API surface of `Hydra-Panel`.

It is a working backend contract, not a final public product spec.
Until the frontend is rebuilt, this file is the main reference for what the Rust panel currently exposes.

Base URL in local development:

- `http://127.0.0.1:8080`

## Route Table

`crates/panel-core/src/routes.rs` holds `ROUTE_TABLE`: one `RouteSpec` per method+path pair, 192 entries covering 163 distinct paths.

It is the single source of truth for the HTTP surface:

- `panel-app` builds the axum `Router` by iterating `ROUTE_TABLE`. There is no second list of routes;
- `GET /api/ui/contracts` is generated from the same constant, filtered to `exposure == admin_ui`.

Because both the served surface and the published contract are projections of one constant, they cannot disagree. Adding a route requires a `RouteId` variant, a `ROUTE_TABLE` row, and a `method_router` arm in `panel-app`; skipping any of the three is a compile error or a failing test, not a silent drift.

### Fields

| field | meaning |
| --- | --- |
| `id` | `RouteId` variant; exists so the handler binding in `panel-app` is an exhaustive `match` |
| `method` | `GET` / `POST` / `PUT` / `DELETE`; the router takes the method from here, so the table and the router cannot disagree on it |
| `path` | axum path pattern, e.g. `/api/nodes/{node_id}/apply-status` |
| `group` | functional area: what the endpoint is about |
| `exposure` | trust boundary: who may call it and with which credential |
| `paginated` | accepts `?limit=` and returns a bounded list |

### Exposure

`exposure` is assigned from the authentication a route actually enforces, not from its path prefix.

| exposure | count | credential |
| --- | --- | --- |
| `admin_ui` | 165 | admin session (`Authorization: Bearer <session>`) |
| `node_agent` | 13 | node token in `x-hydra-node-token` |
| `executor` | 6 | provisioning/installer executor token, in header or request body |
| `public` | 8 | none |
| `debug` | 0 | reserved; must not exist in a production build |

The distinction matters: `/api/installer/jobs/result` sits in group `installer` but authenticates with an executor token carried **in the request body**, so its exposure is `executor` and it is absent from the browser contract. Classifying by path prefix or by scanning for a header check would have published it as `public`.

### Contract projection

`GET /api/ui/contracts` publishes only `admin_ui` routes — 165 endpoints across 13 groups. The `node_agent` and `public` groups are deliberately absent: the frontend must not be built against an agent surface it cannot authenticate to.

## Schema Registry

`crates/panel-core/src/schemas.rs` holds `SchemaId` — every versioned schema in the product, with its number. Document constructors read their version from it, and `/api/ui/contracts` publishes the same registry, so the version in a document body and the version advertised to the frontend are the same value.

Before the registry these were two lists and had already diverged: the contract advertised `protocol_capabilities` version 1 while `ProtocolCapabilitiesView` emitted 3.

### Kinds

| kind | meaning | published |
| --- | --- | --- |
| `document` | one constructor emits it; that constructor must take its version from the registry | yes |
| `model` | a data model spanning several documents, with no single constructor | yes |
| `persistence` | on-disk file format | no |

`subscription_catalog` is a `model`. It is the only entry that is not tied to a constructor: nothing in the codebase ever emitted version 8, and the number describes the evolution of the catalog model (plans, clients, devices, enrollment grants) rather than the body of one document. `subscription_bundle` (version 1) is the actual document behind the `diagnostic_json` subscription format.

`route_material_store` and `reality_material_store` are `persistence` and are withheld from the contract — an on-disk format is not part of the browser API. `reality_material_store` holds Reality x25519 key pairs and short ids per inbound; private keys in it are encrypted with AES-256-GCM under their own master key, exactly like route materials.

### Registry

| schema | version | kind |
| --- | --- | --- |
| `ui_bootstrap` | 2 | document |
| `ui_overview` | 1 | document |
| `ui_contracts` | 3 | document |
| `resource_budget` | 1 | document |
| `subscription_client_access_preview` | 1 | document |
| `subscription_bundle` | 1 | document |
| `subscription_catalog` | 8 | model |
| `node_runtime_config` | 1 | document |
| `apply_plan` | 2 | document |
| `protocol_capabilities` | 5 | document |
| `node_runtime_validation` | 1 | document |
| `panel_access_modes` | 1 | document |
| `panel_install_plan` | 1 | document |
| `panel_installer_bootstrap` | 1 | document |
| `panel_installer_session` | 1 | document |
| `node_provisioning_executor_contract` | 1 | document |
| `route_material_store` | 1 | persistence (not published) |
| `reality_material_store` | 1 | persistence (not published) |

`schema_version` is `u16` everywhere. Three structs declared it `u32` before the registry; the field is a small counter and there is no reason for two widths.

## Versioning Policy

A `schema_version` exists so a client can tell whether it still understands a document. It is not a build counter: bump it when a client that was written against the previous version would now be wrong, and only then.

### Documents

| change | version | also required |
| --- | --- | --- |
| add an optional field | unchanged | — |
| add a required field | **bump** | changelog entry |
| rename a field | **bump** | changelog entry |
| remove a field | **bump** | changelog entry |
| change a field's type | **bump** | changelog entry |
| change a field's meaning while keeping its name and type | **bump** | changelog entry |
| reorder fields | unchanged | — |
| widen a numeric range within the same type | unchanged | — |

The row that gets missed is the last "bump" one. If `used_traffic_bytes` starts counting a different thing, nothing about the JSON changes shape and every mechanical check stays green — a client keeps parsing it and keeps being wrong. Silent meaning changes are the reason this is a written rule rather than a lint.

Adding an optional field is safe only if consumers ignore unknown fields. Rust consumers do this by default with `serde`; do not add `#[serde(deny_unknown_fields)]` to a type that crosses the API.

### Enums

| change | version | also required |
| --- | --- | --- |
| add a variant | unchanged | frontend must already have a fallback |
| rename a variant's serialized value | **bump** | changelog entry |
| remove a variant | **bump** | changelog entry |
| reorder variants | unchanged | — |

Adding a variant does not bump the version, which puts an obligation on the consumer: **every match on a contract enum must have a `_ => Unknown` arm.** A frontend without one breaks the first time the panel learns a new node status or audit event type. This is the deliberate trade — new variants ship without a coordinated release, in exchange for consumers handling the unknown case.

The obligation runs one way. Inside the panel, matches on these enums are exhaustive on purpose (see the enum registry), because the panel is where the variant is added.

Removing a variant is a break: a client that still sends it, or that renders it, is now wrong. Prefer keeping a dead variant marked as deprecated in the changelog over removing it.

### Models

`model` entries (currently only `subscription_catalog`) follow the same rules applied to the model as a whole: bump when a document belonging to that model changes in a way that would break a client reading the model.

### Changelog

Every bump adds a line under a `#### Changes` heading in this file, next to the endpoint that serves the document:

```markdown
#### Changes

- `3` — `runtime_components[].component` became an enum; unknown values are no longer accepted.
- `2` — `endpoints` changed from strings to objects.
```

Newest first. State what changed and what a client must do, not that something changed.

Golden snapshots make the mechanical half of this enforceable: a renamed or removed field fails `cargo test` and the failure message points here. A regenerated golden without a version bump and a changelog line is the thing to catch in review.

#### History before the registry

Version history predating the schema registry was never recorded. `ui_bootstrap` (2), `apply_plan` (2), `protocol_capabilities` (3) and `subscription_catalog` (8) carry numbers whose reasons are not documented anywhere in this repository, and they are not reconstructed here rather than guessed at. Changelog entries start from the first bump made under this policy.

## Deployment Modes

Primary mode is `Panel Standalone`.

In this mode the main server runs `Hydra-Panel` only. A separate `Hydra-node` process is not required for the panel host to generate `xray.json`, validate it, apply core runtime state, restart core, or run Xray update flows.

Remote-node mode is an extension.

Additional servers run `Hydra-node`, authenticate through `/api/node-agent/*`, receive node-scoped least-knowledge runtime config, and report health/sync/apply state back to the panel. Remote nodes are for scaling and multi-server routing, not a prerequisite for single-server operation.

## Authentication

### Admin API

Most `/api/...` routes require:

- `Authorization: Bearer <admin-session-token>`

Admin session token is returned by:

- `POST /api/admin/login`

### Node-Agent API

Node-agent routes require:

- `X-Hydra-Node-Token: <node-auth-token>`

The plaintext node token is only returned at rotation time:

- `POST /api/nodes/{node_id}/auth/rotate`

The panel stores only the token hash.
Freshly created nodes expose `auth_token_issued_at_unix=null`; this means no deployable plaintext token has been issued yet.
After rotation, `auth_token_issued_at_unix` is set and the previous token becomes invalid immediately.

Node auth token lifecycle:

1. Create node metadata with `POST /api/nodes`.
2. Issue the deployable token once with `POST /api/nodes/{node_id}/auth/rotate`.
3. Install that token into the node-agent secret store.
4. If the token is lost, suspected compromised, or the node is reprovisioned, rotate again and redeploy the returned token.
5. Disabled nodes cannot authenticate against `/api/node-agent/*`, even with a previously valid token.

Node-agent steady-state loop:

1. authenticate with `X-Hydra-Node-Token`;
2. send heartbeat;
3. fetch `/api/node-agent/config`;
4. if required, fetch `/api/node-agent/route-credentials`;
5. render or fetch `/api/node-agent/xray-config`;
6. validate before runtime activation;
7. report sync/apply-result;
8. on transport failure, use bounded retry/backoff and report secret-free `retry_state` on the next successful sync.

## Contract Tests

Four mechanisms keep this document and the served API from drifting apart. All live in `panel-core`.

### Golden snapshots

13 admin-surface documents are snapshotted to `crates/panel-core/golden/*.json` and compared on every test run: `ui_contracts`, `ui_bootstrap`, `ui_overview`, `ui_security`, `ui_users`, `ui_clusters`, `ui_telegram`, `ui_audit`, `ui_subscriptions`, `ui_installer`, `ui_protocols`, `protocol_capabilities`, `panel_access_modes`.

They catch what `cargo check` cannot: a renamed or removed field still compiles, and still breaks a frontend bound to the old name.

Volatile values are normalized to `"<volatile>"` — timestamps, ids, tokens, hashes, revisions, paths, and live host measurements (`*_bytes`, `*_percent`, uptime). **Field names are never normalized**, which is the point. In `ui_bootstrap` only 12% of leaves are normalized; the other 88% are pinned literally, so enum values, defaults and booleans are covered too.

Regenerating:

```bash
HYDRA_UPDATE_GOLDEN=1 cargo test -p panel-core golden
```

An updated golden file must land in the same commit as the change that caused it. Reviewing that diff is how a contract change gets noticed.

### Docs drift

Every `admin_ui` route in `ROUTE_TABLE` must appear somewhere in this file. Adding a route without documenting it fails the build.

### Secret guard

Every golden snapshot is walked as `serde_json::Value` before comparison:

- a key containing `token`, `secret`, `password`, `private_key`, `hash`, `hmac` or `credential` must not carry a non-empty string;
- no string value may look like credential material — a PEM block, or a 32+ character run of base64/hex-shaped characters with mixed case and digits.

The check is on **shape, not substrings**. `node_auth_token_rotated` is an audit event type and `/api/system/secret-readiness` is a route; a substring check flags both, which is why the earlier version of this guard broke as soon as the enum registry was published. The guard itself is tested against known-bad inputs, so it cannot silently degrade into a no-op.

### Enum and schema parity

Covered by the registries — see [Enum registry](#enum-registry) and [Schema Registry](#schema-registry).

## Unset Fields Are Omitted, Never Null

Xray treats an absent key differently from a key holding a value — the removed
`allowInsecure` is rejected by presence alone. Optional fields are therefore omitted
from Xray-facing JSON rather than serialised as `null`.

Documents in `panel-domain/src/xray.rs` use `skip_serializing_if = "Option::is_none"`;
the hand-assembled raw config uses `insert_if_some`, because `json!` with an `Option`
writes `null`. Clients reading `/api/core/xray-config` will see previously-`null` keys
(such as `serverName` on an inbound without SNI) simply absent.

## Startup Refusals

The panel refuses to start, naming the path, when:

- a persisted file exists but does not parse (never replaced with an empty default);
- a secret-class file is wider than `0600`;
- a data directory is wider than `0700`.

Master keys are written once, so a key created with a foreign umask — a backup restore, a
manual `chmod` — would otherwise stay world-readable indefinitely. The panel reports it
instead of repairing it silently: the operator needs to know the secret was exposed.

## Reality And uTLS

### Reality

`security: reality` on a host is rendered end to end, not just modelled.

The panel generates an x25519 key pair and a short id per inbound on first use and keeps them in `reality_material_store`. Private keys there are encrypted with AES-256-GCM under their own master key (`HYDRA_REALITY_MATERIALS_MASTER_KEY_B64`), written atomically with `0600`. The public half is never stored — it is derived from the private key on read, so the two cannot drift apart.

Private keys reach a node through `GET /api/node-agent/route-credentials`, in the added optional `reality_materials` field. That route has `node_agent` exposure, so the key is never part of the admin surface, never appears in `/api/core/xray-config`, and is out of scope for the secret guard by construction. The field is additive with `#[serde(default)]`; no schema version was bumped.

The node renders `realitySettings` itself, substituting the key the same way it already substitutes certificate file paths:

```json
"security": "reality",
"realitySettings": {
  "dest": "www.microsoft.com:443", "xver": 0,
  "serverNames": ["www.microsoft.com"],
  "privateKey": "...", "shortIds": ["65e44873e150a969"]
}
```

`dest` and `serverNames` are derived from the host SNI rather than configured separately: Reality masquerades as the site whose name it presents, and a second source of truth for that pair would drift.

A host may serve only one Reality inbound. The material is per-inbound while the public
half lives on the host, so a host covering two Reality inbounds has no single answer and
is refused fail-closed rather than served the first one's keys. Host-to-inbound scoping
goes through node and cluster: both sides named must match, an unnamed side is shared,
and node-scoped against cluster-scoped does not match.

Subscription links carry the public half — `security=reality`, `sni`, `pbk`, `sid`. A host declared `reality` without SNI, public key or short id is refused fail-closed: no link is issued. A link missing `pbk` would fail only at the user's client, which is the wrong place to find out.

### uTLS fingerprint

Every `tls` and `reality` link carries `fp=chrome`, and the client config block carries `tls_fingerprint`. Without it Xray presents Go's `crypto/tls` handshake, whose JA3/JA4 matches no browser and is fingerprinted automatically.

The Rust identifier is `TLS_FINGERPRINT` / `tls_fingerprint`, deliberately prefixed: `fingerprint` in `panel-core` already means the subscription device fingerprint, which is an unrelated entity.

Plain (`security=none`) links carry no `fp` — there is no TLS handshake to disguise.

## Deployment Scenarios

`protocol_capabilities` carries `deployment_scenarios[]` on top of the capability rows.

The rows answer "what is technically possible"; a grid of protocol/transport/security
checkboxes does not tell an operator what to pick. The scenarios answer that, and they
are a **projection, not a second model**: each one points at an existing
production-ready row, and a test refuses any scenario whose
`protocol + transport + security` triple is missing from the matrix or not marked
production-ready. A second test runs each scenario through the panel's own XHTTP
validation, so a scenario cannot advertise a configuration we would reject.

| scenario | transport | security | flow | XHTTP mode | CDN |
| --- | --- | --- | --- | --- | --- |
| Direct, max stealth *(recommended)* | `xhttp` | `reality` | `xtls-rprx-vision` | `stream-one` | no |
| Direct, max throughput | `tcp` | `reality` | `xtls-rprx-vision` | — | no |
| Behind CDN | `xhttp` | `tls` | — | `packet-up` | yes |

The trade-offs, carried in each scenario's `rationale`:

- **max stealth** — Reality hides the handshake and defeats active probing, XHTTP hides
  the connection profile after it. Cost: `stream-one` is the slowest XHTTP mode.
- **max throughput** — Vision over TCP splices on Linux and is markedly cheaper on CPU,
  which matters at 1 vCPU. Cost: the post-handshake profile is not disguised.
- **behind CDN** — `packet-up` is the only mode guaranteed through a CDN and Nginx.
  Reality is unavailable there because the CDN terminates TLS, and Vision is
  incompatible with `packet-up`.

Vision appears in two scenarios with different transports and is absent in the third.
That is only expressible because Vision is modelled as `flow`, not as a transport.

Adding the scenario layer surfaced a real gap: `tcp + reality` — the most common Reality
deployment — was missing from the VLESS capability rows. The consistency test found it.

## XHTTP

`xhttp` splits upstream and downstream into separate HTTP transactions, so each looks
like an ordinary request/response pair. Reality hides the handshake; XHTTP hides the
connection profile *after* it. They cover different detection layers and are used
together, not instead of one another.

Rendered as `xhttpSettings`: `path`, `mode`, and `extra.xPaddingBytes` (`100-1000`) —
padding normalises packet sizes, which otherwise stay tunnel-shaped even behind a
disguised handshake.

### Modes

| mode | Vision | behind CDN / Nginx |
| --- | --- | --- |
| `auto` | resolves to `stream-one` under Reality | no |
| `packet-up` | **no** | **yes, the only guaranteed mode** |
| `stream-up` | no | no |
| `stream-one` | **yes, the only compatible mode** | no |

`auto` is emitted resolved, not literally: under Reality Xray picks `stream-one`, and
publishing `auto` would leave the operator guessing which mode is actually in force.

Vision is not a transport — it is the `flow: xtls-rprx-vision` flag, and it lives
inside XHTTP `stream-one` as well as on plain TCP. The axis of choice is `tcp` vs
`xhttp`, never "Vision vs XHTTP".

### Enforced by validation, not by documentation

`xray run -test` accepts every combination below, verified against 26.6.27. It does
not enforce any of them, so the panel must:

- `flow: xtls-rprx-vision` with a mode other than `stream-one` → rejected;
- Reality behind a CDN → rejected (the CDN terminates TLS, so handshake substitution cannot work);
- behind a CDN with a mode other than `packet-up` → rejected;
- behind a CDN with Vision → rejected, because a CDN forces `packet-up`;
- `xhttp_mode` set on a non-XHTTP transport → rejected rather than silently ignored;
- `auto` + Reality → accepted with an `info` issue stating the resolved mode.

### Version matching

`GET /api/ui/protocols` carries `xray_version`, read from the panel's own binary.

XHTTP is under active development and server and client versions must match; a
mismatch fails without a useful error. The field is a working condition, not a
convenience. It is read per request on purpose: the Xray update flow swaps the binary
without restarting the panel, and a cached value in a field whose entire purpose is
detecting mismatches would be worse than no value.

## Pagination

Every list endpoint takes `?limit=` and nothing else. There is no `offset` and no cursor.

Rules, applied in one place (`resolve_limit` in `panel-core`):

- omit `limit` and you get the configured maximum;
- `limit` can only **narrow** the result. Asking for more than the maximum clamps to the maximum;
- `limit=0` is meaningless and is raised to `1`.

The maximum comes from `runtime_limits`: a collection with its own buffer cap uses that cap (`max_operational_log_lines_buffered`, `max_audit_events_buffered`, `max_alert_events_buffered`, …); everything else uses `max_list_page_size`, default `200`.

### Why no offset

At a 512 MB budget, deep paging is not reachable in the first place, and an `offset` parameter would advertise a capability the panel cannot honour. Narrowing a result is the job of the filters each endpoint already exposes — `search`, `status`, `kind`, `verdict`, time windows — not of a sliding window over an unbounded set.

This was not previously uniform. Seven list endpoints — `/api/users`, `/api/subscription-plans`, `/api/subscription-plans/{plan_id}/clients`, `/api/system/logs`, `/api/system/alerts/history`, `/api/subscription-clients/{client_id}/devices`, `/api/subscription-clients/{client_id}/device-enrollments`, `/api/subscription-clients/{client_id}/sessions` — returned the **entire** collection when `limit` was omitted, and four more accepted any `limit` without an upper clamp.

Internal full enumeration (config generation needs every user and client) goes through dedicated `all()` / `all_clients()` methods, so it cannot be confused with a paged API read.

Routes that accept `limit` are marked `paginated: true` in `ROUTE_TABLE` and published with that flag in `/api/ui/contracts`.

## Common Notes

- Most successful write operations return `200 OK` with JSON or `204 No Content`
- Validation failures return `400`
- missing auth returns `401`
- active IP ban may return `429`
- internal persistence failures return `500`

## Health

### `GET /health`

Returns a minimal health payload for the panel process.

## Admin / Security

### `POST /api/admin/login`

Body:

```json
{
  "username": "admin",
  "password": "secret",
  "two_factor_code": "123456",
  "challenge_token": "optional-for-2fa-2step"
}
```

Behavior:

- authenticates admin credentials
- enforces login protection and smart bans
- enforces 2FA when enabled
- supports `2FA 2-step`

### `POST /api/admin/logout`

Invalidates the current admin session token.

### `GET /api/admin/me`

Returns the authenticated admin session view.

### `GET /api/admin/sessions`

Returns active admin sessions.

Session tokens are never returned. Each item contains a stable `session_id` derived from the token hash, plus username, client IP, issue time, and expiry time.

### `POST /api/admin/sessions/{session_id}/revoke`

Revokes an active admin session by public `session_id`.

This endpoint can revoke the current session too. In that case the current token becomes invalid immediately after the request completes.

## UI Bootstrap

### `GET /api/ui/bootstrap`

Returns a safe startup snapshot for the dashboard.

This endpoint is intended for the future Leptos CSR frontend so initial page load does not need to fan out into many requests. It returns only lightweight state and aggregate counts, not generated config documents, Xray JSON, private keys, node auth tokens, subscription tokens, or route credential material.

Payload includes:

- authenticated admin session view
- security settings and 2FA state
- active ban count
- active admin session count
- system overview and active alerts
- core runtime state, generated revision, and config validity metadata
- user counts by status
- node counts by status / sync / provisioning state
- cluster counts by status
- Telegram delivery summary and public Telegram settings
- audit buffer summary

Current schema version: `3`.

The bootstrap endpoint intentionally returns summaries, not large lists. UI pages should use dedicated bounded list endpoints for audit, Telegram events, nodes, users, and logs.

### `GET /api/ui/overview`

Returns a compact polling snapshot for the future dashboard overview page.

This endpoint is intended for live UI refresh of CPU/RAM/disk-style system data, core status, aggregate counters, node-health summary, Telegram delivery summary, and audit summary without reloading the heavier bootstrap payload.

Payload includes:

- `schema_version`
- `checked_at_unix`
- `system`
- `core`
- `users`
- `nodes`
- `clusters`
- `node_health`
- `node_health_recommendations`
- `telegram`
- `audit`

Current schema version: `1`.

Contract rules:

- this endpoint is authenticated
- it must stay bounded and safe for frequent polling
- it must not include generated config documents, Xray JSON, subscription tokens, node auth tokens, private keys, SSH secrets, or route credential material
- detailed pages should still use dedicated bounded endpoints for large lists and logs

### `GET /api/ui/contracts`

Returns the frontend-safe API contract snapshot for the future dashboard.

Payload includes:

- `schema_version`
- `checked_at_unix`
- `api_version`
- `schemas`
- `endpoint_groups`
- `enums`

Current schema version: `2`.

`endpoint_groups[].endpoints` is generated from `ROUTE_TABLE` filtered to `exposure == admin_ui` (see [Route Table](#route-table)). Each entry is an object:

```json
{ "method": "GET", "path": "/api/nodes/{node_id}/apply-status", "paginated": false }
```

Contract rules:

- this endpoint is authenticated
- it must stay bounded and safe for startup/runtime checks
- it exposes schema versions, endpoint groups, and enum values intended for UI feature gating
- endpoints are never hand-listed here; anything not in `ROUTE_TABLE` cannot appear, and anything in it with `admin_ui` exposure cannot be omitted
- it must not include generated config documents, Xray JSON, subscription tokens, node auth tokens, private keys, SSH secrets, or route credential material

#### Changes to `protocol_capabilities`

- `5` — also added the `xhttp` transport and the `xhttp_mode` enum (`auto`, `packet-up`, `stream-up`, `stream-one`), plus two VLESS capability rows for XHTTP. Adding enum variants is additive and would not need a bump on its own; it rides the same version as the protocol removal because both landed together. A client need do nothing to keep working, but will not see XHTTP until it re-reads the document.
- `5` — VMess, Trojan and Shadowsocks were removed. `ProxyType` now has three variants (`vless`, `hysteria2`, `wireguard`), and the capability rows, credential derivation, validation, renderers and subscription link schemes for the three protocols are gone. **A client must drop any UI that offers them and must not send `proxy_type` values outside the remaining three** — a persisted profile naming a removed protocol now fails to load, by design (see below). VMess has no masquerade of its own and a recognizable TLS handshake; Trojan is passively detectable as TLS-in-TLS above 70% (USENIX Security 2024) while costing more to deploy than Reality; Shadowsocks never produced a valid config at all, because AEAD-2022 requires an inbound-level PSK that was never modelled.

- `4` — obfs / v2ray-plugin support was removed. `ProtocolSecurityMode::Obfs` no longer exists, the Shadowsocks capability row lists only `none`, and the `requires_plugin` field is gone from `ProtocolModeCapability`. A client that rendered an obfs option or read `requires_plugin` must drop both. obfs gave no resistance to active probing, the Xray-side runtime for it was never implemented, and Reality covers the niche.

#### Enum registry

`enums[]` is generated from `crates/panel-domain/src/registry.rs`. Each registered type gets `pub const ALL: &[Self]` through the `enum_registry!` macro, and the published values are the serde serialization of `ALL` — not a parallel list of literals.

45 types are registered, covering 302 variants. Nothing in this section is hand-written any more.

Four of them were `String` fields until the registry was introduced and are now real types:

| value | was | now |
| --- | --- | --- |
| `runtime_component` | path segment validated by string `matches!` | `RuntimeComponent`, parsed at the boundary |
| `runtime_component_action` | path segment validated by string `matches!` | `RuntimeComponentAction`, parsed at the boundary |
| `node_health_flag` | `Vec<String>` built by 17 `push("literal")` calls | `Vec<NodeHealthFlag>` |
| `apply_timeline_stage` | `phase: String` | `NodeApplyTimelineStage` |

The serialized strings are byte-identical to the previous literals, so this is not a wire change and no schema version was bumped for it.

`NodeReportedRuntimeComponentView.component` is deliberately left as `String`: it carries data reported *by* a node agent, and an unknown component name must not fail the whole report. Be strict about what the panel emits, lenient about what it accepts — except at the URL boundary, which is an authorization surface and is strict.

Adding a variant to a registered enum without adding it to the registry is a compile error (`non-exhaustive patterns`), not a missing contract value. The parity test additionally checks that each value round-trips back to its variant and that no two variants collapse to the same string.

#### Changes

- `3` — the `protocol` enum lost `vmess`, `trojan` and `shadowsocks`. The document shape is unchanged; only registry content shrank. Bumped anyway: a client holding a cached contract would keep offering those protocols and keep sending values the panel now rejects, and the version is its only signal to re-read. This is the "removed enum variant" rule from [Versioning Policy](#versioning-policy) applied to the document that publishes the registry.
- `2` — `endpoints[]` changed from `"GET /path"` strings to `{method, path, paginated}` objects: re-parse this array rather than splitting strings. The document now covers only the browser-callable surface — `node_agent` and public routes were removed, so a client must not expect to find them here. Everything else in this bump is additive and needs no client change: endpoints went from ~60 hand-written entries to 168 generated ones (including the corrected `GET /api/nodes/{node_id}/apply-status`, previously advertised as `/apply/status`), `schemas` from 6 to 16, and `enums` from 29 to 45 — among them `panel_installer_payload_kind`, which regained the `dependency_install` value the hand-written list had lost.

### Compact UI Endpoints

These endpoints return the same safe summary slices used by bootstrap/overview, but separately:

- `GET /api/ui/security`
- `GET /api/ui/core`
- `GET /api/ui/users`
- `GET /api/ui/nodes`
- `GET /api/ui/clusters`
- `GET /api/ui/telegram`
- `GET /api/ui/audit`
- `GET /api/ui/subscriptions`
- `GET /api/ui/protocols`
- `GET /api/ui/installer`

#### `GET /api/ui/subscriptions`

Counters for the subscription catalog: plans, clients by status, clients expiring within 24 hours, clients that have reached their data limit, devices by status, enrollment grants by status.

Counters only. The catalog is the largest collection in the panel, so no list is returned here — use `/api/subscription-plans` and `/api/subscription-plans/{plan_id}/clients`, both paged.

#### `GET /api/ui/protocols`

Compact projection of `protocol_capabilities` for gating inbound/host forms: protocol, display name, status, `available`, `disabled_reason`, supported transports and security modes.

Carries `capabilities_schema_version` so the frontend can tell when the full matrix at `/api/protocol-capabilities` is worth re-reading. The full matrix also lists per-mode capabilities, required binaries and secret classes, which is too heavy to fetch on every form render.

Bounded by construction: the protocol list is fixed by the capability matrix and does not grow with data.

#### `GET /api/ui/installer`

State for the first-run wizard: whether TLS material is configured, the available access modes, the recommended one, and installer job counts by status with the latest job timestamp.

No job ids, executor tokens, or bootstrap payloads.

Contract rules:

- all endpoints are authenticated
- all responses must stay bounded
- summary endpoints must not return large lists
- summary endpoints must not return generated config documents, Xray JSON, subscription tokens, node auth tokens, private keys, SSH secrets, or route credential material
- list/detail pages should use dedicated paged/bounded API routes

### `GET /api/admin/security/settings`

Returns current security settings.

### `PUT /api/admin/security/settings`

Updates:

- login protection toggle
- smart ban toggle
- `X-Forwarded-For` trust settings
- trusted proxy IPs/CIDRs
- failed attempts / attempt window / block duration
- session TTL

Forwarded client IP resolution is fail-closed:

- forwarded headers are ignored unless `trust_x_forwarded_for=true`;
- the direct TCP peer must match `trusted_proxy_ips` or `trusted_proxy_cidrs`;
- malformed `X-Forwarded-For` chains are ignored and the direct peer IP is used;
- valid chains are resolved right-to-left: trusted proxy hops are stripped from the right side and the nearest untrusted hop becomes the client IP.

Successful updates create a `security_settings_updated` audit event.
Rejected authenticated updates create a `security_settings_update_failed` audit event without storing the submitted settings payload.

### `POST /api/admin/security/preset`

Applies an operator security preset and returns the resulting security settings.

Body:

```json
{
  "preset": "strict"
}
```

Supported presets:

- `standard`
- `strict`
- `paranoid`

Preset application preserves existing trusted proxy IP/CIDR lists so reverse-proxy deployments are not broken by a one-click profile change.

### `GET /api/admin/security/status`

Returns current login protection state for the current client IP.

### `GET /api/admin/security/audit`

Query:

- `event_type`
- `username`
- `client_ip`
- `search`
- `created_from_unix`
- `created_to_unix`
- `limit`

Returns recent security audit events.

The `limit` is bounded by the configured in-memory audit buffer size. This endpoint is intended for operator UI inspection and must not be treated as an unbounded export API.

### `GET /api/admin/security/bans`

Returns active IP bans.

Temporary ban entries include `remaining_seconds`. Permanent bans return `remaining_seconds: null`.
Expired temporary bans are evicted before the response is built.
Each entry includes:

- `client_ip`
- `ban_kind`: `temporary` or `permanent`
- `source`: `automatic` or `manual`
- `reason`
- `created_at_unix`
- `blocked_until_unix`
- `ban_level`: smart-ban level for automatic bans, `0` for manual bans
- `remaining_seconds`

### `POST /api/admin/security/bans`

Creates a manual IP ban.

Body:

```json
{
  "client_ip": "203.0.113.10",
  "ban_kind": "temporary",
  "duration_seconds": 600,
  "reason": "repeated scanner"
}
```

Rules:

- `client_ip` must be a valid IP address;
- temporary bans require `duration_seconds`;
- temporary manual bans are limited to `2592000` seconds;
- permanent bans must be requested explicitly with `ban_kind: "permanent"`;
- `reason` is optional, trimmed, limited to 256 characters, and cannot contain control characters.

### `POST /api/admin/security/bans/{client_ip}`

Removes an active IP ban.

## Two-Factor Authentication

### `GET /api/admin/2fa/state`

Returns current 2FA state.

### `POST /api/admin/2fa/setup`

Generates or returns setup material for panel-wide 2FA.

### `POST /api/admin/2fa/enable`

Body:

```json
{
  "code": "123456",
  "two_step_enabled": false
}
```

Enables 2FA after TOTP verification.

### `POST /api/admin/2fa/disable`

Body:

```json
{
  "code": "123456"
}
```

Disables 2FA and rotates state.

### `POST /api/admin/2fa/two-step`

Body:

```json
{
  "enabled": true
}
```

Toggles the `2FA 2-step` mode.

## Telegram

### `GET /api/telegram/settings`

Returns public Telegram settings state.

Important:

- does not return plaintext `bot_token`
- returns only `bot_token_configured`
- persisted `bot_token` is encrypted at rest in `telegram-settings.json`
- the Telegram secret master key is loaded from `HYDRA_TELEGRAM_SECRETS_MASTER_KEY_B64` or from `HYDRA_TELEGRAM_SECRETS_KEY_PATH` / `data/telegram-secrets.key`

Admin 2FA secret persistence:

- the panel stores the admin TOTP secret encrypted at rest in `admin.json`
- the admin secret master key is loaded from `HYDRA_ADMIN_SECRETS_MASTER_KEY_B64` or from `HYDRA_ADMIN_SECRETS_KEY_PATH` / `data/admin-secrets.key`

### `PUT /api/telegram/settings`

Body:

```json
{
  "enabled": true,
  "bot_token": "123:abc",
  "default_chat_id": "123456789",
  "notify_on_security_events": true,
  "notify_on_system_alerts": true,
  "notify_on_node_events": true,
  "notify_on_node_health_alerts": true,
  "alert_policy": {
    "enabled": true,
    "notify_on_activation": true,
    "notify_on_resolution": true,
    "min_severity": "warning",
    "included_alert_kinds": [
      "disk_usage",
      "memory_usage",
      "panel_memory_budget",
      "node_offline",
      "node_stale_heartbeat",
      "node_config_drift",
      "node_provisioning_stale",
      "node_provisioning_failed",
      "node_reported_apply_failed",
      "node_runtime_alert"
    ],
    "cooldown_seconds": 300
  }
}
```

Updates Telegram delivery settings.

If Telegram is already configured, `bot_token` may be omitted while `enabled` remains `true`; the existing encrypted token is preserved.
If no token is configured yet, enabling Telegram requires a non-empty `bot_token`.
Settings changes emit `telegram_settings_updated`. Audit detail records booleans such as whether a token/chat id was supplied, but never the plaintext bot token, chat id, or alert-policy payload.

`notify_on_node_events` controls general node CRUD/operator events.
`notify_on_node_health_alerts` controls node health alert notifications such as offline nodes, stale heartbeat, config drift, failed provisioning, node-reported apply failures, and node-reported runtime alerts.

`alert_policy` controls which alert events are eligible for notification:

- `enabled=false` suppresses alert notifications while preserving alert history.
- `notify_on_activation` controls activated alert delivery.
- `notify_on_resolution` controls resolved alert delivery.
- `min_severity` is `warning` or `critical`.
- `included_alert_kinds` is an allowlist of alert kinds.
- `cooldown_seconds` suppresses repeated notifications for the same alert kind and event status; max value is `86400`.

Node health Telegram messages are emitted only when the shared alert pipeline activates or resolves a node health alert, not on every polling request.

### `GET /api/telegram/events`

Query:

- `kind`
- `status`: `queued`, `delivered`, `retry_scheduled`, `skipped`, `failed`
- `limit`

Returns Telegram delivery/test history.

Public Telegram event responses are redacted:

- `target_chat_id` is masked
- `message` and `last_error` are bounded and redacted for obvious token/password/private-key lines
- Telegram Bot API URLs in errors redact the bot token segment

Telegram delivery fields:

- `alert_kind`: present when this Telegram event was created from a system/node alert
- `alert_severity`: present when this Telegram event was created from a system/node alert
- `alert_status`: `activated` or `resolved` when this Telegram event was created from a system/node alert
- `attempt_count`: how many delivery attempts were made
- `last_error`: last delivery failure reason, if any
- `next_retry_at_unix`: when the event becomes eligible for retry
- `delivered_at_unix`: successful delivery time, if delivered

Delivery retry policy is bounded:

- first failure retries after 30 seconds
- second failure retries after 120 seconds
- third failure retries after 600 seconds
- later retryable failures retry after 1800 seconds
- after 5 attempts the event becomes permanently `failed`

### `POST /api/telegram/retry-due`

Retries due Telegram events with status `retry_scheduled`.

This action is bounded by persisted event state and does not create an unbounded background queue. It returns the events that were retried and their updated delivery state.
The request emits `telegram_retry_due_requested`; audit detail contains only the retry count.

### `POST /api/telegram/test`

Body:

```json
{
  "message": "optional",
  "chat_id": "optional"
}
```

Sends a test message through the same Telegram delivery pipeline as operational notifications.
The request emits `telegram_test_message_requested`; audit detail contains only the event id.

## Installer / Panel Access Modes

These endpoints are the backend contract for managed panel installation and the
future first-run setup UI.

Planning, bootstrap, job creation, and job listing require an admin session.
Executor session, heartbeat, and result routes instead require the job id and
one-time executor token. First-host installation cannot use a managed job because
no panel exists yet; its future local bootstrap must use the same typed contract
without exposing a public unauthenticated setup endpoint.

Linux first-host installation is intentionally not an HTTP endpoint. The local
`panel-installer-executor` accepts `HYDRA_INSTALLER_MODE=first_host`, validates
the release artifact and plan through `panel-core`, executes the same command
envelopes, and validates the result locally. `HYDRA_INSTALLER_DRY_RUN=1`
prints the secret-free session without host mutation.

### `GET /api/installer/access-modes`

Returns supported panel access modes:

- `domain_tls`: recommended production mode with domain + trusted HTTPS.
- `ip_http`: quick IP-only HTTP mode for operators without a domain.
- `ip_self_signed_tls`: IP-only HTTPS mode with generated self-signed certificate and fingerprint verification.
- `reverse_proxy`: operator-managed reverse proxy/TLS mode.

Each option includes:

- `recommended`
- `requires_domain`
- `tls_required`
- operator-facing `description`
- non-secret `warnings`

### `POST /api/installer/plan`

Builds a dry-run install/access-mode plan.

This endpoint does not modify the machine, write certificates, open firewall ports, or restart services.
It validates input and returns the exact high-level steps, warnings, required confirmations, certificate plan, reverse-proxy trust plan, and security posture.

Body:

```json
{
  "access_mode": "ip_self_signed_tls",
  "domain": null,
  "public_ip": "203.0.113.10",
  "bind_host": "0.0.0.0",
  "bind_port": 2053,
  "acme_email": null,
  "firewall_allowlist": ["198.51.100.7/32"],
  "trusted_proxy_cidrs": [],
  "confirm_public_http": false
}
```

Important behavior:

- `domain_tls` requires a valid domain without scheme/path.
- `ip_http` on public bind requires explicit `confirm_public_http`.
- `ip_http` reports `danger_plain_http_public` security posture.
- `ip_self_signed_tls` returns a self-signed certificate plan and requires fingerprint verification.
- `reverse_proxy` binds to `127.0.0.1:8080` by default and does not trust forwarded headers unless `trusted_proxy_cidrs` is provided.
- `firewall_allowlist` entries must be valid IP addresses or CIDR ranges.
- `trusted_proxy_cidrs` entries must be valid CIDR ranges.

Response fields:

- `access_mode`
- `security_posture`
- `public_url`
- `bind_address`
- `requires_confirmation`
- `required_confirmations`
- `warnings`
- `hardening_defaults`
- `certificate_plan`
- `reverse_proxy_plan`
- ordered `steps[]`

### `POST /api/installer/bootstrap`

Builds a one-line installer bootstrap contract from a validated plan.

Body:

```json
{
  "target_os": "linux",
  "target_arch": "x86_64",
  "package_channel": "stable",
  "release_manifest": {
    "manifest_version": 1,
    "signature_url": "https://downloads.example.test/hydra/manifest.sig",
    "signing_key_fingerprint": "ABCD1234ABCD1234ABCD1234ABCD1234ABCD1234",
    "artifacts": [
      {
        "name": "hydra-panel-linux-x86_64.sh",
        "artifact_kind": "installer_script",
        "target_os": "linux",
        "target_arch": "x86_64",
        "package_channel": "stable",
        "version": "1.2.3",
        "url": "https://downloads.example.test/hydra/linux-x86_64/install.sh",
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      },
      {
        "name": "hydra-panel-linux-x86_64",
        "artifact_kind": "panel_binary",
        "target_os": "linux",
        "target_arch": "x86_64",
        "package_channel": "stable",
        "version": "1.2.3",
        "url": "https://downloads.example.test/hydra/linux-x86_64/hydra-panel-linux-x86_64",
        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      }
    ]
  },
  "plan": {
    "access_mode": "domain_tls",
    "domain": "panel.example.com"
  }
}
```

Behavior:

- returns Linux or Windows command snippets for the selected target OS;
- command snippets download the installer to a temporary file, verify SHA-256, and only then execute it;
- includes the supported platform matrix: CentOS 7/8/9, Debian 10/11/12, Ubuntu 18.04/20.04/22.04/24.04, Windows 10/11/Server 2019/Server 2022, AlmaLinux 8/9, and Astra Linux;
- supports `x86_64` and `aarch64` target architecture selection;
- can select installer URL, version, SHA-256, and signature metadata from `release_manifest`;
- `release_manifest.manifest_version` must be `1`;
- manifest artifacts are typed as `installer_script`, `panel_binary`, or `node_binary`;
- one-line bootstrap selects only an `installer_script`;
- manifest artifacts must match selected `target_os`, `target_arch`, `package_channel`, and `pinned_version` when pinned mode is used;
- Linux installer scripts must be `.sh`, Windows installer scripts must be `.ps1`, Windows binary artifacts must be `.exe`, and Linux binary artifacts must not use Windows extensions;
- requires HTTPS `installer_script_url`, except localhost HTTP for local development;
- requires `artifact_verification.sha256` before `ready_to_run=true`;
- optional detached signature metadata requires a pinned `signing_key_fingerprint`;
- supports `stable`, `latest`, and `pinned` package channels;
- `pinned` requires `pinned_version`;
- command snippets are secret-free and must not contain admin passwords, 2FA seeds, private keys, node tokens, or route credentials;
- if no manifest artifact or manual `installer_script_url` + `artifact_verification.sha256` is available, response is returned as a template with `ready_to_run=false`.
- tracked managed jobs additionally require a matching `panel_binary`; the
  installer script is never reused as the executable payload.

Current limitation:

- this endpoint does not execute installation or publish artifacts;
- managed execution is performed by the separate `panel-installer-executor`;
- detached signature metadata is rejected by the executor until a real verifier is implemented.

### `POST /api/installer/jobs`

Creates a tracked panel installer job from a ready bootstrap contract.

Auth:

- admin session required.

Behavior:

- rejects bootstrap contracts where `ready_to_run=false`;
- requires compatible executor contract version;
- creates a bounded-lifetime installer job;
- returns `executor_token` once;
- stores and persists only a hash of the executor token internally;
- job view does not serialize the plaintext executor token;
- emits `panel_installer_job_created` audit event with the job id and without the one-time executor token.

### `GET /api/installer/jobs`

Returns current installer jobs for the admin UI.

Auth:

- admin session required.

### `POST /api/installer/jobs/executor-session`

Returns the persisted job/session to the managed installer executor.

Auth:

- no admin session;
- requires the exact `job_id` and one-time `executor_token` in the JSON body.

The route is read-only. It rejects invalid tokens using constant-time hash
comparison and rejects expired or terminal jobs. The returned session contains a
validated `panel_binary` payload, never the bootstrap script as the install
binary.

Current platform gate: the executor performs real host mutation only on Linux.
Windows sessions fail before the first operation until Windows service
environment, ACL, and certificate recipes are production-ready.

### `POST /api/installer/jobs/heartbeat`

Executor heartbeat endpoint.

Auth:

- no admin session;
- requires `job_id` and one-time `executor_token` from job creation.

Body:

```json
{
  "job_id": "panel-installer-job-...",
  "executor_token": "one-time-token",
  "observed_phase": "install_binary",
  "message": "downloaded and verifying artifact"
}
```

### `POST /api/installer/jobs/result`

Executor result endpoint for a tracked installer job.

Auth:

- no admin session;
- requires `job_id` and one-time `executor_token`.

Important behavior:

- executor submits only `command_results`;
- panel derives `expected_command_ids` from the saved session;
- panel derives `expected_operation_ids` from saved command envelopes and requires matching per-operation results;
- executor cannot reduce or rewrite the acceptance contract;
- accepted result marks job `succeeded`;
- rejected result marks job `rejected`;
- expired or terminal jobs cannot be updated.

### `POST /api/installer/session`

Builds an executor session from the same dry-run plan request.

Body:

```json
{
  "executor_contract_version": 1,
  "target_os": "linux",
  "target_arch": "x86_64",
  "package_channel": "stable",
  "plan": {
    "access_mode": "ip_self_signed_tls",
    "public_ip": "203.0.113.10",
    "bind_port": 2053,
    "firewall_allowlist": ["198.51.100.7/32"]
  }
}
```

Returns:

- `session_id`
- compatibility with the requested executor contract version
- explicit `target_os`, `target_arch`, `package_channel`, and selected artifact metadata when available
- embedded validated `plan`
- ordered `command_envelopes[]`
- each command envelope includes `operations[]`:
  typed executor operations such as `install_package_dependency`, `download_artifact`, `verify_sha256`, `install_binary`, `write_config`, `issue_lets_encrypt_certificate`, `generate_self_signed_certificate`, `write_service`, `start_service`, and `health_check`
- `loop_contract`

Executor rules:

- execute only the command envelope payloads;
- execute only typed `operations[]`; when an operation has `program + args`, run it as direct argv without shell interpolation;
- write-file/config/service operations are declarative operations, not shell snippets;
- Linux service operations use systemd unit metadata; Windows service operations use `sc.exe` with the selected Windows binary path;
- do not infer hidden shell commands;
- do not persist or submit passwords, private keys, tokens, or raw secret values;
- every command result must be submitted with the required attestation fields;
- rejected results must be treated as fail-closed.

### `POST /api/installer/session/result`

Validates executor results for an installer session.

Body:

```json
{
  "session_id": "panel-install-...",
  "access_mode": "ip_self_signed_tls",
  "expected_command_ids": [
    "1:preflight",
    "2:install_binary",
    "3:certificate",
    "4:configure_listener",
    "5:security_defaults",
    "6:service"
  ],
  "expected_operation_ids": [
    "2:install_binary/2:install_binary:download-artifact",
    "2:install_binary/2:install_binary:verify-sha256"
  ],
  "command_results": [
    {
      "command_id": "2:install_binary",
      "exit_code": 0,
      "attestation": {
        "operation_results": [
          {
            "operation_id": "2:install_binary/2:install_binary:download-artifact",
            "exit_code": 0,
            "completed": true,
            "verified": true
          },
          {
            "operation_id": "2:install_binary/2:install_binary:verify-sha256",
            "exit_code": 0,
            "completed": true,
            "verified": true
          }
        ]
      }
    }
  ]
}
```

Acceptance behavior:

- `expected_command_ids` must not be empty.
- every expected command id must have exactly one result;
- unexpected or duplicate command ids are rejected;
- every command must have `exit_code=0`;
- when `expected_operation_ids` is present, every expected operation id must have exactly one `operation_results[]` entry under its command attestation;
- operation result ids use `{command_id}/{operation_id}` format, where `operation_id` is the id exposed in the command envelope;
- duplicate, missing, unexpected, incomplete, failed, or explicitly unverified operation results are rejected fail-closed;
- preflight requires OS support, at least `512 MB` RAM, at least `1024 MB` free disk, and selected port availability;
- binary install requires explicit installed flag, binary path, artifact source URL, and successful SHA-256 verification attestation;
- certificate operation requires certificate path, private key path, `0600` key mode, and self-signed fingerprint when mode is `ip_self_signed_tls`;
- listener config requires explicit bind address and config written flag;
- firewall config requires firewall rules applied flag;
- security defaults require security defaults applied flag;
- service install requires service name, active service, and successful health check.

This endpoint validates and returns acceptance/rejection. Persistent job history,
token-authenticated session fetch, and real managed command execution are
implemented. The executor executes direct argv or declarative operations and
never asks the panel process to mutate the operating system.

## Clusters

Clusters model multi-hop routing topology:

- entry nodes
- relay nodes
- exit nodes
- edges between cluster-local node handles
- routing policy
- failover policy

The current implementation is a control-plane model and validation layer. It does not yet apply real multi-hop xray runtime config.

Cluster data is now included in:

- `GET /api/core/generated-config`
- `GET /api/core/xray-config`

Changing valid cluster topology changes the generated config revision.

Node-specific cluster debug targets are temporarily available through:

- `GET /api/nodes/{node_id}/cluster-targets`
- `GET /api/node-agent/cluster-targets`

These targets describe the node role, upstream peers, downstream peers, route edge ids, and cluster revision for the authenticated node.
They are development/debug payloads and must not be treated as the production cluster operating model.

Production cluster operation uses least-knowledge `node_route_assignments` embedded in node-specific generated config.
Relay nodes should receive only local route id, role, optional listen, previous peer, and next peer.
Route auth includes opaque `identity_ref` values so node-side VLESS hop client IDs can be derived without exposing the full cluster graph.
Route transport security uses `credential_ref` references for mTLS material; private keys are delivered only through the separate node-agent route credential endpoint.
For an outbound hop, `security.credential_ref` points to the local node material, while `auth.identity_ref` points to the next peer identity.

## Node Health Center

### `GET /api/nodes/health-center`

Returns a lightweight fleet-level node health view for the operator UI.

This endpoint does not call every node live. It uses the panel's persisted node heartbeat, metrics, sync, provisioning state, and bounded sync history so the response stays bounded and cheap on low-resource hosts.

The response includes:

- current generated config revision
- aggregate counts for enabled/disabled, health status, sync status, provisioning state
- stale heartbeat and stale metrics counts
- node-reported apply failure counts from the latest sync report per node
- node-reported runtime alert counts from the latest sync report per node
- latest apply status/target revision from bounded apply-result history
- latest successful and latest failed apply revisions
- rollback availability
- per-node `health_flags`
- per-node latest secret-free retry state and whether retry backoff is currently active
- per-node recommendations
- fleet-level recommendations

Important flags include:

- `offline`
- `degraded`
- `unknown_status`
- `stale_heartbeat`
- `stale_metrics`
- `config_drift`
- `apply_pending`
- `provisioning_running`
- `provisioning_stale`
- `provisioning_failed`
- `reported_apply_failed`
- `retry_backoff_active`
- `runtime_alerts_active`
- `rollback_available`
- `memory_high`
- `disk_high`

The endpoint never returns node auth tokens, route private keys, SSH credentials, or local secret material.

Node health center also feeds the shared system alert pipeline when system overview, active alerts, or health center are requested.
Current system/node alert kinds:

- `disk_usage`
- `memory_usage`
- `panel_memory_budget`
- `node_offline`
- `node_stale_heartbeat`
- `node_config_drift`
- `node_provisioning_stale`
- `node_provisioning_failed`
- `node_reported_apply_failed`
- `node_runtime_alert`

Node alerts are aggregate alerts, not per-node secret-bearing payloads.
For node alerts, `observed_value` is the affected node count and `threshold_value` is `1`.
For `panel_memory_budget`, `observed_value` is panel process RSS in bytes and `threshold_value` is the configured budget threshold in bytes.
For `node_runtime_alert`, severity is `critical` when at least one active node-reported runtime alert is critical; otherwise it is `warning`.

### `GET /api/nodes/{node_id}/apply-status`

Returns a focused apply/runtime status view for one node.

This endpoint is intended for the UI screen that answers:

- what generated revision the panel expects
- what revision the panel believes the node applied
- what revision the node local runtime reports
- whether the panel can read node local state
- whether Xray runtime is running/stopped/failed
- whether the last Xray core update is running/succeeded/failed and which phase it reached
- whether the last Xray render failed closed or has issues
- whether node-agent has reported a real external Xray validation result
- whether node-agent has reported active local runtime alerts
- whether required node route credentials are active, missing, or explicitly revoked
- latest node-reported apply stages/issues from sync history
- normalized apply lifecycle state:
  `unknown`, `pending`, `downloaded`, `rendered`, `validated`, `applied`, `failed`, `rolled_back`
- normalized apply timeline phase status
- whether rollback material is available and whether restart is considered safe
- recent sync history
- blocking issues and operator recommendations

It may call the node local `/state` endpoint. If local state is unavailable, the response still includes panel-side sync and heartbeat information.
It also inspects the panel route material store. If a required `credential_ref` is explicitly revoked, the response includes a failed `route_credentials` stage and a blocking issue so the node remains fail-closed until an admin rotates/reissues the material.
If the latest sync report includes `apply_stages` or `apply_issues`, the response includes a `node_reported_apply` stage and converts failed reported stages/error issues into blocking issues.
If the latest sync report includes `apply_lifecycle_state`, `last_good_revision`, or `rollback_available`, the response exposes them through `lifecycle`. If the state is omitted, the panel infers it from sync status and reported stage names where possible.
The response includes `xray_external_validation` as a standard stage when Xray is required. Missing, skipped, or failed external validation becomes a blocking issue for restart safety. A passed report can come from node local state, recent apply-result history, or recent sync history.

The response includes `timeline[]`, a fixed normalized sequence for UI display:

- `fetch_runtime_config`
- `fetch_route_credentials`
- `render_xray_config`
- `validate_xray_config`
- `write_runtime_state`
- `restart_xray`
- `report_sync`
- `report_apply_result`

Each timeline item contains `phase`, `status`, `detail`, `source`, and `observed_at_unix`. Valid timeline statuses are `pending`, `active`, `ok`, `warning`, `failed`, `skipped`, and `unknown`.

The response must not expose node auth tokens, route private keys, SSH credentials, or route credential material.

### Node Local API Token

`POST /api/nodes` and `PUT /api/nodes/{node_id}` accept optional `local_api_token`.

This token is used by the panel when calling the node's protected local API endpoints:

- `GET /state`
- `POST /runtime/*`
- `POST /runtime-components/{component}/{action}`
- `POST /runtime/rollback`
- `POST /xray/update`

Response safety:

- node responses expose only `local_api_token_configured`
- plaintext `local_api_token` is write-only and is not returned by API responses
- persisted `local_api_token` is encrypted at rest in `nodes.json`
- the node-secret master key is loaded from `HYDRA_NODE_SECRETS_MASTER_KEY_B64` or from `HYDRA_NODE_SECRETS_KEY_PATH` / `data/node-secrets.key`
- configuring or clearing the token emits a `node_local_api_token_updated` audit event without including the token value

Update behavior:

- omit `local_api_token` to leave the existing token unchanged
- send an empty string to clear the token

Privileged local node actions:

- `POST /api/nodes/{node_id}/local/runtime/{action}` emits `node_local_runtime_action_requested`
- `POST /api/nodes/{node_id}/local/runtime-components/{component}/{action}` emits `node_local_runtime_action_requested`
- `POST /api/nodes/{node_id}/local/xray/update` emits `node_local_xray_update_requested`
- audit details include node id/action only, not local API tokens or node secret material

Runtime component action contract:

- supported components:
  `xray`, `hysteria2_sidecar`, `wireguard_node_native`
- supported actions:
  `install`, `update`, `validate`, `start`, `stop`, `restart`, `status`, `logs`
- unsupported component/action values fail closed before proxying to the node local API
- this is a contract placeholder for future sidecar/node-native lifecycle implementation; the panel proxies the action to the node-agent, and the node-agent owns actual execution
- `/api/ui/contracts` exposes `runtime_component` and `runtime_component_action` enum values so UI code does not need to hardcode lifecycle options
- `GET /api/protocol-capabilities` also exposes `runtime_components[].supported_actions` for per-component lifecycle UI rendering

### `GET /api/nodes/{node_id}/provisioning/status`

Returns a compact provisioning status view for one node.

The response includes:

- latest provisioning task, if any
- latest task status
- `stale_active_task` when the latest pending/running task stopped reporting progress
- parent task id and reason for retry/reprovision lineage when present
- current/last step
- failed step, if any
- `can_retry`
- `can_reprovision`
- latest preflight report
- task `completion` summary when a task exists:
  executor-step readiness, required handoff proofs, proof sources, bootstrap state, and explicit blockers
- next remediation actions
- operator recommendations
- latest task `executor_readiness` snapshot when a task exists:
  sanitized transport readiness checks captured at task creation time
- `recovery` decision:
  `ready`, `retry`, `reprovision`, or `repair_first`

This endpoint is intended for UI decision-making and should be preferred over manually deriving state from the raw task list.
Sensitive executor inputs such as SSH password and private key are request-scoped and are not persisted or returned.
`executor_readiness` stores only sanitized fields such as transport, target-host presence, port, username presence, auth-method flags, and node-token readiness.

`recovery` distinguishes between:

- `ready`: the latest task is usable
- `retry`: the last failure is recoverable without mandatory repair
- `reprovision`: a fresh provisioning run is preferred
- `repair_first`: a blocking prerequisite must be fixed before retrying

Pending/running tasks are considered active blockers while they are fresh. A task that has not updated for more than 30 minutes is surfaced as `stale_active_task=true`; it becomes retryable/reprovisionable instead of staying an eternal opaque `running` state. Starting another task while a fresh active task exists is rejected.

Examples of mandatory repair actions:

- missing node token -> rotate/issue node auth token
- missing sudo -> repair sudo/root access
- occupied remote ports -> free required ports or change node ports

### `GET /api/nodes/{node_id}/provisioning/events`

Returns bounded provisioning events for one node.

Query:

- `task_id`
- `kind`
- `limit`

Use this endpoint for the UI installation log and retry/reprovision timeline.
Events are persisted separately from task state and do not include request-scoped executor secrets.

### `GET /api/clusters`

Returns all configured clusters.

### `POST /api/clusters`

Creates a cluster.

Body shape:

```json
{
  "name": "primary multi-hop",
  "description": "optional",
  "status": "draft",
  "nodes": [
    {
      "id": "entry-1",
      "node_id": "panel-node-id",
      "role": "entry",
      "position_x": 0,
      "position_y": 0
    }
  ],
  "edges": [
    {
      "from_cluster_node_id": "entry-1",
      "to_cluster_node_id": "exit-1",
      "priority": 100,
      "enabled": true
    }
  ],
  "routing_policy": {
    "name": "default",
    "description": "optional",
    "prefer_domestic_entry": true,
    "controlled_egress": true
  },
  "failover_policy": {
    "enabled": false,
    "max_failover_hops": 0
  }
}
```

### `PUT /api/clusters/{cluster_id}`

Updates a cluster and produces a new revision when topology/policy changes.

### `DELETE /api/clusters/{cluster_id}`

Deletes a cluster.

### `GET /api/clusters/{cluster_id}/validation`

Returns graph validation errors and warnings.

Validation currently checks:

- at least one entry node
- at least one exit node
- referenced panel nodes exist
- edges reference known cluster nodes
- edges do not point to themselves

### `GET /api/clusters/{cluster_id}/preview`

Returns bounded path preview from entry nodes to exit nodes.

The preview is capped to avoid unbounded graph traversal in memory.

## System / Monitoring

### `GET /api/system/overview`

Returns:

- memory budget
- current RAM usage
- current disk usage
- current core status
- active alerts

### `GET /api/system/resource-budget`

Returns a runtime resource budget report for the low-memory deployment target.

The top-level report includes:

- `memory_budget_mb`: configured panel memory budget, default `512`;
- `process_memory_used_bytes`: current panel process RSS;
- `process_memory_budget_bytes`: configured memory budget converted to bytes;
- `process_memory_percent_of_budget`: current panel process RSS as a percentage of the configured budget;
- `process_cpu_usage_percent`: current panel process CPU snapshot;
- `target_vcpu`: deployment target CPU count, currently `1`;
- `target_disk_gb`: deployment target disk size, currently `10`;
- `status`: worst status across the process RSS item and tracked runtime buffers;
- `items`: per-buffer/process budget items;
- `recommendations`: human-readable remediation hints.

The report compares tracked runtime collections against configured limits:

- panel process resident memory (`process.rss_mb`);
- security audit events, login IP counters, admin sessions;
- operational logs, alert history, core apply history;
- Telegram delivery events;
- user activity events;
- subscription devices, sessions, usage points, enforcement actions;
- node sync/apply/bootstrap history;
- node provisioning tasks/events/submissions;
- panel installer jobs.

Compaction behavior:

- panel installer jobs prefer active/recent jobs;
- node provisioning tasks prefer active/recent tasks;
- subscription usage telemetry must be inserted through bounded buffers when ingestion is enabled.

Status values:

- `ok`
- `warning`
- `over_limit`

Target deployment envelope:

- `1 vCPU`
- `512 MB RAM`
- `10 GB disk`

### `GET /api/system/thresholds`

Returns current warning/critical thresholds.

### `PUT /api/system/thresholds`

Updates warning/critical thresholds for disk and memory.

### `GET /api/system/secret-readiness`

Returns non-secret readiness diagnostics for persisted secret master keys.

The response includes one item for:

- `admin`
- `node`
- `telegram`
- `route_materials`

Each item reports:

- `env_var_name`
- `key_path`
- `source`: `environment`, `key_file`, or `key_file_pending`
- `status`: `ready`, `pending`, or `invalid`
- `detail`

This endpoint never returns secret values, decrypted material, private keys, node tokens, Telegram bot tokens, or 2FA secrets.

Operational behavior:

- `ready` means the configured environment variable or key file contains valid base64-encoded 32-byte key material.
- `pending` means no environment variable is set and the key file does not exist yet; the panel will generate the key file on first use.
- `invalid` means the configured environment variable or existing key file cannot be used and encrypted material may be unreadable until fixed.

### `GET /api/system/alerts`

Returns currently active alerts.

### `GET /api/system/alerts/history`

Query:

- `kind`
- `severity`
- `status`
- `limit`

Returns alert event history.

### `GET /api/system/logs`

Query:

- `level`
- `limit`

Returns operational log lines.

## Core / Runtime

### `GET /api/core/config`

Returns persisted core config text plus save/validation state.

### `PUT /api/core/config`

Body:

```json
{
  "config": "{ ...json... }"
}
```

Validates that the payload is valid JSON and persists it.

### `GET /api/core/generated-config`

Returns the generated control-plane config preview.

Includes:

- `revision`
- generated users
- inbounds
- hosts
- nodes

### `GET /api/core/xray-config`

Returns an xray-oriented document generated from the current control-plane state.

This is closer to future runtime apply output than the plain preview.

The response includes `raw_config`, a deterministic raw-like Xray JSON object intended as the next input for real Xray validation.
It also includes `raw_config_validation`, an internal validation report for the raw-like object.
It currently contains:

- `log`
- `inbounds`
- `outbounds`
- `routing`
- `policy`

Inbound documents now include renderer-oriented fields:

- `stream_settings`:
  network, security, path, host, service name, and fail-closed `allow_insecure=false`
- `security_settings`:
  security mode, server name, certificate reference, and future Reality fields

Current renderer-oriented coverage includes:

- `VLESS + TLS + WebSocket` stream metadata
- `Trojan + TLS` security metadata through the same TLS model
- base TCP/TLS metadata for existing production-ready inbound modes

Protocol-specific Xray client credentials are derived during runtime rendering:

- VLESS and VMess render deterministic UUID-like `id` values.
- Trojan and Shadowsocks render protocol-specific derived `password` values.
- Raw Xray config must not include the user's subscription token directly.
- Internal raw validation is protocol-aware and fail-closed:
  VLESS/VMess client ids must remain UUID-like, VMess `alterId` must be `0`, Trojan must use TLS, TLS mode must include server/certificate/key fields, and Shadowsocks must use the expected `2022-blake3-aes-128-gcm` method.
- Node runtime render summaries normalize raw validation issues to the affected inbound tag/protocol when possible, so node apply status can show which inbound failed without requiring UI-side raw JSON parsing.
- VMess remains legacy compatibility and must not become a recommended default.
- Hysteria2 and WireGuard remain planned until runtime ownership, key lifecycle, and apply/update behavior are explicit.

This endpoint still returns an xray-oriented contract document plus `raw_config`.
The external validation endpoint can pass `raw_config` through `xray run -test -config` when an Xray binary is configured.

### `GET /api/core/xray-config/validation`

Returns only the internal raw Xray config validation report.

Response fields:

- `valid`
- `checked_at_unix`
- `issue_count`
- `issues`

Issue fields:

- `path`
- `severity`
- `reason`

This is an internal structural validation stage. It does not execute the Xray binary.

### `GET /api/core/xray-config/external-validation`

Runs the external Xray validation stage for the generated `raw_config`.

Behavior:

- requires auth
- first generates the current Xray document
- skips execution if internal validation already failed
- skips execution if `HYDRA_XRAY_BINARY_PATH` is not configured
- writes a temporary `xray.json` under `HYDRA_XRAY_VALIDATION_TEMP_DIR`
- runs `xray run -test -config <temp-file>`
- deletes the temporary file after execution
- does not restart Xray and does not replace the binary

Environment:

- `HYDRA_XRAY_BINARY_PATH`: optional path to the Xray binary
- `HYDRA_XRAY_VALIDATION_TEMP_DIR`: optional temp directory, default `data/xray-validation`

Response fields:

- `status`: `passed`, `failed`, or `skipped`
- `checked_at_unix`
- `binary_path`
- `internal_validation_valid`
- `exit_code`
- `stdout`
- `stderr`
- `detail`
- `config_retained`: currently always `false`

`stdout` and `stderr` are bounded in the backend response. The temporary config is not retained because it may contain client/runtime material.

### `GET /api/core/state`

Returns:

- core status
- last core action, including `result` and operator-readable `detail`
- current applied revision
- last Xray update lifecycle report, if any

### `POST /api/core/actions`

Body:

```json
{
  "action": "start"
}
```

Supported:

- `start`
- `stop`
- `restart`

The action endpoint keeps the response compatible, but `GET /api/core/state` exposes the resulting
`last_action.result` and `last_action.detail`.

Restart remains explicit after Xray update. If the latest Xray lifecycle report is `swapped`,
restart records that it followed a validated binary swap. If the latest report is `planned`,
`failed`, or `blocked`, restart still records the condition so the UI/API can show why the runtime
was restarted without a confirmed safe swap.

### `POST /api/core/restart`

Shortcut for restart.

Audit:

- emits `core_restart_requested`
- audit detail contains only the requested action and restart-gate diagnostic, not generated config or command output

### `POST /api/core/xray/update`

Plans the panel-side Xray core update lifecycle against the official `XTLS/Xray-core` GitHub release API.

Body:

```json
{
  "target_version": "optional tag such as v25.1.1",
  "allow_prerelease": false,
  "confirm_binary_swap": false
}
```

Current behavior:

- requires auth
- requires `HYDRA_XRAY_BINARY_PATH`
- resolves the official release metadata
- selects the best release asset for current OS/architecture
- downloads the selected ZIP asset with a bounded streaming download
- records archive SHA256
- extracts only the candidate `xray` / `xray.exe` binary through safe archive paths
- runs candidate `xray version`
- runs candidate `xray run -test -config` against the generated raw Xray config
- records staged lifecycle output in core runtime state
- if `confirm_binary_swap=false`, does not replace or restart the active Xray binary
- if `confirm_binary_swap=true`, backs up the active binary, replaces it with the validated candidate, runs post-swap version/config tests, and attempts rollback if post-swap validation fails
- never restarts Xray automatically
- emits `core_xray_update_requested` for audit filtering

Stages:

- `preflight`
- `release_resolved`
- `asset_selected`
- `download_prepared`
- `downloaded`
- `extracted`
- `candidate_version`
- `candidate_config_test`
- `binary_swap`
- `config_test`
- `restart_gate`

Status:

- `planned`: candidate binary was downloaded, extracted, version-checked, and config-tested without active runtime mutation
- `swapped`: active binary was replaced after candidate validation and post-swap validation passed
- `failed`: preflight/release/asset/workdir/download/extract/candidate check failed
- `blocked`: policy-level block, such as prerelease update without `allow_prerelease`

Environment:

- `HYDRA_XRAY_BINARY_PATH`: target Xray binary path
- `HYDRA_XRAY_RELEASE_API_URL`: release API URL, default `https://api.github.com/repos/XTLS/Xray-core/releases/latest`
- `HYDRA_XRAY_UPDATE_WORK_DIR`: update work directory, default `data/xray-updates`
- `HYDRA_XRAY_UPDATE_MAX_DOWNLOAD_BYTES`: max ZIP download size, default `134217728`

Security rule:

- Xray restart must remain gated until the candidate binary has been downloaded, extracted, checksum-verified, tested with the generated config, and swapped with rollback support.
- ZIP extraction must only accept safe enclosed paths and must extract only the candidate binary, not arbitrary archive contents.
- Binary swap keeps a backup under the update work directory and attempts rollback if post-swap validation fails.
- Restart must be a separate explicit action after `status: "swapped"` and must leave a clear action detail in runtime state/audit.

### `POST /api/core/apply-generated`

Body:

```json
{
  "revision": "optional"
}
```

Applies the current generated config revision to the core runtime state.

If `revision` is supplied and does not match the current generated revision, the apply is skipped and recorded as such.

Audit:

- emits `core_apply_requested`
- audit detail includes only the generated config revision, not rendered raw config or secret material

Apply now records explicit stages:

- `generated`
- `internal_validated`
- `external_validated`
- `runtime_state_updated`

External Xray validation is executed during apply when `HYDRA_XRAY_BINARY_PATH` is configured. A failed external validation blocks runtime state update and records `result: "failed"`. A skipped external validation is allowed for dev/incomplete environments, but the skipped stage is visible in the returned record and apply history.

### `GET /api/core/apply-history`

Query:

- `result`
- `limit`

Returns core apply history, including validation/apply stages for new records.

## Users

### `GET /api/users`

Query:

- `status`
- `search`
- `limit`

Returns filtered users.

### `POST /api/users`

Creates a new user.

Supports:

- template linkage
- data limit
- expire time
- note
- proxy profile linkage
- inbound exclusion linkage

### `GET /api/users/{username}`

Returns one user.

### `PUT /api/users/{username}`

Updates user fields.

### `DELETE /api/users/{username}`

Deletes a user.

### `GET /api/users/activity`

Query:

- `kind`
- `limit`

Returns global user activity history.

### `GET /api/users/{username}/activity`

Query:

- `kind`
- `limit`

Returns activity for a single user.

### `POST /api/users/{username}/usage/reset`

Resets `used_traffic_bytes` to zero.

### `POST /api/users/{username}/usage/report`

Body:

```json
{
  "bytes_delta": 1024
}
```

Adds usage to `used_traffic_bytes`.

### `GET /api/users/{username}/subscription`

Returns subscription view and subscription path.

### `POST /api/users/{username}/subscription/revoke`

Rotates the user subscription token and marks the old one revoked.

### `GET /api/users/{username}/subscription/render`

Query:

- `format=json|plain_text|base64|diagnostic_json`

Renders subscription output for admin-side inspection:

- `json`: safe structured client bundle with endpoint metadata, client credentials, and bounded render issues
- `plain_text`: newline-delimited interoperable client URIs
- `base64`: standard Base64 encoding of the `plain_text` URI list
- `diagnostic_json`: authenticated operator-only generated config, including internal profile settings needed for runtime diagnostics

`diagnostic_json` must never be proxied to an unauthenticated client.

### `GET /api/users/{username}/config-preview`

Returns resolved preview:

- proxy profiles
- available inbounds
- excluded inbound tags
- hosts

### `GET /api/users/{username}/generated-config`

Returns generated user config after resolving links.

## User Templates

### `GET /api/user-templates`

Returns templates.

### `POST /api/user-templates`

Creates a template.

### `PUT /api/user-templates/{template_id}`

Updates a template.

### `DELETE /api/user-templates/{template_id}`

Deletes a template.

## Subscription Catalog

Current status:

- backend domain/API contract exists for subscription plans and subscription-scoped clients
- catalog persistence uses `HYDRA_SUBSCRIPTION_CATALOG_PATH`, default `data/subscription-catalog.json`
- clients support status, max simultaneous devices, max simultaneous source IPs, traffic limit, expiration timestamp, operator note, node/cluster/protocol access policy, usage reset, revoke, and delete
- usage detail supports fixed windows and custom absolute ranges through a bounded usage point contract
- catalog clients can now be rendered through admin API and public `/sub/{subscription_token}`
- rendered catalog-client output includes explicit access policy metadata
- inbound/protocol endpoints and host endpoints can be bound to `node_id` and/or `cluster_id`, and catalog-client rendering filters both through that policy
- catalog-client access policy also supports protocol filtering through `allow_all_protocols` and `protocols`; protocol filtering applies to inbounds and proxy profiles, while hosts remain filtered by node/cluster binding
- active catalog clients now participate in generated core/Xray runtime configuration; their server-side client binding is included only in inbounds allowed by their access policy
- catalog runtime principals use stable `catalog/{client_id}` identities rather than editable display names, so credential identity does not collide or rotate solely because a client label changes
- node-agent generated config is node-scoped before runtime document creation: unrelated nodes, endpoints, users, and proxy profiles are pruned from the authenticated node's projection
- disabled, expired, or revoked catalog clients are excluded from generated Xray runtime configuration
- a first device/HWID admission registry exists for catalog clients, with active/revoked state and max-device gate
- raw device fingerprints are request-only; persistence stores a keyed HMAC using `HYDRA_SUBSCRIPTION_DEVICES_KEY_PATH`, and API responses do not expose the HMAC
- self-service device enrollment uses short-lived one-time grants; raw enrollment tokens and per-device subscription credentials are returned only in their respective success responses and are stored only as domain-separated keyed HMACs
- each enrolled device receives its own `/sub/device/{device_credential}` bearer path; the shared catalog-client token cannot render an enrolled device, and revoking the device invalidates its path
- node-agent session observations now produce bounded `allow` / `block` policy verdicts for assigned `catalog/{client_id}` runtime principals
- each `block` verdict includes a typed `terminate_session` enforcement action, and node-agent can acknowledge the execution result as `applied` or `failed`
- exact enforcement uses an optional request-time `runtime_session_ref`; panel stores only a keyed HMAC of that opaque node-local reference and never returns it through the admin session API
- session reporting declares a bounded runtime capability set; panel issues exact termination only for nodes declaring opaque-reference, exact-termination, and post-action-verification support
- a node declaring only principal-wide termination still receives a `block` verdict, but panel will not issue a destructive action that can terminate unrelated sessions for the same client
- when a client has a device limit, an active registered fingerprint is required for a reported session; optional `max_simultaneous_ips` limits simultaneously observed source IPs
- raw device fingerprints and raw source IPs are request-only; session API views expose neither values nor internal HMACs
- session observation and enforcement-action state are bounded, in-memory, stale-pruned, and subject to node-local action deadlines; the Rust node-agent carries a protected lease-bound adapter handshake plus fail-closed executable-driver orchestration with targeted terminate, separate absence verification, and a refreshed runtime-table check; WireGuard supports exact peer-per-device enforcement when each device owns a unique peer key, while Xray/Hysteria2 remain non-exact and Telegram bot actions remain pending work

### `GET /api/subscription-plans`

Query:

- `search`
- `limit`

Returns subscription plans.

### `POST /api/subscription-plans`

Creates a subscription plan.

### `GET /api/subscription-plans/{plan_id}`

Returns one subscription plan.

### `PUT /api/subscription-plans/{plan_id}`

Updates one subscription plan.

### `DELETE /api/subscription-plans/{plan_id}`

Deletes an empty subscription plan. Plans with clients are rejected.

### `GET /api/subscription-plans/{plan_id}/clients`

Query:

- `search`
- `limit`

Returns clients inside one subscription plan.

### `POST /api/subscription-plans/{plan_id}/clients`

Creates a client inside one subscription plan.

### `GET /api/subscription-clients/{client_id}`

Returns one subscription-scoped client.

### `PUT /api/subscription-clients/{client_id}`

Updates one subscription-scoped client.

### `DELETE /api/subscription-clients/{client_id}`

Deletes one subscription-scoped client.

### `GET /api/subscription-clients/{client_id}/subscription/render`

Query:

- `format=json|plain_text|base64|diagnostic_json`
- optional `device_id`; required when `max_simultaneous_devices` is configured

Renders a subscription-scoped client for admin-side inspection. Output includes:

- `subject_type=subscription_client`
- `plan_id`
- `client_id`
- generated subscription config
- `access_policy` with `allow_all_nodes`, `node_ids`, `cluster_ids`, `allow_all_protocols`, and `protocols`

Production render formats:

- `plain_text` emits VLESS, VMess, Trojan, Shadowsocks, and Hysteria2 client URIs
- `base64` wraps that exact URI list in standard Base64
- `json` emits schema-versioned endpoint objects and is required for WireGuard, because WireGuard has no interoperable subscription URI
- WireGuard JSON includes only the client public key, server public key, assigned addresses, and endpoint; the client private key remains client-owned and is never accepted, generated, stored, or rendered by the panel
- malformed or incomplete endpoint/profile combinations are omitted fail-closed and described through bounded `issues`
- raw `settings_json`, TLS key paths, node interface private keys, and the subscription token are never included in production render bodies

Without device limiting, the generated runtime username is `catalog/{client_id}`. Limited subscriptions use `catalog/{client_id}/device/{device_id}` and reject a missing, foreign, or revoked `device_id` instead of rendering a shared parent credential. The editable catalog client `name` remains operator metadata and is not used as an Xray principal key.

### `GET /api/subscription-clients/{client_id}/access-preview`

Returns the subscription access diagnostic view for UI/Telegram/operator checks:

- `renderable`
- access policy
- allowed/denied inbounds with reasons
- allowed/denied hosts with reasons
- warnings for non-renderable client state, empty allowed inbounds, empty allowed hosts, or empty explicit policy

### `PUT /api/subscription-clients/{client_id}/node-access`

Updates the explicit node/cluster access policy.

### `GET /api/subscription-clients/{client_id}/devices`

Query:

- `status=active|revoked`
- `limit`

Returns redacted devices registered to the client. The response exposes device id, optional label/platform, lifecycle timestamps, status, and optional WireGuard public-key/AllowedIPs assignment metadata. WireGuard public keys are not secret, but private keys are never accepted or returned. The raw generic fingerprint, keyed WireGuard fingerprint, and stored HMACs are never exposed.

### `POST /api/subscription-clients/{client_id}/device-enrollments`

Admin-authenticated endpoint that creates a one-time enrollment grant.

Body:

- optional `expires_in_seconds`, from `60` through `1800`; default `600`

The response contains the redacted grant and `enrollment_token`. The raw token is returned exactly once, uses 256 bits of randomness, must be transferred to the intended client over a confidential channel, and is never written to the catalog or audit log. At most eight active grants may exist for one client. Creation fails when the client is unavailable, revoked, expired, already at its active device limit, or when the bounded grant store is full.

### `GET /api/subscription-clients/{client_id}/device-enrollments`

Admin-authenticated endpoint that lists redacted grants.

Query:

- optional `status=active|consumed|revoked|expired`
- optional `limit`

Responses never expose token HMACs. Expiry is evaluated from `expires_at_unix`, even if the persisted terminal status has not yet been compacted.

### `POST /api/subscription-clients/{client_id}/device-enrollments/{grant_id}/revoke`

Admin-authenticated endpoint that revokes an active unused grant. Consumed, expired, and already revoked grants cannot be changed through this endpoint.

### `POST /api/device-enrollment/exchange`

Public, body-limited endpoint used once by a client device.

Body:

- `enrollment_token`: the exact one-time token issued by the administrator
- `device`: the same validated device registration object described below

On success the panel atomically registers a new device, consumes the grant, and returns:

- redacted `device`
- one-time `device_credential`
- `/sub/device/{device_credential}` in `subscription_path`
- supported production formats: `base64`, `plain_text`, and `json`

The exchange is serialized against concurrent attempts. Exactly one request can consume a grant; replay, malformed, unknown, expired, revoked, or already consumed tokens fail with the same generic unauthorized result. An existing fingerprint cannot be claimed through a new enrollment token. The raw enrollment token, raw fingerprint, and raw device credential are excluded from persistence and audit. The response uses `Cache-Control: private, no-store`, `Pragma: no-cache`, and `X-Content-Type-Options: nosniff`.

### `GET /sub/device/{device_credential}`

Public device-scoped subscription delivery endpoint.

Query:

- optional `format=base64|plain_text|json`; default `base64`

`diagnostic_json` is forbidden. The credential is a 256-bit bearer secret for one device and is looked up through a domain-separated keyed HMAC. Only an active device belonging to an active, non-expired client can render. Revoking either the device or client immediately invalidates this path. The catalog-client parent token cannot be used as a fallback for an enrolled device.

HTTP request tracing redacts every `/sub/...` path to `/sub/{redacted}` and does not record query strings, so parent or device bearer credentials cannot enter normal request spans.

Subscription catalog audit:

- plan lifecycle emits `subscription_plan_created`, `subscription_plan_updated`, and `subscription_plan_deleted`
- client lifecycle emits `subscription_client_created`, `subscription_client_updated`, `subscription_client_revoked`, and `subscription_client_deleted`
- client access-policy changes emit `subscription_client_access_updated`
- client usage reset emits `subscription_client_usage_reset`
- device admission/revoke emits `subscription_device_registered` and `subscription_device_revoked`
- device enrollment emits `subscription_device_enrollment_created`, `subscription_device_enrollment_revoked`, and `subscription_device_enrollment_consumed`
- session observation/enforcement emits `subscription_session_reported` and `subscription_session_enforcement_reported`
- audit details are intentionally minimal: plan/client/device/action ids only, never subscription tokens, raw fingerprints, source IPs, HMACs, rendered configs, or full access-policy payloads

### `POST /api/subscription-clients/{client_id}/devices/register`

Authenticated device-admission foundation endpoint.

Body:

- `fingerprint`: required request-time HWID/device fingerprint, 8-512 characters
- optional `label`
- optional `platform`
- optional `wireguard_public_key`: canonical 44-character base64 WireGuard public key
- optional `wireguard_allowed_ips`: 1-16 unique `/32` or `/128` device host routes

`wireguard_public_key` and `wireguard_allowed_ips` must be provided together. The device generates its WireGuard keypair locally and sends only the public key. The panel must never generate, receive, persist, log, or render the device private key.

Behavior:

- calculates a keyed HMAC of the fingerprint and never persists the raw submitted value
- admits an already known active device and refreshes `last_seen_at_unix`
- refuses a revoked device
- refuses a new device once `max_simultaneous_devices` active registrations are already present
- refuses new records once bounded registry capacity is reached
- binds one WireGuard public key and its host routes to exactly one device record
- rejects public-key or AllowedIPs reuse across devices, including revoked records
- treats the same key/routes on the same active device as idempotent, but rejects silent key or route replacement; rotation requires revoking the old device and registering a new key
- stores only a keyed HMAC of the canonical `wireguard-sha256:` device fingerprint used by node-side session reporting

For node runtime generation, each active WireGuard-enabled device becomes a separate peer profile. Limited subscriptions bind it to `catalog/{client_id}/device/{device_id}`; legacy unlimited subscriptions retain the parent principal for compatibility. Revoked devices and incomplete peer assignments are omitted fail-closed. Public subscription rendering does not expose node-side WireGuard interface private material.

Registration establishes device admission. Reported online sessions additionally apply the device and source-IP policy verdict described below.

Device limiting is protocol-agnostic. One parent subscription key owns a bounded set of active logical devices. Panel derives a separate runtime principal and credential for every active device:

- VLESS and VMess receive a distinct UUID per device and profile
- Trojan, Shadowsocks, and Hysteria2 receive a distinct password per device and profile
- WireGuard receives the device-owned peer public key

Derived credentials are domain-separated by client, device, profile, protocol, and the current subscription token. Rotating or revoking the parent token changes all derived non-WireGuard credentials. Revoking one device removes its runtime principal from generated Panel/Node configuration for every protocol.

Native VPN protocols do not transmit a trustworthy hardware identifier. This feature therefore limits logical device credentials, not uncopyable physical hardware. If one credential is copied to another machine, simultaneous session/source-IP policy or a future attested Hydra client is required to detect that sharing.

### `GET /api/subscription-clients/{client_id}/sessions`

Query:

- `verdict=allow|block`
- `limit`

Returns the bounded current observation view for catalog-client sessions. Responses include the node, stable runtime principal, linked device id when admitted, whether a source IP was present, verdict, and reason. Responses never include raw device fingerprints, raw source IPs, or stored HMACs.

Blocked session views include the latest `terminate_session` action and its `pending`, `applied`, or `failed` status when an action is tied to a known catalog session. Session observations expire from the in-memory view when stale. This is an enforcement/diagnostic contract, not durable traffic accounting.

### `POST /api/node-agent/subscription-sessions/report`

Authenticated node-agent endpoint for reporting the node's current observed catalog sessions.

Body:

- `observation_source=node_managed_runtime_table`
- `runtime_capabilities[]`, supported declarations:
  `opaque_session_reference`, `exact_session_termination`, `post_action_absence_verification`, `principal_wide_termination_only`
- `observations[].session_id`
- `observations[].runtime_username`, expected as `catalog/{client_id}` for legacy/unlimited clients or `catalog/{client_id}/device/{device_id}` for device-scoped credentials
- optional `observations[].runtime_session_ref`, an opaque node-local exact-session handle used only for enforcement proof binding
- optional `observations[].device_fingerprint`, used only for keyed admission lookup
- optional `observations[].source_ip`, used only for keyed concurrent-IP evaluation
- optional `observations[].connected_at_unix`

The panel evaluates only principals present in that node's projected runtime config. Unknown or unassigned principals receive a generic `block` verdict, preventing relay nodes from probing catalog membership. The response returns a verdict and operator-readable reason per reported session. A `block` verdict includes a bounded `terminate_session` action only when the report declares the complete exact capability set and the observation contains an opaque exact runtime handle.

The opaque runtime session reference is not exposed in action/admin responses. A node already owns that local handle and must keep it locally while reporting back proof. If the node cannot provide an exact reference, it may report failure, but must not claim successful termination.

Panel returns `enforcement_unavailable_reason` rather than an action when the reported node capabilities cannot safely perform an exact termination, or when a blocked observation omitted its opaque runtime session reference. `principal_wide_termination_only` is intentionally not a valid substitute for exact enforcement because it could disconnect valid sessions sharing the same client principal.

Standalone mode uses the same registry internally. Generated local Xray config exposes `StatsService` only at `127.0.0.1:10085`; `panel-app` polls non-resetting user counters, reports recent `catalog/{client}/device/{device}` activity under node id `standalone-panel`, and declares no exact capabilities. Polling and activity-window defaults are configurable through `HYDRA_XRAY_STATS_POLL_INTERVAL_SECONDS` and `HYDRA_XRAY_ACTIVITY_WINDOW_SECONDS`.

### `POST /api/node-agent/subscription-sessions/enforcement-result`

Authenticated node-agent endpoint for acknowledging execution of a session enforcement action.

Body:

- `action_id`: action id returned by the session report response
- `session_id`: local session id bound to that action
- `status=applied|failed`; `pending` is not accepted as a completion report
- for `status=applied`, the exact original `runtime_session_ref`
- for `status=applied`, `adapter=node_managed_exact_session`
- for `status=applied`, `session_absent_after_action=true` and `verified_at_unix`
- optional `detail`, bounded diagnostic text without secrets

The action is accepted only from the node to which it was issued and only for the bound session id. An `applied` acknowledgement is rejected unless the HMAC of the supplied runtime reference matches the observation that caused the action and the node provides post-action absence evidence. Results update the current session enforcement view and append a security audit event.

This endpoint confirms node execution; the panel itself does not terminate a remote runtime connection. The existing Rust Node now validates and forwards proof from its dedicated local session-adapter boundary, but Xray presence alone is not evidence of exact per-session termination capability: a real local runtime adapter must resolve the node-local handle and verify that the targeted session disappeared.

### `POST /api/subscription-clients/{client_id}/devices/{device_id}/revoke`

Revokes one registered device. Revoking the parent subscription client also revokes all of its active devices. Revocation changes generated credential material, so VLESS, VMess, Trojan, Shadowsocks, Hysteria2, and WireGuard omit that device on the next safe runtime apply. If a WireGuard peer is currently observed, its exact driver can remove only that peer and verify absence. Xray Stats and Hysteria2 Traffic Stats observations are deliberately observation-only: the panel returns a blocked verdict plus `enforcement_unavailable_reason` and never claims that one exact session was disconnected.

### `GET /api/subscription-clients/{client_id}/usage`

Query:

- `window=hours12|day1|days3|week1|month1|months3|custom`
- `from_unix` for custom windows
- `to_unix`

Returns total usage plus durable usage points with node/cluster references.
Usage telemetry is persisted in `HYDRA_SUBSCRIPTION_USAGE_PATH`, default `data/subscription-usage.json`.
The persisted file is bounded by `max_subscription_usage_points_buffered`, so usage detail queries remain memory-bounded on the `512 MB RAM` target.

### `POST /api/subscription-clients/{client_id}/usage`

Admin/operator traffic ingestion endpoint for manual tests, imports, Telegram workflows, or future operator tools.

Body:

```json
{
  "at_unix": 1760000000,
  "node_id": "node-a",
  "cluster_id": "cluster-a",
  "bytes_downlink": 1048576,
  "bytes_uplink": 262144
}
```

Rules:

- at least one of `bytes_downlink` / `bytes_uplink` must be non-zero
- `node_id` and `cluster_id` are optional but validated when present
- both `node_id` and `cluster_id` may be present on one point, allowing UI breakdown by server and cluster
- updates the client's `used_traffic_bytes`
- appends one bounded durable usage point

### `POST /api/node-agent/subscription-clients/{client_id}/usage`

Node-agent traffic ingestion endpoint.
Requires the node auth token, not an admin session.

The request body is the same as the admin endpoint, but `node_id` is bound to the authenticated node:

- if `node_id` is omitted, the panel fills it from the node token
- if `node_id` is present and does not match the authenticated node, the request is rejected
- `cluster_id` may still be provided for cluster/route breakdown

### `POST /api/subscription-clients/{client_id}/usage/reset`

Resets the client's tracked usage counter and clears the client's durable usage points.

### `POST /api/subscription-clients/{client_id}/revoke`

Rotates the subscription token and marks the client revoked.

## Network Resources

Network resource audit:

- inbound writes emit `inbound_created`, `inbound_updated`, and `inbound_deleted`
- host writes emit `host_created`, `host_updated`, and `host_deleted`
- proxy profile writes emit `proxy_profile_created`, `proxy_profile_updated`, and `proxy_profile_deleted`
- audit detail uses only stable resource identifiers such as inbound tag, host id, or proxy profile id
- audit detail must not include host remark, address, SNI, path, proxy profile name, proxy `settings_json`, rendered config, or generated runtime material

### `GET /api/protocol-capabilities`

Returns the protocol capability matrix used by backend validation and future UI forms.

The response includes:

- `runtime_components`:
  runtime owner, component name, support status, production readiness, supervisor, supported lifecycle actions, required binaries, update strategy, validation strategy, and disabled reason
- protocol id and display name
- support status:
  `production`, `legacy`, or `planned`
- whether the protocol should be recommended by default
- runtime owner:
  `xray`, `sidecar`, `node_native`, or `planned`
- required binaries, for example `xray`, `hysteria`, or `wireguard-tools`
- required secret classes, for example TLS material, Hysteria auth secret, or WireGuard peer keys
- supported transports and supported security modes summarized from all modes
- `disabled_reason` when the protocol is visible but not usable in write/apply paths
- supported protocol modes:
  transport, security, production readiness, domain/path/plugin/secret-material requirements, and notes

Current policy:

- `vmess` remains available as `legacy` compatibility support.
- `vless`, `trojan`, and `shadowsocks` expose production-ready modes where renderer/apply support is expected.
- `hysteria2`, `wireguard`, and Shadowsocks plugin modes are modeled as planned/disabled until runtime ownership, renderer, apply-flow, and key/plugin lifecycle are implemented.
- non-Xray runtime owners fail closed until sidecar/node-native lifecycle management exists.
- runtime component metadata is schema-versioned with the protocol matrix so UI/apply code can distinguish "protocol is disabled" from "runtime component is missing lifecycle support".

### `GET /api/inbounds`
### `POST /api/inbounds`
### `PUT /api/inbounds/{tag}`
### `DELETE /api/inbounds/{tag}`

Inbound CRUD.

Inbound writes validate protocol, transport, and TLS combinations against production-ready capability modes.
Planned modes are visible in `GET /api/protocol-capabilities` but are rejected by inbound CRUD until fully implemented.

Inbound binding fields:

- `node_id` optional, binds the inbound/protocol endpoint to one node
- `cluster_id` optional, binds the inbound/protocol endpoint to one cluster
- create/update validates referenced nodes and clusters when these fields are present
- legacy user subscriptions still see all inbounds
- catalog-client subscriptions filter inbounds through the client's explicit node/cluster/protocol access policy
- when a catalog client uses an explicit allowlist, unbound/global inbounds are not included

### `GET /api/hosts`
### `POST /api/hosts`
### `PUT /api/hosts/{host_id}`
### `DELETE /api/hosts/{host_id}`

Host CRUD.

Host binding fields:

- `node_id` optional, binds the host endpoint to one node
- `cluster_id` optional, binds the host endpoint to one cluster
- create/update validates referenced nodes and clusters when these fields are present
- legacy user subscriptions still see all hosts
- catalog-client subscriptions filter hosts through the client's explicit node/cluster access policy
- when a catalog client uses an explicit allowlist, unbound/global hosts are not included

### `GET /api/proxy-profiles`
### `POST /api/proxy-profiles`
### `PUT /api/proxy-profiles/{profile_id}`
### `DELETE /api/proxy-profiles/{profile_id}`

Proxy profile CRUD.

## Nodes (Admin)

Node admin audit and secret handling:

- node create/update/delete emit `node_created`, `node_updated`, and `node_deleted`
- node auth-token rotation emits `node_auth_token_rotated`, returns the plaintext token once, and persists only the token hash
- local API token configure/clear emits `node_local_api_token_updated`, persists encrypted local token material, and never serializes the plaintext token in node API responses
- node bootstrap diagnostics emit `node_bootstrap_probe_requested`, not generic `node_updated`
- apply/retry/rollback emit `node_apply_requested`, `node_apply_retry_requested`, and `node_rollback_requested`
- audit detail uses stable node ids and must not include node auth tokens, local API tokens, node names, addresses, runtime action output, or operator retry/rollback reason text
- node-reported sync/apply/local-action diagnostic text is redacted and bounded before being stored or returned

Additional privileged action audit policy:

- applying a security preset emits `security_preset_applied`, not generic `security_settings_updated`
- manual user traffic reports emit `user_usage_reported`, not generic `user_updated`
- core start/stop/restart emit `core_start_requested`, `core_stop_requested`, and `core_restart_requested`

### `GET /api/nodes`

Returns current nodes with effective sync state.

### `POST /api/nodes`

Creates a node.

Creation does not return a deployable node-agent token. Operators or provisioning flow must call `POST /api/nodes/{node_id}/auth/rotate` once to issue the plaintext token that will be installed on the node.

### `PUT /api/nodes/{node_id}`

Updates node fields.

### `DELETE /api/nodes/{node_id}`

Deletes a node.

### `POST /api/nodes/{node_id}/auth/rotate`

Rotates the node auth token and returns the new plaintext token once.

Effects:

- old token is invalid immediately
- `auth_token_issued_at_unix` is updated on the node record
- use this same flow for first issuance, compromise recovery, or node reprovisioning

### `POST /api/nodes/{node_id}/apply`

Body:

```json
{
  "revision": "optional"
}
```

Marks the node as pending config apply.

### `POST /api/nodes/{node_id}/heartbeat`

Admin-side heartbeat update endpoint.

### `POST /api/nodes/{node_id}/sync`

Admin-side sync update endpoint.

### `GET /api/nodes/{node_id}/sync-history`

Query:

- `limit`

Returns node sync history, including bounded node-reported `apply_stages`, `apply_issues`, and optional `retry_state` when the node-agent provides them.

`retry_state` is a secret-free transport observability payload for the node-agent loop:

- `consecutive_failures`
- optional `retry_backoff_seconds`
- optional `next_retry_not_before_unix`
- optional truncated `last_transport_error`

It lets the panel distinguish an agent that is deliberately waiting under bounded retry backoff from an agent that has gone completely silent.

### `GET /api/nodes/{node_id}/apply-results`

Query:

- `limit`

Returns bounded node apply result history. This is separate from generic sync history and is intended for apply attempt debugging, retry decisions, rollback decisions, and future UI apply timelines.

### `POST /api/nodes/{node_id}/apply/retry`

Admin endpoint that creates a new pending apply request for the current generated revision or an explicitly supplied `revision`.

Body:

- optional `revision`
- optional `reason`

The endpoint appends a pending sync-history lifecycle entry. The node-agent sees the retry through `GET /api/node-agent/config` as `apply.apply_required=true`.

### `POST /api/nodes/{node_id}/rollback`

Admin endpoint that creates a pending rollback request.

Body:

- optional `target_revision`
- optional `reason`

If `target_revision` is omitted, the panel uses the latest known `last_good_revision` from apply results/sync history, then falls back to the node's last applied revision. The endpoint records a pending lifecycle entry with a `rollback` stage and `rollback_available=true`. The node-agent must report the final result through `POST /api/node-agent/apply-result`.

### `GET /api/nodes/{node_id}/diagnostics`

Returns aggregated node diagnostics:

- current panel-side node state
- optional local node health
- optional local node detailed state
- last Xray render summary when reported by the Rust node agent
- operator-facing recommendations

This is intended to back a higher-level diagnostics view rather than forcing the UI to manually stitch together multiple low-level calls.

### `GET /api/nodes/{node_id}/provisioning`

Returns persisted provisioning tasks for a node, newest first.

### `GET /api/nodes/{node_id}/provisioning/preflight`

Runs a panel-side preflight check before provisioning/reprovisioning.

Returns:

- `passed`
- `checked_at_unix`
- structured `checks`
- remediation recommendations
- remediation recommendations

### `GET /api/nodes/{node_id}/provisioning/ssh-preflight-probe`

Returns the canonical secret-free SSH shell probe for that node plus the required ports derived from the node record.
Executors should run this script remotely and submit stdout back as `ssh_preflight_output` on the `preflight` step instead of maintaining divergent probe logic.

### `GET /api/nodes/{node_id}/provisioning/ssh-install-plan`

Returns typed, secret-free shell scripts for the SSH installation stages:

- `sudo_check`
- `xray_install`
- `sidecar_runtime_install`
- `node_install`
- `service_install`

The Xray step downloads the official `XTLS/Xray-install` installer to a temporary file and invokes it explicitly.
It must not be executed with `curl | bash` or shell command substitution.

The sidecar runtime step is opt-in and safe by default:

- it always prepares `/etc/hydra-node/sidecar-generated/{hysteria2,wireguard}` with restrictive permissions
- the task-bound executor session exposes `install_plan.sidecar_install` and `install_plan.env_schema` so UI/automation can render explicit inputs instead of relying on hidden environment-variable knowledge
- `HYDRA_INSTALL_WIREGUARD=1` installs `wireguard-tools` through `apt-get`, `dnf`, or `yum`
- WireGuard opt-in also requires verified, matching release artifacts in `HYDRA_NODE_SESSION_ADAPTER_ARTIFACT_URL` and `HYDRA_NODE_WIREGUARD_DRIVER_ARTIFACT_URL`; the node install step places them under `/opt/hydra-node`
- the service step writes and enables a separate hardened `hydra-node-session-adapter.service`; it starts after and requires `hydra-node.service`, runs the driver without shell interpolation, and receives only `CAP_NET_ADMIN`
- a successful `service_started` handoff requires `additional_services` metadata proving that this adapter unit is loaded, active, enabled, uses the expected binary/env paths, and has the expected working directory whenever the task selected WireGuard; a node-only self-report is rejected
- `node.env` remains owner-only `0600` and may contain an independent local adapter token plus a persistent CSPRNG WireGuard session-reference key; neither value may equal the panel node token or appear in reports
- `HYDRA_INSTALL_HYSTERIA2=1` requires `HYDRA_HYSTERIA2_ARTIFACT_URL`, installs `/usr/local/bin/hysteria`, and writes a disabled `hydra-hysteria2@.service` template
- `hysteria2_artifact_url` is accepted only when `install_hysteria2=true` and must point to the official `https://github.com/apernet/hysteria/releases/...` path
- without those flags, no sidecar package, adapter, or driver binary is installed, so baseline node provisioning does not download unused protocol runtimes

The node install step requires the executor to provide `HYDRA_NODE_ARTIFACT_URL`, so the panel never hardcodes a release URL. Because the panel therefore never sees the value, the install script itself pins all three node artifact URLs — `HYDRA_NODE_ARTIFACT_URL`, `HYDRA_NODE_SESSION_ADAPTER_ARTIFACT_URL`, and `HYDRA_NODE_WIREGUARD_DRIVER_ARTIFACT_URL` — to `https://github.com/Zolotushka1/Hydra/releases/download/node-v*`. The check runs before `curl`, so a URL pointing anywhere else aborts the step instead of installing an unrelated binary under the node's systemd unit. The environment schema states the same prefix, so the executor learns the requirement before the run rather than from a failure.
The service step writes a hardened systemd unit with `NoNewPrivileges=true`, `ProtectSystem=strict`, and bounded write paths.

### `POST /api/provisioning/executor-handshake`

Returns the task-independent provisioning executor handshake document.

Body:

```json
{
  "executor_id": "executor-prod-a",
  "executor_contract_version": 1,
  "executor_version": "0.1.0",
  "capabilities": [
    "workflow_graph",
    "resume_projection",
    "recommended_next_command",
    "acceptance_contract",
    "machine_readable_rejections",
    "accepted_result_projection",
    "loop_contract"
  ]
}
```

The response declares:

- panel identity:
  product name and API version
- persisted executor registration:
  stable `executor_id`, enabled/disabled trust state, executor version, last contract version, last handshake timestamp, compatibility status, and accepted/rejected result counters
- executor contract schema and minimum executor version
- supported capabilities and the smaller set of required capabilities
- `missing_required_capabilities`
- compatibility verdict
- heartbeat policy shared by executor-driven provisioning
- security expectations:
  compatible-version result writes, no forged panel-observed transitions, no persisted secret payloads, remote attestation for sensitive handoffs

Compatibility is fail-closed and distinguishes:

- `compatible`
- `unknown_executor`
- `executor_upgrade_required`
- `panel_upgrade_required`
- `missing_required_capabilities`

Executors should complete this handshake before starting or resuming automated provisioning.

### `GET /api/provisioning/executors`

Lists registered provisioning executors.

The list is operator-facing metadata only:

- `executor_id`
- `enabled`
- last executor version and contract version
- last declared capabilities
- last handshake timestamp
- last compatibility status
- whether an executor auth token is configured
- auth token issue timestamp
- accepted/rejected result counters

The persisted registry stores only the executor auth token hash. Public API responses never return that hash or a plaintext token.

### `POST /api/provisioning/executors/{executor_id}/auth/rotate`

Rotates the provisioning executor auth token and returns the plaintext token once.

This endpoint requires an admin bearer token. The returned `auth_token` must be installed into the executor secret store and then used as:

```http
Authorization: Bearer <executor-auth-token>
```

The panel stores only a hash of this token. If the token is lost or suspected compromised, rotate it again and redeploy the new value to the executor.

### `GET /api/provisioning/executor-submissions`

Returns a bounded journal of recent provisioning executor result submissions.

Optional query filters:

- `executor_id`
- `node_id`
- `task_id`
- `status`:
  `accepted` or `rejected`
- `limit`

Each entry includes:

- `executor_id`
- `node_id`
- `task_id`
- result channel:
  `step_report`, `command_report`, `handoff_report`, or `panel_observed`
- `status`
- accepted workflow node id when available
- compact detail/reason
- timestamp

The journal is stored without secrets and is bounded by the existing node provisioning event buffer limit.

### `PUT /api/provisioning/executors/{executor_id}/trust`

Enables or disables a registered provisioning executor.

Body:

```json
{
  "enabled": false,
  "reason": "operator disabled this executor after a suspicious install report"
}
```

Disabled executors remain visible in the registry and may continue to handshake, but result writes are rejected fail-closed until an operator explicitly enables them again.

Provisioning executor audit:

- handshake emits `provisioning_executor_handshaked`
- trust changes emit `provisioning_executor_trust_updated`
- auth token rotation emits `provisioning_executor_token_rotated`
- trust audit records `reason_present=true|false`, not the operator-supplied reason text
- audit detail must not include plaintext executor tokens, token hashes, SSH credentials, private keys, command output, or request-time secret payloads

### `GET /api/nodes/{node_id}/provisioning/{task_id}/executor-session`

Returns one task-bound, secret-free executor contract that binds together:

- executor contract metadata:
  `schema_version`, `minimum_executor_version`, `capabilities`, and `compatibility`
- canonical preflight probe
- typed install scripts
- material handoff plan
- ordered executor actions for every planned task step
- heartbeat interval and stale timeout
- finish conditions
- current `completion` summary derived from task steps, handoff evidence, and bootstrap verification state
- per-step `expected_report` contract:
  `structured_step`, `command_report`, or `orchestration_step`
- typed `workflow` graph with executable nodes, dependency edges, and transition conditions
- `resume` projection for interrupted sessions:
  completed nodes, runnable nodes, replay policies, blocked nodes, and next node candidates
- top-level `loop_contract` describing the full executor cycle:
  read next command, execute bounded payload, submit accepted result channel, handle accepted/rejected result, continue or recover

This is the preferred contract for a future Rust SSH executor because it prevents clients from stitching the provisioning flow together differently.
Long-running install steps are explicitly marked with `requires_heartbeat=true`.
`completion` is the server-derived answer to "can this task honestly finish now?":
it lists missing/failed executor steps, missing/failed required handoffs, proof source per handoff, bootstrap readiness, and human-readable blockers.
`token_issued` and `agent_returned` are shown as `panel_observed`; `node_env_written` and `service_started` become `remote_attestation` only after valid attestations pass validation.
`workflow` is the machine-readable execution graph for a future SSH executor. It interleaves ordinary steps and handoff nodes so clients do not invent their own order:
node install -> token issue -> env attestation -> service install -> service attestation -> agent return -> post-install checks.
`workflow.validation` reports whether the generated graph passed internal invariants:
unique node ids, existing dependencies, acyclic graph, required SSH handoff nodes, and correct post-agent ordering.
`resume` projects persisted task state onto that workflow so an executor can recover after interruption without guessing:
`safe_to_replay` nodes may be repeated if needed, `reissue_required` nodes must mint fresh material before replay, and `panel_observed_only` nodes cannot be forged by executor replay.
It also exposes `recommended_next_action`, a single server-selected runnable workflow node with its typed step/handoff kind, endpoint suffix, `executor_should_submit` flag, heartbeat requirement, and explanation. Future executors should prefer this field when resuming one task rather than inventing their own next-step chooser; `executor_should_submit=false` marks panel-observed waits such as authenticated agent return, not work the executor is allowed to self-report.
`recommended_next_command` is the execution-ready envelope for that same node. It keeps one bounded payload together:
`payload_kind`, canonical probe/install script when applicable, expected report contract, required handoff kind, optional canonical attestation script, replay policy, endpoint suffix, and trust-boundary flags. Executors should consume this envelope instead of joining `steps`, `install_plan`, `material_handoff`, and `workflow` manually.
Each command envelope also includes an `acceptance` contract. It tells the executor which result channel the panel will accept (`step_report`, `command_report`, `handoff_report`, or `panel_observed`), the accepted result endpoint when executor submission is allowed, the required success fields, and the explicit fail-closed checks the panel will enforce. For panel-observed transitions such as token issuance and authenticated agent return, `executor_may_submit=false` makes it explicit that executor self-report is not authoritative.
Executor-facing result rejections from `POST .../step`, `POST .../command-report`, and `POST .../handoff` are machine-readable:
`code`, `recovery_hint`, and `retryable` accompany the human `error`. Current codes include `executor_identity_required`, `executor_not_registered`, `executor_disabled`, `invalid_preflight_evidence`, `wrong_result_channel`, `missing_node_env_attestation`, `missing_service_attestation`, `task_not_active`, `task_not_found`, and fallback `invalid_result`. This lets executors distinguish identity/registration/trust problems, payload correction, command-channel correction, remote re-attestation, session refresh, and terminal stop cases without parsing prose.
`incompatible_executor_contract` is returned when result submission omits or mismatches the required contract version; executors must refresh the session and stop automated progress until compatibility is restored.
Successful responses from those same executor result endpoints return a compact accepted-result projection rather than forcing an immediate second session fetch:
`accepted_node_id`, current task status, completed workflow node ids, next runnable node ids, and the next `recommended_next_command` when one exists. Executors may still refresh the full session explicitly, but normal step-by-step progression can continue from this bounded response.
`loop_contract` is the higher-level protocol wrapper around those pieces. It documents the ordered loop phases, the source of the next command, accepted/rejected result shapes, terminal conditions, and typed recovery rules. Future executors should follow this cycle instead of re-deriving control flow from individual payload fields.

Automated executors should call the endpoint with:

- `executor_contract_version`

Current session schema is `1`.
The panel currently requires executor contract version `1` and advertises the minimum executor binary version as `0.1.0`.
The compatibility result is fail-closed:

- `compatible`:
  the executor contract version matches what the panel supports
- `unknown_executor`:
  no `executor_contract_version` was supplied; the session is still inspectable by humans, but an automated executor must not run it
- `executor_upgrade_required`:
  the executor sent a contract version older than the panel minimum
- `panel_upgrade_required`:
  the executor sent a contract version newer than this panel understands
- `missing_required_capabilities`:
  version matches, but the executor omitted one or more capabilities required for safe automated execution

Current capability flags are:

- `workflow_graph`
- `resume_projection`
- `recommended_next_command`
- `acceptance_contract`
- `machine_readable_rejections`
- `accepted_result_projection`
- `loop_contract`

`material_handoff` is intentionally a plan, not a secret bundle:

- `/etc/hydra-node` must be created as `0700`
- `/etc/hydra-node/node.env` must be written as `0600`
- node auth token plaintext is issued one time through `POST /api/nodes/{node_id}/auth/rotate`
- executor composes `node.env` from node id, panel URL, and that freshly issued token, but the panel must not persist the resulting payload in provisioning history
- `env_schema` requires:
  `HYDRA_PANEL_URL`, `HYDRA_NODE_ID`, `HYDRA_NODE_AUTH_TOKEN`
- optional sidecar env keys are allowlisted but not required:
  `HYDRA_NODE_SIDECAR_RECIPE_MODE`, `HYDRA_NODE_SIDECAR_RUNTIME_CONFIG_PATH`, `HYDRA_NODE_HYSTERIA2_BINARY_PATH`, `HYDRA_NODE_HYSTERIA2_SERVICE_NAME`, `HYDRA_NODE_WIREGUARD_BINARY_PATH`, `HYDRA_NODE_WG_QUICK_BINARY_PATH`, `HYDRA_NODE_WIREGUARD_INTERFACE_NAME`
- `write_sequence` requires atomic `node.env` write before service start and plaintext-token disposal after the first successful agent authentication
- `expected_reports` gives the canonical typed handoff order:
  `token_issued`, `node_env_written`, `service_started`, `agent_returned`
- route private material is not preloaded over SSH; after installation, the authenticated node-agent fetches only its own least-knowledge material from `GET /api/node-agent/route-credentials`
- post-install proof is agent authentication, heartbeat, and then config/credential fetches before bootstrap verification

Bootstrap verification distinguishes ordinary panel-side heartbeat writes from authenticated node-agent heartbeats.
`agent_heartbeat` fails until the installed agent has called the node-agent heartbeat endpoint with its own token **after the latest node-token issuance**; a reachable local API or an old heartbeat alone is not enough to declare bootstrap complete after rotation/reprovisioning.

### `POST /api/nodes/{node_id}/provisioning/start`

Starts a new provisioning task for a node.

Body:

- `verify_after_finish`
- optional `executor`

This is panel-side orchestration state only. SSH credentials or other request-time secrets are not stored here.

Supported executor payload:

- `ssh_host`
- `ssh_port`
- `ssh_username`
- `ssh_password`
- `ssh_private_key_pem`

Validation:

- `ssh_username` is required when `executor` is used
- at least one of `ssh_password` / `ssh_private_key_pem` is required
- optional `ssh_host` must not be empty
- optional `ssh_port` must be greater than zero
- `ssh_private_key_pem`, when used, must look like a PEM/OpenSSH private key
- optional `executor.sidecar_install` controls OS-level sidecar runtime preparation:
  `install_hysteria2`, official `hysteria2_artifact_url`, and `install_wireguard`
- panel persists only a sanitized request context:
  transport, host, port, username, auth-method flags, and selected sidecar install options
- each created task also persists secret-free `executor_readiness`:
  transport, host/port/username readiness, auth-method readiness, node-token readiness, and operator recommendations
- tasks also expose normalized `failures` and structured `remediation`

New tasks are created with structured planned steps:

- `preflight`
- for SSH transport:
  `ssh_connect`, `sudo_check`, `xray_install`, `sidecar_runtime_install`, `node_install`, `service_install`
- `agent_reachability`
- `runtime_health`
- `config_apply`
- `bootstrap_verify`

Preflight also checks `node_auth_token_issued`.
Fresh nodes must have a deployable node-agent token issued before provisioning verification can pass, otherwise the installed agent cannot authenticate to the panel.

### `POST /api/nodes/{node_id}/provisioning/reprovision`

Starts a fresh provisioning task for an existing node.

Accepts the same optional `executor` payload as `provisioning/start`.

### `POST /api/nodes/{node_id}/provisioning/{task_id}/step`

Appends a provisioning step update.

Body:

- `step`
- `status`
- `detail`
- optional `failure_category`
- optional `remote_prerequisites` for the `preflight` step
- `from_command_report` is internal panel-derived state and must not be sent by arbitrary clients

Normalized remote prerequisite kinds:

- `os_supported`
- `sudo_available`
- `disk_ok`
- `memory_ok`
- `ports_available`
- `package_manager_available`

When provided on the `preflight` step, they are persisted separately from free-form step logs and later drive operator recommendations.
Failed prerequisites are also normalized into provisioning failures:

- `sudo_available=false` -> `authorization`
- `ports_available=false` -> `connectivity`
- unsupported OS, low disk, low memory, or missing package manager -> `validation`

That lets the existing remediation engine produce actionable next steps without the executor having to hand-author every failure mapping.

Successful step reporting is fail-closed:

- automated executor submissions must include `executor_id=...` and `executor_contract_version=1` in the query string
- automated executor submissions must authenticate with the executor bearer token issued by `POST /api/provisioning/executors/{executor_id}/auth/rotate`; admin session tokens are not accepted for these result writes
- `preflight=succeeded` requires canonical `ssh_preflight_output` or normalized `remote_prerequisites`
- `ssh_connect`, `sudo_check`, `xray_install`, `node_install`, and `service_install` must succeed through `command-report`; a bare `/step` success is rejected
- orchestration-only steps may still use `/step` with explicit `status` + `detail`
- `/step` free-form `detail` is redacted and bounded before it is stored in task steps, failures, and provisioning events; it must not be used to store tokens, passwords, private keys, rendered env files, or full command output

### `POST /api/nodes/{node_id}/provisioning/{task_id}/command-report`

Preferred machine-executor endpoint for shell command results.

Automated executor submissions must include `executor_id=...` and `executor_contract_version=1` in the query string.
They must also authenticate with the executor bearer token, not an admin session token.

Body:

```json
{
  "step": "sudo_check",
  "exit_code": 1,
  "stdout": "optional raw stdout",
  "stderr": "optional raw stderr"
}
```

The panel:

- derives `succeeded` vs `failed` from `exit_code`
- keeps bounded stdout/stderr summaries only
- redacts obvious secret lines such as token/password assignments and private-key blocks
- classifies common failures into existing categories such as `connectivity`, `authentication`, `authorization`, or `runtime`
- records the normalized result through the existing step/failure/remediation pipeline

### `POST /api/nodes/{node_id}/provisioning/{task_id}/handoff`

Automated executor submissions must include `executor_id=...` and `executor_contract_version=1` in the query string.
They must also authenticate with the executor bearer token, not an admin session token.

Records typed, secret-safe executor handoff progress separately from shell steps.

Supported `kind` values:

- `token_issued`
- `node_env_written`
- `service_started`
- `agent_returned`

Supported `status` values:

- `pending`
- `succeeded`
- `failed`

The task keeps the latest state for each handoff kind, while bounded provisioning events retain the timeline.
`detail` is redacted and bounded before persistence, so executors must report status context only and never rely on this endpoint to store plaintext tokens, private keys, rendered env contents, or full command output.
SSH tasks cannot be finalized as `completed` or `verified` until every required handoff kind has a latest `succeeded` report.
`agent_returned` is special: the panel also auto-confirms it for the latest fresh active SSH task after a real authenticated node-agent heartbeat newer than the latest node-token issuance.
`token_issued` is also auto-confirmed by the panel itself when `POST /api/nodes/{node_id}/auth/rotate` succeeds for the latest fresh active SSH task.
`node_env_written` can only be reported as `succeeded` with a secret-free `node_env_attestation` that proves:

- path is `/etc/hydra-node/node.env`
- mode is `0600`
- owner is `uid=0,gid=0`
- target is a regular file
- executor used an atomic write
- key names match the current env schema
- `schema_fingerprint` matches the fingerprint exported in `material_handoff`

Values are never returned; this attestation verifies shape and local file safety, not plaintext secret contents.
`material_handoff` also returns a canonical `node_env_attestation_script` so executors do not maintain divergent metadata probes; the script emits only path/mode/owner/file/atomic/key-name/fingerprint facts, never env values.

`service_started` can only be reported as `succeeded` with a secret-free `service_started_attestation` that proves:

- service name is `hydra-node.service`
- unit file path is `/etc/systemd/system/hydra-node.service`
- systemd load state is `loaded`
- active state is `active`
- unit file state is `enabled`
- `ExecStart` path is `/opt/hydra-node/hydra-node`
- environment file path is `/etc/hydra-node/node.env`
- working directory is `/opt/hydra-node`

`material_handoff` exports a canonical `service_started_attestation_script`; it reads only systemd metadata and must not dump unit contents or secret-bearing environment files.

### `POST /api/nodes/{node_id}/provisioning/{task_id}/touch`

Refreshes `updated_at_unix` for a currently active provisioning task without creating a fake installation step.

Body:

```json
{
  "detail": "still installing packages"
}
```

Use this lightweight heartbeat during long executor operations so a healthy installation does not become `stale_active_task`.
The heartbeat is persisted as a bounded provisioning event with `kind=heartbeat`; only active `pending`/`running` tasks may be touched.

Supported step kinds are now typed:

- `preflight`
- `agent_reachability`
- `runtime_health`
- `config_apply`
- `bootstrap_verify`
- `ssh_connect`
- `sudo_check`
- `xray_install`
- `node_install`
- `service_install`

The canonical SSH executor preflight should emit normalized shell facts and map them into `remote_prerequisites` rather than uploading raw command logs as readiness:

- OS id
- sudo availability
- free root disk MB
- total memory MB
- package manager
- required-port availability

Current minimums used by the parser are `1024 MB` free disk and `512 MB` total RAM. Unknown or missing facts fail closed.
For the canonical parser path, submit the probe stdout as `ssh_preflight_output` on the `preflight` step; the panel derives `remote_prerequisites` from the node's own configured `port` and `api_port`.

### `POST /api/nodes/{node_id}/provisioning/{task_id}/finish`

Finalizes a provisioning task.

Body:

- `status`
- `detail`
- `run_bootstrap_probe`

If bootstrap probing is requested, the panel runs the node bootstrap verification and stores:

- `verified_ready`
- `verify_probe_id`
- remediation recommendations
- normalized `failures`
- structured `remediation`

When a request tries to finish as `completed`/`verified` before required executor steps or SSH handoff proofs are ready, the endpoint returns `400` with:

- `error`
- current server-derived `completion`

This lets an executor recover from the exact blocker without separately refetching the task/session state.

### `POST /api/nodes/{node_id}/provisioning/{task_id}/retry`

Creates a new provisioning task by retrying a previously failed one.

Failed tasks are retryable. A stale active task may also be retried after it has stopped reporting progress for more than 30 minutes.

Provisioning failure categories:

- `connectivity`
- `authentication`
- `authorization`
- `validation`
- `runtime`
- `apply`
- `bootstrap_verification`
- `unknown`

Provisioning remediation actions:

- `retry_task`
- `reprovision_node`
- `run_bootstrap_probe`
- `check_local_api`
- `check_ssh_connectivity`
- `check_sudo_access`
- `restart_node_runtime`
- `rollback_node_runtime`
- `update_xray_core`
- `apply_node_config`
- `review_firewall`
- `inspect_runtime_state`

### `GET /api/nodes/{node_id}/bootstrap-readiness`

Runs a fresh bootstrap verification pass and returns:

- `ready`
- `checked_at_unix`
- `failed_steps`
- operator-facing recommendations

This is intended for post-install verification and repair flows.

### `POST /api/nodes/{node_id}/bootstrap-probe`

Runs a detailed bootstrap probe and persists the resulting step history.

Each result includes:

- `probe_id`
- `ready`
- `checked_at_unix`
- per-step success/failure details

### `GET /api/nodes/{node_id}/bootstrap-history`

Returns persisted bootstrap probe step history for a node.

Supports:

- `?limit=...`

### `GET /api/nodes/{node_id}/local/health`

Fetches local Rust node agent health from the node debug surface.

### `GET /api/nodes/{node_id}/local/state`

Fetches detailed local Rust node agent state.

Includes:

- apply history
- runtime event history
- xray runtime status
- last xray render summary:
  renderer version, source revision, feature flags, inbounds/outbounds/rules counts, fail-closed state
- restart backoff state
- backup / rollback marker paths

### `POST /api/nodes/{node_id}/local/runtime/{action}`

Supported actions:

- `validate`
- `start`
- `stop`
- `restart`
- `rollback`

This proxies the matching local node runtime action.

### `POST /api/nodes/{node_id}/local/xray/update`

Triggers the local Rust node action that updates the Xray core from the official `XTLS/Xray-core` release feed.

## Node-Agent Contract

These routes are intended for the existing sibling Rust `Hydra-node` agent as its runtime integration is completed.

### `GET /api/node-agent/me`

Returns authenticated node identity and effective sync state.

### `GET /api/node-agent/config`

Returns:

- `node_id`
- current `revision`
- `apply` directive:
  `apply_required`, `target_revision`, `current_applied_revision`, `current_sync_status`, `requested_at_unix`, `reason`
- `apply_plan`: explicit safe apply sequence for node-agent/runtime UI
- `route_credential_status`: secret-free credential readiness summary for this node projection
- `runtime_config`: stable node-runtime-config document for the Rust node-agent
- `generated_config`: compatibility/debug projection of node-specific generated control-plane config

`runtime_config` is the primary contract for `node-runtime-config.json`.

Current `apply_plan` schema:

- `schema_version`: currently `2`
- `generated_at_unix`
- `target_revision`
- `apply_required`
- `least_knowledge`
- `credential_ref_count`
- `requires_route_credentials`
- `requires_xray_validation`
- `xray_binary_configured`
- `safe_restart_after_successful_validation`
- `runtime_components[]`:
  required runtime owner/component, required binaries, validation/update strategy, production readiness, and disabled reason
- `steps[]`: ordered safe apply steps with `step`, `required`, and `detail`

`safe_restart_after_successful_validation=false` when any required runtime component is not production-ready or carries a disabled reason. The node-agent must treat that as fail-closed and must not restart the runtime even if Xray validation itself would pass.

The plan is intentionally descriptive and secret-free. Node-agent implementations should use it as the operator/debug contract for the apply flow, but still fail closed if required credentials, runtime components, or Xray validation are unavailable.

Node apply status surfaces runtime component readiness through the standard `stages[]` list as `runtime_components`, with blocking issues and recommendations when a required component is disabled or not production-ready.

`POST /api/node-agent/sync` may include `runtime_components[]` reports from the node-agent. Reports are secret-free and bounded, and include owner/component, installed/healthy flags, version, last validation timestamp, last error, and checked timestamp. Apply status compares required components with the latest local-state or sync-history report and fails the `runtime_components` stage when a reported required component is unhealthy.

`route_credential_status` includes:

- `required_ref_count`
- `active_ref_count`
- `revoked_required_refs`
- `missing_active_refs`
- `safe_to_apply`
- `detail`

It is secret-free by design. If a required ref is explicitly revoked, `safe_to_apply=false` and the node-agent must fail closed until an admin explicitly rotates/reissues that ref.

Current `runtime_config` schema:

- `schema_version`: currently `1`
- `generated_at_unix`
- `revision`
- `node`: redacted node identity:
  `node_id`, `name`, `address`, optional `port`, optional `api_port`, `enabled`
- `apply`: same apply directive as the top-level response
- `runtime`:
  `xray_config_path`, optional `xray_binary_path`, `restart_policy`, `least_knowledge`
- `config`:
  redacted users/inbounds/hosts/nodes projection for this authenticated node
- `route_assignments`:
  route assignments filtered to this authenticated node
- `credential_refs`:
  sorted unique credential references required by the route assignments

Contract rules:

- `runtime_config` must be secret-free
- `runtime_config.config.nodes` must contain redacted node identities only, never full node records
- private route material must be fetched separately through `GET /api/node-agent/route-credentials`
- node-agent implementations should treat `runtime_config` as the stable file to persist as `node-runtime-config.json`
- node-agent implementations should expose `apply_plan` in local state/logs so operators can see which phase failed
- `generated_config` remains available for compatibility/diagnostics, but new runtime code should prefer `runtime_config`
- if the node-agent wants the panel-rendered Xray document instead of rendering locally, it can call `GET /api/node-agent/xray-config`

If the authenticated node has cluster route assignments, the returned config is sanitized for least-knowledge operation:

- `node_route_assignments` is filtered to the authenticated node
- full cluster topology is omitted
- graph-like `cluster_node_targets` are omitted
- unrelated nodes are omitted
- users/inbounds/hosts are omitted for non-entry cluster nodes

The `apply` directive is the node-agent's primary control signal. A node should apply when `apply_required=true`, then report progress through `POST /api/node-agent/sync` using `apply_lifecycle_state`, bounded `apply_stages`, and bounded `apply_issues`. The directive is derived from the current generated revision, panel-side node sync state, last applied revision, and the latest pending apply request in sync history.
- route peers expose only previous/next peer projection, not the complete path
- hop security is explicit in the assignment through `security` and `auth`

Current security contract:

- generated cluster assignments require `mutual_tls`
- `credential_ref` values are references to node-local or future provisioned material, not inline private keys
- `allow_insecure` must remain `false` for production cluster assignments
- node runtimes resolve `credential_ref` locally and must fail closed rather than silently rendering insecure relay hops when material is missing
- panel-generated config must not inline private keys, SSH secrets, or raw TLS key material

`runtime_config.contract` is the node-facing least-knowledge diagnostic block.
It includes:

- `valid`
- `fail_closed`
- `least_knowledge`
- local `node_id`
- projected node ids visible to this node
- route assignment count
- credential ref count
- issue count and issue list

The contract is valid only when the projection contains the local node identity, route assignments belong only to that node, and `credential_refs` exactly match refs used by local route assignments.
If the contract is invalid, the node-agent must treat the config as unsafe and stop before replacing runtime state or restarting Xray.
Those contract issues are also mirrored into `render_summary.issues` with `route_id=runtime_config` so apply diagnostics can explain whether failure came from the runtime projection or from Xray JSON validation.
For `GET /api/node-agent/xray-config`, invalid contract issues are also injected into `xray_config.raw_config_validation` as blocking validation issues. This makes the apply pipeline fail closed before external validation, runtime-state write, or Xray restart.

### `GET /api/node-agent/xray-config`

Returns the Xray render output derived from the authenticated node's `runtime_config`.

Response:

- `node_id`
- `revision`
- `runtime_config`
- `apply_plan`
- `route_credential_status`
- `render_summary`
- `runtime_validation_report`
- `xray_config`

`render_summary` includes:

- `renderer_version`
- `source_revision`
- optional `xray_detected_version`
- `feature_flags`
- inbound/outbound/routing rule counts
- `fail_closed`
- bounded render/validation issues

Contract rules:

- this endpoint must authenticate exactly like other `/api/node-agent/*` routes
- `xray_config` is produced from the node-local `runtime_config` projection, not the full panel config
- `xray_config.raw_config` is the candidate `xray.json` payload
- `xray_config.raw_config_validation` is the internal validation report
- `runtime_validation_report` combines runtime component readiness, route credential readiness, render issues, raw Xray validation issues, and the latest node-reported external Xray validation into one fail-closed report with `valid`, `safe_to_restart`, and bounded issues
- missing external Xray validation is a warning and blocks `safe_to_restart`; the node-agent must report a `passed` external validation after running the real Xray binary before runtime restart is considered safe
- route credential private key material is still not included; fetch it through `GET /api/node-agent/route-credentials`
- a safe apply sequence is:
  fetch `runtime_config`, fetch credentials, render or fetch `xray_config`, validate with Xray, write runtime state, then report through `POST /api/node-agent/sync` and `POST /api/node-agent/apply-result`

### `GET /api/node-agent/route-credentials`

Returns node-specific route credential material for authenticated node-agent use.

This is intentionally separate from `/api/node-agent/config`:

- generated config remains non-secret least-knowledge runtime intent
- private keys are not embedded into the general config payload
- only the authenticated node receives material for its own `credential_ref` values
- current material kind is `mutual_tls`

Response:

- `node_id`
- `revision`
- `generated_at_unix`
- `credentials[]`

Each credential contains:

- `credential_ref`
- `kind`
- `certificate_pem`
- `private_key_pem`
- `ca_certificate_pem`
- optional `server_name`
- optional `certificate_pins`

Operational rule:

- this endpoint must be used only by node-agent authenticated traffic
- production deployments should use TLS for panel/node transport
- nodes must write private key material to local files with restrictive permissions and use local manifest references for Xray rendering

Panel persistence:

- route material store env: `HYDRA_ROUTE_MATERIALS_PATH`
- default: `data/route-materials.json`
- route material master key env: `HYDRA_ROUTE_MATERIALS_MASTER_KEY_B64`
- route material key file env: `HYDRA_ROUTE_MATERIALS_KEY_PATH`
- default key file: `data/route-materials.key`
- the store persists route CA and issued credential material so credentials are not regenerated on every request
- existing active credentials are reused while their `credential_ref` and `server_name` still match
- changed material is rotated by revoking the previous record and issuing a new active record
- explicitly revoked `credential_ref` values are stored as revocation tombstones and are not silently reissued on the next node-agent credential fetch
- explicit credential rotation clears the revocation tombstone and issues fresh active material
- private keys in the store are encrypted with AES-256-GCM
- if `HYDRA_ROUTE_MATERIALS_MASTER_KEY_B64` is not set, the panel creates/uses the local key file
- the key file is written with `0600` permissions on Unix
- losing the master key or key file makes existing encrypted route materials unrecoverable
- production deployments should provide and back up `HYDRA_ROUTE_MATERIALS_MASTER_KEY_B64` through a secret manager

## Route Material Admin API

These admin routes expose lifecycle metadata only. They do not return private keys.
Privileged route-material mutations create specific audit events:

- `route_credential_rotated`
- `route_credential_revoked`
- `route_ca_rotated`

Audit details may include `credential_ref`, but must never include private keys, encrypted private-key blobs, or route material master keys.

### `GET /api/route-materials`

Returns route material store status:

- CA timestamps
- active/revoked credential counts
- revoked `credential_ref` tombstones
- credential refs
- server names
- active/revoked state
- whether each private key is encrypted

### `POST /api/route-materials/credentials/rotate`

Body:

```json
{
  "credential_ref": "cluster/cluster-1/node/relay-1/mtls"
}
```

Revokes the active credential for that ref and issues a new one under the current route CA.
If the ref was previously revoked, this explicit rotation clears the revocation tombstone and restores node-agent delivery for that ref.

### `POST /api/route-materials/credentials/revoke`

Body:

```json
{
  "credential_ref": "cluster/cluster-1/node/relay-1/mtls"
}
```

Marks the active credential as revoked and records a revocation tombstone for that `credential_ref`.
The panel will not silently reissue that ref during the next node-agent credential fetch; the node will lose a usable secure route until an admin explicitly rotates the credential.

### `POST /api/route-materials/ca/rotate`

Rotates the route CA and reissues all active credentials.

Operational impact:

- nodes fetch route credential bundles during their tick
- node credential install is idempotent and compares file contents
- changed cert/key/CA material forces node config apply
- unchanged material does not force re-apply

### `POST /api/node-agent/heartbeat`

Updates:

- node runtime status
- xray version
- node version

### `POST /api/node-agent/sync`

Body includes:

- `sync_status`
- `applied_revision`
- `detail`
- optional `apply_lifecycle_state`:
  `pending`, `downloaded`, `rendered`, `validated`, `applied`, `failed`, `rolled_back`, `unknown`
- optional `last_good_revision`
- optional `rollback_available`
- optional `apply_stages[]`
- optional `apply_issues[]`
- optional `runtime_components[]`
- optional `external_xray_validation`
- optional `runtime_alerts[]`

Panel compares `applied_revision` with current expected revision and marks drift if needed.
Node-reported stages/issues/runtime alerts are bounded and truncated before persistence so a bad node cannot grow panel memory or sync history unboundedly.
`external_xray_validation` is a bounded, secret-free report from the node-agent's real `xray run -test -config ...` step. It is stored with sync history and can be used by runtime validation reports to decide whether restart is safe.
`runtime_alerts[]` is a bounded, secret-free list of active node-local runtime diagnostics. Current alert kinds are `poll_backoff`, `runtime_validation_failed`, `xray_runtime_failed`, `xray_update_failed`, `sidecar_failed`, and `sidecar_degraded`.
If `apply_lifecycle_state` is omitted, the panel infers a best-effort lifecycle state from `sync_status`, `applied_revision`, failed stages, error issues, and known stage names such as `fetch_config`, `render_xray`, `validate_xray`, `apply_runtime`, and `rollback`.

### `POST /api/node-agent/apply-result`

Body includes:

- `attempt_id`
- `target_revision`
- `status`: `applied`, `failed`, `rolled_back`, `skipped`
- optional `started_at_unix`
- optional `finished_at_unix`
- optional `applied_revision`
- optional `last_good_revision`
- `rollback_available`
- `safe_to_restart`
- optional `detail`
- optional `apply_stages[]`
- optional `apply_issues[]`
- optional `runtime_components[]`
- optional `external_xray_validation`

This endpoint records a dedicated apply attempt result and also writes a sync-history entry with the matching lifecycle state. `applied` with the current expected revision marks the node synced. `failed` and `rolled_back` mark the node drifted so the panel does not treat the runtime as converged.
Apply results preserve the external Xray validation report for audit/debugging. A node-agent should only set `safe_to_restart=true` after internal validation, external Xray validation, durable runtime-state write, and restart-safety checks have all passed.

`attempt_id` is idempotent per `node_id + target_revision`. If the node-agent retries the same result after a network timeout, the panel returns the existing entry and does not append duplicate apply-result or sync-history records.

### `POST /api/node-agent/logs`

Uploads bounded node log lines.
These are appended into panel operational logs with node prefixing.

### `POST /api/node-agent/metrics`

Uploads:

- memory used/total
- disk used/total

These values are stored on node state.

## Public Subscription

### `GET /sub/{subscription_token}`

Public subscription route.

Query:

- `format=json|plain_text|base64`
- `device_id`, required for catalog clients with `max_simultaneous_devices`

Returns legacy user subscription output or catalog-client subscription output if the token is active and not revoked. Catalog clients fail closed when disabled, expired, revoked, past `expire_at_unix`, or when a limited subscription omits a valid active `device_id`.

The default public format is `base64`.

- `plain_text` returns newline-delimited client URIs
- `base64` returns standard Base64 of the same URI list
- `json` returns the safe structured client bundle
- `diagnostic_json` is rejected on this public route

The renderer currently emits:

- `vless://` for VLESS
- `vmess://` with the standard Base64 JSON payload for VMess
- `trojan://` for Trojan
- SIP002-style `ss://` for Shadowsocks
- `hysteria2://` for Hysteria2
- structured JSON only for WireGuard

Generated client credentials are the same credentials written into standalone Panel Xray or projected to the remote Rust Node. URI generation and runtime generation must never derive credentials through separate formulas.

## Subscription Catalog Roadmap

The public subscription endpoint now provides production client delivery for the currently supported protocols. The broader catalog model remains:

- operators can create multiple subscription groups/plans in the panel
- each subscription can contain its own clients
- each client can have explicit access to selected nodes, clusters, or server paths
- subscription rendering filters protocols, hosts, nodes, and cluster paths through that client access policy
- device/HWID/session policy should attach to the subscription client layer

Implemented API families:

- `/api/subscription-plans`
- `/api/subscription-plans/{plan_id}/clients`
- `/api/subscription-clients/{client_id}/node-access`
- `/api/subscription-clients/{client_id}/devices`
- `/api/subscription-clients/{client_id}/sessions`
- `/api/subscription-clients/{client_id}/usage`
- `/api/subscription-clients/{client_id}/usage/reset`
- `/api/subscription-clients/{client_id}/revoke`
- `/api/node-agent/subscription-clients/{client_id}/usage`
- `/api/node-agent/subscription-sessions/report`
- `/api/node-agent/subscription-sessions/enforcement-result`

Planned client fields:

- maximum simultaneously connected devices
- traffic limit
- expiration date
- operator note
- status/revocation state
- selected node/cluster access policy

Usage detail API:

- fixed relative windows:
  `12h`, `1d`, `3d`, `1w`, `1m`, `3m`
- custom absolute range:
  `from_unix` and `to_unix`
- response must include total usage plus server/node breakdown
- queries must be bounded, aggregated, and safe for low-memory deployments
- storage is durable and bounded; long-term rollups may still be added later if we need retention beyond the configured point buffer

Telegram parity:

- Telegram bot workflows must be able to inspect client settings, show usage detail windows, reset traffic, revoke subscription, and delete clients where permitted
- destructive Telegram actions must require explicit confirmation and create audit events

Security/API requirements:

- client node access must be explicit, auditable, and deny-by-default
- subscription/client secrets must be redacted from bootstrap, telemetry, logs, and node runtime config
- relay node runtime payloads must remain least-knowledge and must not receive global subscription/client lists
- cluster graph APIs should expose references needed for UI visualization without exposing private route material

## Next Contract Changes Expected

The following areas are still expected to evolve:

- deeper xray runtime document model
- richer node provisioning/auth lifecycle
- notification policies beyond Telegram
- more precise core/node apply semantics

When changing or adding routes, update this file together with code.

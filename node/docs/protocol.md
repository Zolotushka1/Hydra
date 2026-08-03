# Node Protocol Notes

This document tracks the Rust node's current contract with `Hydra-Panel`.

## Authentication

Node requests authenticate with:

- `X-Hydra-Node-Token`

The node token is expected to be issued by the panel through node auth token rotation.

## Current Agent Flow

1. call `GET /api/node-agent/me`
2. call `GET /api/node-agent/config`
3. call `GET /api/node-agent/cluster-targets`
4. persist local cluster target state for diagnostics/runtime decisions
5. persist node-local least-knowledge route assignments
6. fetch and install node-local route credentials when assignments require them
7. send heartbeat
8. compare local revision and local runtime inputs with panel state
9. apply config if revision changed or runtime inputs changed
10. report sync
11. periodically send metrics
12. optionally upload bounded log batches

Sync reports include the current bounded `runtime_alerts` projection so the panel can persist node-local runtime diagnostics in sync history and feed health center / system / Telegram alert evaluation without live-polling the node.

If `GET /api/node-agent/cluster-targets` is unavailable, returns an error, or cannot be decoded, the agent falls back to `generated_config.cluster_node_targets` from `GET /api/node-agent/config`.
For production cluster routing, `generated_config.node_route_assignments` is the authoritative least-knowledge input.
Graph-like cluster targets are debug compatibility only.

## Important Constraints

- node should not keep unbounded log queues
- node should not assume config apply succeeded without reporting sync
- node should treat panel revision as the source of truth
- node should not skip apply solely because the revision matches when cluster targets, route assignments, or route credential material changed locally
- node should use bounded retry/backoff after failed ticks instead of hammering the panel at a fixed interval during outages

## Poll Retry / Backoff

The node keeps `consecutive_tick_failures` in local state.

Poll delay behavior:

- success: reset failures and sleep the configured normal poll interval
- failure: increment failures, persist `last_error`, and sleep `max(poll_interval, bounded_backoff)`
- backoff is bounded by `HYDRA_NODE_TICK_BACKOFF_MAX_SECONDS`

Environment:

- `HYDRA_NODE_TICK_BACKOFF_BASE_SECONDS`
- `HYDRA_NODE_TICK_BACKOFF_MAX_SECONDS`

This prevents tight retry loops against the panel while keeping the failure state visible in `/state` and `/health`.

## Local Debug Surface

The Rust node currently exposes a local-only HTTP surface:

- `GET /health`
- `GET /state`
- `GET /runtime/artifacts`
- `GET /runtime/validation-report`
- `GET /runtime/sidecars`
- `GET /runtime/alerts`
- `GET /runtime/events`
- `GET /runtime/apply-history`
- `POST /runtime/validate`
- `POST /runtime/start`
- `POST /runtime/stop`
- `POST /runtime/restart`
- `POST /runtime/rollback`
- `POST /runtime/sidecars/{sidecar}/{action}`
- `POST /runtime/subscription-sessions/observations`
- `POST /runtime/subscription-sessions/adapter/register`
- `GET /runtime/subscription-sessions/actions`
- `POST /runtime/subscription-sessions/actions/{action_id}/result`
- `POST /xray/update`

This surface is intended for local diagnostics while the rewrite is in progress.

Security:

- `GET /health` is intentionally unauthenticated for local health checks.
- protected endpoints are `GET /state`, `GET /runtime/artifacts`, `GET /runtime/validation-report`, `GET /runtime/sidecars`, `GET /runtime/alerts`, `GET /runtime/events`, `GET /runtime/apply-history`, `POST /runtime/*`, `POST /runtime/rollback`, and `POST /xray/update`.
- set `HYDRA_NODE_LOCAL_API_TOKEN` to require auth on protected endpoints.
- clients may send either `X-Hydra-Local-Token: <token>` or `Authorization: Bearer <token>`.
- default local bind remains `127.0.0.1:8081`.
- the local API must not be exposed publicly; token auth is a hardening layer, not a replacement for network isolation.
- All `/runtime/subscription-sessions/*` endpoints require their own `HYDRA_NODE_SESSION_ADAPTER_TOKEN` and `X-Hydra-Session-Adapter-Token` header; they are unavailable when that secret is not configured.
- Before submitting observations, an adapter registers/renews a lease at `POST /runtime/subscription-sessions/adapter/register`.
- Observation, action poll, and result routes require `X-Hydra-Session-Adapter-Instance`, matching the active lease owner.

`GET /state` now includes:

- apply history
- runtime event history
- local xray runtime status
- restart backoff state
- config backup path
- rollback marker path
- Xray update lifecycle:
  status, phase, target version, source release, backup path, detail
- cluster target count
- cluster targets assigned to this node
- derived cluster runtime intents
- least-knowledge node route assignments
- subscription session adapter capability state
- runtime validation report
- sidecar lifecycle state
- active runtime alert summaries

The same active runtime alert summaries are also included in `POST /api/node-agent/sync`.
They are derived state, not an append-only queue, and must remain bounded and secret-safe.

## Subscription Session Enforcement Boundary

The panel contract now defines:

- `POST /api/node-agent/subscription-sessions/report`
- `POST /api/node-agent/subscription-sessions/enforcement-result`

The existing Rust node contains the typed request/response models for this boundary and exposes its current readiness through:

- `/state.subscription_session_adapter`

Without a configured local adapter, safe status is `unsupported`:

- no node-managed exact active-session table exists yet
- no opaque exact runtime session reference is generated yet
- no one-session termination plus post-action absence verification exists yet

The node must not advertise:

- `opaque_session_reference`
- `exact_session_termination`
- `post_action_absence_verification`

until those behaviors are provided by a real node-managed runtime adapter. Managing the Xray process or disconnecting a complete principal is not equivalent to terminating exactly one violating subscription session and is not an acceptable fallback.

Observation-only staging is available for a trusted local runtime adapter:

- configure `HYDRA_NODE_SESSION_ADAPTER_TOKEN`
- register one live adapter instance and its declared capability set; only the current lease owner may submit snapshots or read/complete actions
- submit complete bounded snapshots to `POST /runtime/subscription-sessions/observations`
- snapshots may include session principal, optional device fingerprint and source IP for panel policy evaluation
- snapshots must declare no runtime enforcement capabilities and must not include `runtime_session_ref`
- the node forwards non-stale observations to the panel and exposes only counts/timestamps in `/state`
- `HYDRA_NODE_MAX_SESSION_OBSERVATIONS` bounds the snapshot size; default `2048`
- `HYDRA_NODE_MAX_PENDING_SESSION_ENFORCEMENTS` bounds unacknowledged exact actions; default `256`
- `HYDRA_NODE_SESSION_OBSERVATION_STALE_AFTER_SECONDS` bounds how long an unrefreshed snapshot is forwarded; default `120`
- `HYDRA_NODE_SESSION_ADAPTER_LEASE_SECONDS` bounds ownership freshness; default `90`
- `HYDRA_NODE_SESSION_ACTION_TIMEOUT_SECONDS` bounds exact command completion time; default `30`

This allows policy visibility without pretending a destructive action is executable. If the panel ever returns an enforcement action to this observation-only node, the node reports that action as `failed` rather than attempting a broad disconnect.

An exact-capable external local adapter can now integrate through the same protected boundary:

- submit the complete capability set:
  `opaque_session_reference`, `exact_session_termination`, `post_action_absence_verification`
- include a bounded opaque `runtime_session_ref` for each submitted observation
- poll `GET /runtime/subscription-sessions/actions` for panel-approved targeted commands
- execute termination only for that local opaque handle
- report the outcome through `POST /runtime/subscription-sessions/actions/{action_id}/result`
- an `applied` result is accepted only when it contains the matching opaque handle, `session_absent_after_action=true`, and a verification timestamp
- each returned command includes `expires_at_unix`; commands after that deadline are not executable and are failed back to panel

Raw runtime handles and queued commands remain in memory only and are exposed only to the token-protected local adapter surface. The node now bundles the fail-closed orchestration process for a trusted executable runtime driver. A protocol-specific driver is still required: Xray process management is not given false exact-session semantics.

### Session Adapter Client Crate

The workspace includes `crates/node-session-adapter-client` for physical local adapters.

The client:

- attaches `X-Hydra-Session-Adapter-Token` and `X-Hydra-Session-Adapter-Instance`
- registers or renews the adapter lease
- submits bounded observation snapshots
- polls deadline-bounded exact actions
- submits proof results
- rejects mismatched registration instance ids before making a network request

Adapters should depend on this crate instead of implementing raw HTTP calls directly.

The workspace also includes `crates/node-session-adapter`. It has two explicit modes.

Observation-only mode is the default behavior:

- reads `HYDRA_NODE_LOCAL_API_URL`, default `http://127.0.0.1:8081`
- requires `HYDRA_NODE_SESSION_ADAPTER_TOKEN`
- uses `HYDRA_NODE_SESSION_ADAPTER_INSTANCE_ID`, or a generated bounded adapter id
- renews a lease with no exact capabilities
- submits empty bounded snapshots, or reads an observation-only JSON snapshot from `HYDRA_NODE_SESSION_ADAPTER_SNAPSHOT_PATH`
- bounds snapshot file size through `HYDRA_NODE_SESSION_ADAPTER_MAX_SNAPSHOT_BYTES`, default `1048576`
- bounds snapshot observations through `HYDRA_NODE_SESSION_ADAPTER_MAX_SNAPSHOT_OBSERVATIONS`, default `2048`
- checks snapshot metadata stability through `HYDRA_NODE_SESSION_ADAPTER_SNAPSHOT_STABILITY_MILLIS`, default `100`
- rejects snapshot capabilities and `runtime_session_ref` in dry-run mode
- polls pending actions only for diagnostics

Exact driver mode is enabled only with:

- `HYDRA_NODE_SESSION_ADAPTER_DRY_RUN_OBSERVATION_ONLY=false`
- absolute `HYDRA_NODE_SESSION_ADAPTER_DRIVER_PATH`
- optional static argv JSON in `HYDRA_NODE_SESSION_ADAPTER_DRIVER_ARGS_JSON`
- optional `HYDRA_NODE_SESSION_ADAPTER_DRIVER_TIMEOUT_SECONDS`, default `10`, range `1..60`
- optional `HYDRA_NODE_SESSION_ADAPTER_DRIVER_MAX_OUTPUT_BYTES`, default and maximum `1048576`

The executable is started directly, never through a shell. The adapter appends `--operation <handshake|observe|terminate|verify>`. A versioned JSON request is sent through stdin, so `runtime_session_ref` and `session_id` never need to appear in argv. The driver must write exactly one bounded JSON response to stdout. Stderr is drained with the same bound but is not copied into panel results because it may contain runtime-sensitive data.

Driver request example:

```json
{
  "protocol_version": 1,
  "operation": "terminate",
  "session_id": "session-a",
  "runtime_session_ref": "opaque-runtime-owned-handle"
}
```

Handshake and observe responses must declare exactly:

```json
[
  "opaque_session_reference",
  "exact_session_termination",
  "post_action_absence_verification"
]
```

An observe response also supplies `observations`. Every observation must have a unique bounded `session_id` and `runtime_session_ref`. A verify response must contain `session_absent=true` and `verified_at_unix`. The adapter additionally performs a fresh observe and rejects success if the same opaque handle is still present.

Exact action processing is fail-closed:

1. reject expired, unsupported, or non-verifiable commands
2. require the pair `session_id + runtime_session_ref` in the latest trusted snapshot
3. invoke targeted terminate
4. invoke independent absence verification
5. load and validate a fresh exact runtime table
6. report `applied` only when the exact handle is absent from that table

On Unix, the configured driver must be a regular executable file, must not be a symlink, and must not be group/world-writable. Static argv count/length, process time, stdout/stderr, observation count, ids, and opaque handles are bounded. Driver errors are fail-closed and the target opaque handle is redacted from the bounded result detail.

This executable contract does not make Xray exact-session capable. A driver that only restarts Xray, removes a complete principal, or disconnects all devices for one user is invalid and must not declare the capability set. Observation-only mode remains correct until the selected runtime exposes a genuine exact handle and targeted termination primitive.

### WireGuard Exact Peer Driver

`crates/node-session-driver-wireguard` is the first runtime-specific implementation of the exact driver contract. Its unit of enforcement is one WireGuard peer public key. This is valid only when Hydra provisions a distinct peer key for each client device.

Generated profile credentials are also the client-delivery contract. The Panel
uses the same explicit UUID/password material for public subscription rendering,
standalone Xray generation, and remote Node projections. The Node must consume
that material as provided and must not derive a second credential from mutable
display metadata.

During apply, node-core:

- renders every WireGuard user profile as a separate `[Peer]`
- requires explicit non-empty `allowed_ips` for every peer
- derives a stable device fingerprint from the peer public key unless the panel supplied an explicit fingerprint
- writes `sidecar-generated/wireguard-session-map.json` with `0600` permissions on Unix
- excludes interface private keys, peer endpoints, AllowedIPs, and unrelated sidecar secrets from that map

The runtime driver requires:

- `HYDRA_NODE_WIREGUARD_BINARY_PATH`, normally `/usr/bin/wg`
- `HYDRA_NODE_WIREGUARD_SESSION_MAP_PATH`, default `data/sidecar-generated/wireguard-session-map.json`
- `HYDRA_NODE_WIREGUARD_SESSION_REF_KEY`, a dedicated random secret of at least 32 bytes
- optional `HYDRA_NODE_WIREGUARD_ACTIVE_WITHIN_SECONDS`, default `180`
- optional `HYDRA_NODE_WIREGUARD_COMMAND_TIMEOUT_SECONDS`, default `5`
- optional `HYDRA_NODE_WIREGUARD_MAX_COMMAND_OUTPUT_BYTES`, default and maximum `1048576`

For every configured interface, the driver executes direct argv without shell interpolation:

```text
wg show <interface> dump
wg set <interface> peer <public-key> remove
```

The driver treats a peer as active only when its latest handshake is non-zero and no older than the configured activity window. Because WireGuard is connectionless, `connected_at_unix` represents the latest handshake, not the beginning of a TCP-style connection.

The driver never exposes the peer public key as `runtime_session_ref`. It derives domain-separated HMAC-SHA256 values for `session_id` and `runtime_session_ref`. Before removal it re-reads the live peer table and requires both opaque values to match the same peer. Verification performs a new `wg dump`; the adapter then performs its own additional observe and accepts `applied` only if the same opaque runtime ref remains absent.

The mapping file must be a regular non-symlink owner-only file and is bounded to 1 MiB, 32 interfaces, and 4096 peers. Interface names, peer keys, runtime usernames, fingerprints, child-process time, and command output are validated and bounded.

WireGuard runtime removal is immediate but not durable configuration revocation by itself. Panel policy and subsequent generated config must stop authorizing a revoked device key; otherwise a later `wg-quick` re-apply can restore that peer and the policy loop will need to remove it again.

Xray and Hysteria2 do not use this exact driver:

- Xray's supported user-management action removes a user/principal rather than one selected connection
- Hysteria2 exposes online counts and `/kick` by client id, but the kick affects that client id and its reconnect logic can reconnect

Those runtimes must remain observation-only for exact device enforcement unless their upstream runtime APIs gain a genuine connection-specific termination primitive.

The node nevertheless performs factual device-principal observation for these runtimes:

- generated Xray config enables `StatsService`, `stats`, and level-0 user uplink/downlink counters on `HYDRA_NODE_XRAY_STATS_API_ADDRESS`, default `127.0.0.1:10085`
- the node runs `xray api statsquery` without resetting counters, detects counter deltas, and treats the principal as recently active for `HYDRA_NODE_RUNTIME_ACTIVITY_WINDOW_SECONDS`, default 120 seconds
- generated Hysteria2 config uses the official `userpass` backend keyed by `catalog/{client_id}/device/{device_id}`
- every Hysteria2 inbound receives a loopback Traffic Stats API listener starting at `HYDRA_NODE_HYSTERIA2_TRAFFIC_STATS_BASE_PORT`, default 19090, with a per-inbound HMAC-derived authorization secret
- the node reads Hysteria2 `/online` and reports each non-zero client principal
- local queries are bounded by `HYDRA_NODE_RUNTIME_STATS_TIMEOUT_SECONDS`, default 5 seconds, and a 1 MiB response limit

These observations are sent to the panel with an empty `runtime_capabilities` list and no `runtime_session_ref`. Xray observations mean recent traffic activity. Hysteria2 observations mean an online principal and may represent more than one client instance. Neither may be presented as an exact connection or used to claim an exact disconnect.

Revoking a device removes its protocol-specific credential from the next generated runtime revision. WireGuard can additionally remove its exact peer immediately. Xray and Hysteria2 rely on the safe config apply/reload to make revocation durable; any still-visible observation is reported as blocked with `enforcement_unavailable_reason`, not a fabricated successful termination.

Snapshot file handoff must be atomic:

1. write the new JSON to a temporary file in the same directory as `HYDRA_NODE_SESSION_ADAPTER_SNAPSHOT_PATH`
2. fsync/close the temporary file
3. atomically rename the temporary file over the configured snapshot path
4. never update the configured snapshot path in-place

The dry-run adapter reads the snapshot fail-closed:

- missing file means an empty snapshot
- non-regular files are rejected
- files larger than the configured byte limit are rejected
- metadata must stay stable before and after the read
- changed or partially written files fail the current tick and are retried later

Lease safety:

- only one non-expired local adapter instance can own the session boundary at a time
- a new instance cannot read or complete commands owned by the previous live instance
- lease expiration clears staged observations and reports pending panel actions as failed before a new instance can resume operation
- command deadline expiration reports the individual pending action as failed even if the owning lease remains live

## Least-Knowledge Relay Model

Production clusters must have only one operating model:

- least-knowledge node route assignments

Cluster relay nodes should receive only node-specific route assignments.

Panel-side UI may keep the full graph, but node payloads should be projected down to:

- local route id
- local role
- local listen definition
- previous peer
- next peer

Relay nodes should not receive:

- full cluster graph
- complete upstream/downstream node lists
- final exit topology unless they are the exit
- users/subscription tokens
- unrelated node inventory
- route edge ids that reveal topology

The current protocol keeps `cluster_targets` only for development/debug compatibility.
It must not become a selectable production cluster mode.

The production path is:

- `node_route_assignments` in generated config
- `route_assignments` in `node-runtime-config.json`
- final Xray routing rendered from route assignments

Once panel-side assignment generation is complete, runtime decisions should stop depending on graph-like `cluster_targets`.

## Cluster Runtime Intent

The node derives local `cluster_runtime_intents` from panel-provided cluster targets.

Current intent fields describe:

- cluster id/name/revision
- local cluster-node ids hosted by this node
- local roles:
  `entry / relay / exit`
- upstream node ids
- downstream node ids
- route edge ids
- booleans for local responsibilities:
  accepts client entry / relays cluster traffic / handles cluster egress

This is an intermediate runtime model.
It is intentionally not yet the final Xray routing document.

The next step is to convert this intent into concrete node-local runtime config once the panel-side cluster routing contract is stable enough.

## Node Runtime Config Document

On config apply, the agent now writes a separate node-local runtime document.

Default path:

- `data/node-runtime-config.json`

Environment override:

- `HYDRA_NODE_RUNTIME_CONFIG_PATH`

The document currently contains:

- schema version
- local node id
- source panel revision
- source generation timestamp
- source user/node counts
- inbounds
- hosts
- derived cluster runtime intents
- least-knowledge route assignments
- bounded required protocol list

This file is not the final Xray JSON and must not be used as direct Xray runtime input yet.
It is the stable intermediate layer for the next routing/apply implementation.

`required_protocols` records which protocol families the current config needs and which runtime component owns them.
Examples:

- generated Xray/VLESS inbound -> Xray component
- generated Hysteria2 inbound -> Hysteria2 sidecar
- generated WireGuard inbound -> WireGuard sidecar/tools
- least-knowledge VLESS route assignment -> Xray component

This list is bounded and should not contain secrets, subscription tokens, private keys, or full cluster topology.

## Xray Render Diagnostics

The local `/state` response includes `last_xray_render_summary` after a successful apply.

It is intended for operator/UI diagnostics and contains:

- renderer version
- source revision
- detected Xray version, when known
- renderer feature flags
- final inbound/outbound/routing-rule counts
- `fail_closed` boolean
- bounded render issues with route id, scope, severity, and reason
- render timestamp

Use this summary to explain whether a node applied a usable runtime config or intentionally blocked insecure/missing route material.

## Runtime Validation Report

The next backend readiness layer is a unified `runtime_validation_report` in `/state`.

Purpose:

- show which runtime components are actually usable on the node
- distinguish ready, missing, failed, disabled, and unknown states
- explain why a protocol/runtime path is unavailable before UI starts rendering it as selectable
- keep readiness bounded and operator-safe

Planned component families:

- Xray core
- Hysteria2 sidecar
- WireGuard sidecar/tools
- route credential material
- future protocol-specific helpers

Protocol readiness is derived from component readiness:

- `vless_tls_websocket` requires Xray
- `trojan_tls` requires Xray
- `shadowsocks_obfs` requires Xray
- `vmess` requires Xray
- `hysteria2` requires the Hysteria2 sidecar
- `wire_guard` requires the WireGuard sidecar/tools

If a component is missing, failed, unknown, or disabled, dependent protocols must report blocked/disabled with a visible reason.

The report also includes the protocols required by the last generated `NodeRuntimeConfigDocument`.
This is stricter than global protocol availability:

- a globally optional disabled protocol does not by itself make the node unready
- a disabled/blocked protocol required by the current runtime config makes `runtime_validation_report.ready=false`
- each required protocol status keeps its source and source reference so UI/API can explain what config item is blocked
- required protocol blockage makes local node status `degraded`
- required protocol blockage changes node sync reports to `drifted`, even if the Xray-only part rendered and validated successfully
- sync detail includes the blocking reason so panel/UI can surface why the node is not fully ready

Implemented execution order:

1. expose Xray component readiness using configured apply mode, binary path, detected version, final `xray.json`, and last validation status
2. expose Hysteria2/WireGuard sidecar readiness with explicit binary/toolchain preflight
3. add typed sidecar lifecycle contract:
   install, update, validate, start, stop, restart, status, logs
4. render sidecar-owned payloads and generated sidecar config files when material is complete
5. validate sidecar executor session results before marking sidecar-owned protocol requirements ready
6. feed component and protocol readiness into panel/API readiness so unavailable protocols are fail-closed with visible reasons

The report must not include secrets, raw certificates, private keys, route credential contents, subscription tokens, or full cluster topology.

## Sidecar Lifecycle Contract

The local node API reserves:

- `POST /runtime/sidecars/hysteria2/install`
- `POST /runtime/sidecars/hysteria2/update`
- `POST /runtime/sidecars/hysteria2/validate`
- `POST /runtime/sidecars/hysteria2/start`
- `POST /runtime/sidecars/hysteria2/stop`
- `POST /runtime/sidecars/hysteria2/restart`
- `POST /runtime/sidecars/hysteria2/status`
- `POST /runtime/sidecars/hysteria2/logs`
- `POST /runtime/sidecars/hysteria2/{action}/result`
- the same action set for `wireguard`
- `GET /runtime/sidecar-executor-session`
- `POST /runtime/sidecar-executor-session/result`

Current behavior:

- actions return a structured lifecycle response with command plan and acceptance contract
- `validate` uses safe binary preflight through configured binary paths
- `status`, `install`, `update`, `start`, `stop`, `restart`, and `logs` can execute real operator-configured argv commands
- when `HYDRA_NODE_SIDECAR_RECIPE_MODE=standard`, node can also derive allowlisted OS recipe argv for already installed Hysteria2/WireGuard runtimes
- configured or recipe argv commands are executed directly without shell interpolation
- command stdout/stderr are bounded before being stored or returned
- actions without explicit argv or an available standard recipe remain fail-closed placeholders
- all actions include a command plan and acceptance contract
- executor result submission is explicit and fail-closed
- placeholder command plans use `executor_required=false` and `dry_run=true`
- placeholder acceptance contracts use `fail_closed=true` and expected status `disabled`
- configured command plans use `dry_run=false`, expected `ready/running` status, and require exit code `0`
- unsupported/fail-closed calls are recorded as operator-visible runtime events
- every call updates `/state.sidecars` with bounded operator-visible state
- result submissions are accepted only when `command_id`, expected status, exit code, and all required checks match the acceptance contract
- rejected result submissions update sidecar state to `failed` and record an operator-visible runtime event
- executor session submission groups all currently planned sidecar envelopes under one `session_id`
- session results are accepted only when every required command id is present exactly once and every envelope-level acceptance contract passes
- rejected session results fail closed, record an operator-visible runtime event, and mark related sidecar state as `failed`
- `status` and `validate` actions perform safe binary preflight when a sidecar binary path is configured
- preflight checks only:
  configured path, file existence, safe version probe
- preflight does not install, update, start, stop, restart, or modify services
- regular node tick refreshes sidecar preflight state for Hysteria2 and WireGuard
- automatic tick refresh updates `/state.sidecars` and `runtime_validation_report`, but does not record a runtime event for every poll
- a `sidecar_preflight_state_changed` runtime event is recorded only when status, support flag, configured binary, detected version, or detail changes

Sidecar binary environment:

- `HYDRA_NODE_HYSTERIA2_BINARY_PATH`
- `HYDRA_NODE_WIREGUARD_BINARY_PATH`
- `HYDRA_NODE_WG_QUICK_BINARY_PATH`

Sidecar standard recipe environment:

- `HYDRA_NODE_SIDECAR_RECIPE_MODE=standard`
- `HYDRA_NODE_HYSTERIA2_SERVICE_NAME`, default `hysteria-server.service`
- `HYDRA_NODE_WIREGUARD_INTERFACE_NAME`, default `hydra-wg0`

Standard recipe behavior:

- Hysteria2 Linux service actions use `systemctl start/stop/restart/is-active` and `journalctl --no-pager -n 80 -u <service>`
- Hysteria2 Windows service actions use `sc.exe start/stop/query <service>`
- WireGuard validation uses `wg-quick strip <generated.conf>`
- WireGuard start/stop use `wg-quick up/down <generated.conf>`
- WireGuard status uses `wg show <interface>`
- WireGuard Linux logs use `journalctl --no-pager -n 80 -u wg-quick@<interface>.service`
- service and interface names are allowlisted before command construction
- install/update remain explicit argv or provisioning/installer responsibilities; default recipes never run `curl | bash` or package-manager commands implicitly
- panel SSH provisioning exposes opt-in production wiring for these runtime dependencies:
  `HYDRA_INSTALL_WIREGUARD=1` installs `wireguard-tools` through the supported package manager;
  `HYDRA_INSTALL_HYSTERIA2=1` requires an explicit `HYDRA_HYSTERIA2_ARTIFACT_URL`, installs `/usr/local/bin/hysteria`, and prepares a disabled `hydra-hysteria2@.service` systemd template
- panel executor sessions expose typed `install_plan.sidecar_install` and `install_plan.env_schema`; executors should derive these install variables from that contract instead of hardcoding hidden sidecar choices
- baseline node provisioning does not install sidecar binaries unless those opt-in flags are set
- `scripts/integration-sidecars-linux.sh` is the real-host opt-in preflight harness for Hysteria2 binary/service metadata and WireGuard `wg`/`wg-quick strip` checks; run it on target OS images before enabling sidecar protocols by default

Sidecar explicit command environment:

- `HYDRA_NODE_HYSTERIA2_INSTALL_ARGS_JSON`
- `HYDRA_NODE_HYSTERIA2_UPDATE_ARGS_JSON`
- `HYDRA_NODE_HYSTERIA2_START_ARGS_JSON`
- `HYDRA_NODE_HYSTERIA2_STOP_ARGS_JSON`
- `HYDRA_NODE_HYSTERIA2_RESTART_ARGS_JSON`
- `HYDRA_NODE_HYSTERIA2_STATUS_ARGS_JSON`
- `HYDRA_NODE_HYSTERIA2_LOGS_ARGS_JSON`
- `HYDRA_NODE_WIREGUARD_INSTALL_ARGS_JSON`
- `HYDRA_NODE_WIREGUARD_UPDATE_ARGS_JSON`
- `HYDRA_NODE_WIREGUARD_START_ARGS_JSON`
- `HYDRA_NODE_WIREGUARD_STOP_ARGS_JSON`
- `HYDRA_NODE_WIREGUARD_RESTART_ARGS_JSON`
- `HYDRA_NODE_WIREGUARD_STATUS_ARGS_JSON`
- `HYDRA_NODE_WIREGUARD_LOGS_ARGS_JSON`

Each value is a JSON string array such as `["systemctl","restart","hydra-hysteria2"]`.
Do not pass shell strings; the node does not execute through a shell.

WireGuard readiness requires both:

- configured `wg` binary
- configured and existing `wg-quick` helper

If `wg` exists but `wg-quick` is missing or unset, WireGuard sidecar status is `degraded`, not `ready`.
That keeps WireGuard protocol readiness blocked until the local toolchain is complete.

Sidecar binary/toolchain readiness is not enough to make a sidecar-owned protocol ready.
Hysteria2 and WireGuard protocols stay blocked until the node has generated sidecar material, an existing generated sidecar config file, a ready/running sidecar component, and an accepted executor session matching the current requirement.
If a current config requires Hysteria2/WireGuard while only binary preflight is ready, node sync remains `drifted` with an operator-visible sidecar runtime blocking reason.

During apply, the node also writes `data/sidecar-runtime-config.json` by default.
The path is configurable with `HYDRA_NODE_SIDECAR_RUNTIME_CONFIG_PATH`.
This document is an intermediate sidecar runtime contract.
It records sidecar-owned protocol requirements, typed sidecar payloads, and planned executor envelopes.
It also includes typed sidecar runtime payloads when the panel-provided material is complete:

- `hysteria2_configs`
- `wireguard_configs`

Hysteria2 payload rendering requires:

- matching `GeneratedProxyProfile.settings_json`
- `password`, `auth`, or `auth_password`
- a canonical runtime username; generated YAML uses `auth.type=userpass`, never an anonymous password list
- existing non-empty `tls_certificate_file`/`certificate_file`
- existing non-empty `tls_key_file`/`key_file`

WireGuard payload rendering requires:

- matching `GeneratedProxyProfile.settings_json`
- `private_key` or `interface_private_key`
- `address` or `interface_address`
- `peer_public_key` or `public_key`
- `peer_endpoint` or `endpoint`
- optional `allowed_ips`, defaulting to `0.0.0.0/0` and `::/0`

If required material is missing, the sidecar requirement remains blocked and no partial sidecar payload is emitted.
During apply, typed payloads are also written as generated candidate config files next to `sidecar-runtime-config.json`:

- `sidecar-generated/hysteria2/{tag}.yaml`
- `sidecar-generated/wireguard/{tag}.conf`

These files are generated runtime inputs for explicit argv sidecar executors.
They are secret-sensitive because Hysteria2 configs can contain auth passwords and WireGuard configs contain private keys.
On Unix, generated sidecar config directories are chmod `0700` and files are chmod `0600`.
The node still keeps sidecar protocol readiness blocked until executor validation confirms that these files were applied successfully.
Each sidecar requirement also contains `planned_envelopes` for the executor loop.
Each planned envelope includes:

- sidecar id
- action
- command id
- command plan
- acceptance contract
- reason

Current planned envelopes cover `validate`, `start`, and `status`.
They are executable only when explicit argv is configured, every generated config file exists, and the envelope is not a dry-run placeholder.
The node can also expose these envelopes as one executor session through `GET /runtime/sidecar-executor-session`.
The session includes:

- `session_id`
- source revision
- requirement count
- envelope count
- executable flag
- fail-closed flag
- session acceptance contract
- full requirement list
- flattened envelope list
- per-envelope generated config path
- per-envelope generated config existence flag

`POST /runtime/sidecar-executor-session/result` validates the complete session result.
It rejects missing, duplicate, unexpected, failed, or config-file-missing envelope results.
After a session result is accepted, the node persists the accepted `session_id`, source revision, and command ids.
A sidecar-owned runtime requirement becomes ready only when:

- generated sidecar payload exists
- generated sidecar config file exists
- the relevant sidecar component is `ready` or `running`
- the accepted executor session still matches the current requirement session

If any of those conditions changes, the requirement falls back to fail-closed `blocked`.
When the matching sidecar runtime requirement is ready, the Xray render issue `non_xray_protocol_requires_sidecar` no longer blocks node sync.

`/state` exposes:

- `local_sidecar_runtime_config_path`
- `last_sidecar_runtime_config_saved_at_unix`
- `last_sidecar_runtime_summary`
- `runtime_validation_report.sidecar_runtime`
- `runtime_artifacts`

`runtime_validation_report.sidecar_runtime` contains:

- sidecar runtime config path
- last sidecar runtime summary
- requirement count
- blocked count
- blocked requirement refs
- readiness flag
- operator-safe detail

`GET /runtime/validation-report` returns the same readiness report as a smaller local diagnostics endpoint.

`/state.runtime_artifacts` is the operator-safe manifest for generated runtime files.
It includes artifact kind, path, existence, last saved timestamp, whether it is executable runtime input, whether it is secret-sensitive, and a short detail.
Route credential manifest/directory artifacts are marked secret-sensitive and must not be exposed as raw file contents through UI/API.
`GET /runtime/artifacts` returns the same manifest as a smaller local diagnostics endpoint.

`/state.sidecars` contains one view per known sidecar:

- sidecar id
- status
- supported flag
- configured binary path
- detected version
- last action
- last detail
- last validation timestamp
- updated timestamp
- bounded recent logs

`GET /runtime/sidecars` returns the same sidecar state list as a smaller local diagnostics endpoint.

`GET /runtime/alerts` returns bounded active alert summaries derived from current node state.
Alerts are intended for panel/UI/Telegram policy consumers and are not a persisted queue.
Current alert kinds cover poll-loop backoff, runtime validation failure, Xray runtime failure, Xray update failure, degraded sidecar, and failed sidecar.
Each alert includes `alert_id`, `kind`, `severity`, `source`, `active`, bounded `detail`, and `observed_at_unix`.
Alert details must remain secret-safe and must not include private keys, route credentials, node tokens, or raw config payloads.

`GET /runtime/events` and `GET /runtime/apply-history` return the same bounded history lists exposed in `/state`.
They are diagnostics endpoints for UI screens that need recent operational history without fetching the full node snapshot.

Regular generated Xray inbounds require explicit client credential material before they can be rendered.
The node reads this material from `GeneratedUserConfig.proxy_profiles[*].settings_json`:

- VLESS and VMess require `id` or `uuid`
- Trojan requires `password`
- Shadowsocks requires `method` and `password`
- `proxy_type` must match the inbound protocol
- optional `inbound` or `inbounds` fields can restrict a profile to one or more inbound tags
- `settings_json` must be valid JSON; invalid profile settings are reported as `generated_inbound_profile_settings_invalid`

If material is missing, the renderer omits the inbound, records `generated_inbound_client_material_missing`, marks the render summary fail-closed, and node sync reports `drifted`.
If a generated inbound sets `tls_enabled=true`, the matching profile must also provide existing non-empty TLS files:

- `tls_certificate_file` or `certificate_file`
- `tls_key_file` or `key_file`

If TLS files are missing or empty, the renderer omits the inbound, records `generated_inbound_tls_material_missing`, marks the render summary fail-closed, and node sync reports `drifted`.
Shadowsocks profiles that request `plugin`, `plugin_opts`, `plugin_options`, `obfs`, `obfs_mode`, or `v2ray_plugin` are also fail-closed with `generated_inbound_obfs_plugin_unsupported` until a native plugin/sidecar runtime contract exists.
This prevents empty VLESS/Trojan/Shadowsocks/VMess settings, incomplete TLS settings, or unsupported obfs/plugin behavior from being treated as production-ready Xray JSON.

Real Xray compatibility can be checked with an opt-in integration test:

```bash
HYDRA_TEST_XRAY_BINARY=/path/to/xray cargo test -p node-core real_xray_accepts_generated_production_protocol_documents_when_configured -- --nocapture
```

On a Windows-hosted development machine with a downloaded Xray bundle, the PowerShell form is:

```powershell
$env:HYDRA_TEST_XRAY_BINARY = "<path-to>\xray.exe"
cargo test -p node-core real_xray_accepts_generated_production_protocol_documents_when_configured -- --nocapture
```

When running Cargo from inside WSL Ubuntu, use the WSL interop path instead:

```bash
HYDRA_TEST_XRAY_BINARY=/path/to/xray cargo test -p node-core real_xray_accepts_generated_production_protocol_documents_when_configured -- --nocapture
```

The test validates generated production protocol JSON through `xray run -test -config`.
It covers the production-ready Xray renderer cases that must stay compatible with real Xray:

- `VLESS + TLS + WebSocket`
- `Trojan + TLS`
- `VMess`
- base `Shadowsocks`

Shadowsocks obfs/v2ray-plugin remains fail-closed and is not treated as a working Xray runtime until a native plugin/sidecar lifecycle exists.
It is skipped when `HYDRA_TEST_XRAY_BINARY` is not set so default unit tests remain portable on development hosts without Xray installed.

Implementation rules:

- install/update/validate/start/stop/restart/status/logs must stay explicit actions
- every implemented action must return a bounded command plan and acceptance contract before execution or state transition
- executor results must be checked against the acceptance contract before sidecar readiness changes
- sidecar actions must be protected by the local API auth boundary
- failed sidecar validation must not mark the node protocol-ready
- logs must be bounded and secret-safe
- persisted sidecar state must stay bounded and must not store secrets, raw configs, private keys, tokens, or full command output
- sidecar readiness must feed back into `runtime_validation_report`
- sidecar support must not weaken the existing Xray update/apply safety path

Current render issue reasons include:

- `route_listen_missing`
- `listen_mtls_material_missing`
- `listen_reality_not_supported`
- `next_peer_endpoint_missing`
- `next_peer_mtls_material_missing`
- `next_peer_reality_not_supported`
- `generated_inbound_missing`
- `generated_inbound_protocol_unknown`
- `generated_inbound_profile_settings_invalid`
- `generated_inbound_client_material_missing`
- `generated_inbound_tls_material_missing`
- `generated_inbound_obfs_plugin_unsupported`
- `non_xray_protocol_requires_sidecar`

The issue list is bounded and must not include private keys, raw certificates, auth tokens, subscription tokens, or full cluster topology.

Apply state rule:

- a revision is marked as applied only after render, validation, and runtime start/restart succeed
- failed validation or runtime apply must keep the previous applied revision
- failed validation or runtime apply must write a rollback marker and expose the render summary/details in `/state`
- rollback restores the backup `xray.json`, then validates/applies it through the same runtime action path
- successful rollback clears the rollback marker
- failed rollback keeps the marker and records operator-visible failure detail in `/state`

## Xray Core Update Lifecycle

`POST /xray/update` downloads the official platform asset from `XTLS/Xray-core` GitHub releases and replaces the configured Xray binary only through the guarded update flow.

Safety rules:

- `HYDRA_NODE_XRAY_BINARY_PATH` is required
- existing binary is backed up before replacement
- downloaded binary must pass version detection
- current `data/xray.json` must validate with the updated binary before restart
- if version detection or validation fails, the previous binary is restored
- if no previous binary exists, the invalid downloaded binary is removed
- if Xray was running before the update, restart is attempted only after validation passes

Operator visibility:

- `/state.last_xray_update_status`
- `/state.last_xray_update_phase`
- `/state.last_xray_update_target_version`
- `/state.last_xray_update_source_release`
- `/state.last_xray_update_backup_path`
- `/state.last_xray_update_detail`

Update phases are persisted before risky/blocking work so UI/API consumers can see whether an update is in preflight, release selection, runtime stop, backup, download, version detection, validation, restart, succeeded, or failed state.

## Final Xray Config

The agent now writes a separate final Xray config file.

Default path:

- `data/xray.json`

Environment override:

- `HYDRA_NODE_XRAY_CONFIG_PATH`

Only this file should be validated and passed to Xray runtime actions.
The first renderer version emits a minimal valid Xray document:

- `log`
- `inbounds`
- `outbounds`
- `routing`

When `route_assignments` are present, the renderer uses them as the authoritative cluster input:

- assignment listen definitions become local VLESS inbounds
- assignment next peers become route-specific VLESS outbounds
- routing rules bind assignment inbound tags to the matching next-peer outbound
- graph-like `cluster_targets` are not used for final Xray rendering

Security behavior is fail-closed:

- assignment `security.required=true` means the hop must not be rendered as an insecure plaintext relay
- current panel assignments use `mutual_tls` and `credential_ref` for node-local certificate material
- `credential_ref` is a reference only; it must not contain private keys or raw secrets
- hop transport is rendered as VLESS over TCP with TLS/mTLS stream settings when material is available
- VLESS client IDs are stable UUIDs derived from opaque `identity_ref`, not from real topology ids
- outbound hop security resolves local mTLS material; outbound hop auth identity targets the next peer
- outbound next peers must include both `address` and `port`; missing endpoint data blocks the route
- missing required mTLS material removes the affected inbound/rule instead of downgrading it to plaintext
- until mTLS/Reality material rendering is implemented, required secure hops are not opened as insecure routes
- rendered plans include `secure-route-material-pending-fail-closed` when secure route material is required but not yet renderable
- sidecar-owned protocols must not be emitted into `xray.json`; current renderer omits them and records `non_xray_protocol_requires_sidecar`

### Route Credential Manifest

The node resolves route credential references from a local manifest:

- env: `HYDRA_NODE_ROUTE_CREDENTIALS_PATH`
- default: `data/route-credentials.json`
- material directory env: `HYDRA_NODE_ROUTE_CREDENTIALS_DIR`
- material directory default: `data/route-credentials`

The manifest stores file paths and metadata only. It must not store inline private key text.

Example:

```json
{
  "credentials": [
    {
      "credential_ref": "cluster/cluster-1/node/relay-1/mtls",
      "kind": "mutual_tls",
      "certificate_file": "/etc/hydra-node/routes/cluster-1/relay-1.crt",
      "private_key_file": "/etc/hydra-node/routes/cluster-1/relay-1.key",
      "ca_certificate_file": "/etc/hydra-node/routes/cluster-1/ca.crt",
      "server_name": "relay-1.cluster-1.hydra.internal"
    }
  ]
}
```

Security rules:

- recommended manifest permissions: `0600`
- certificate, private key, and CA certificate files must exist and be non-empty before the renderer opens a required secure hop
- missing material keeps the route fail-closed
- route credentials are node-local and must not be expanded into panel-generated config
- relay nodes still receive only their local assignment projection, not full topology
- the node requests `/api/node-agent/route-credentials` while it has route assignments
- route credential installation is idempotent and compares cert/key/CA file contents
- after installing changed route material, the node forces config apply even if the panel revision did not change
- unchanged route material does not force re-apply
- changed node-local route assignments or cluster targets also force apply even if the panel revision did not change
- if revision, route credentials, route assignments, and cluster targets are all unchanged, the tick reports synced without re-applying
- credential install must write/check certificate, private key, and CA in the same pass
- change detection must not short-circuit in a way that skips writing part of the material set

## Xray Render Roadmap

The runtime pipeline should remain explicitly layered:

1. `Panel generated config`
2. `NodeRuntimeConfigDocument`
3. `SidecarRuntimeConfigDocument`
4. `XrayRenderPlan`
5. final `xray.json`
6. validate/restart runtime

Implementation requirements:

- add a dedicated `xray_render` layer
- keep `generated-config.json`, `node-runtime-config.json`, `sidecar-runtime-config.json`, and `xray.json` as separate files
- keep `last_xray_render_summary` visible in local state for diagnostics
- keep `last_sidecar_runtime_summary` visible in local state for diagnostics
- keep `runtime_artifacts` visible in local state so UI can show apply-flow artifact status without reading files directly
- validate and run only the final `xray.json`
- keep `POST /xray/update` as the one-command Xray update path
- after binary update, validate current `xray.json` before restarting runtime
- existing Xray binary is backed up before replacement
- if the new binary fails version detection or current `xray.json` validation, restore the previous binary and persist an operator-visible failure event
- add renderer compatibility handling for Xray version / renderer schema / feature flags

## External Process Mode

The agent supports an optional explicit external process mode for runtime actions.

Environment:

- `HYDRA_NODE_XRAY_APPLY_MODE=external_process`
- `HYDRA_NODE_XRAY_BINARY_PATH`
- `HYDRA_NODE_XRAY_VALIDATE_ARGS_JSON`
- `HYDRA_NODE_XRAY_RUN_ARGS_JSON`

Notes:

- arguments are JSON arrays of strings
- `{config_path}` placeholder is expanded to the final `xray.json` path
- if `HYDRA_NODE_XRAY_VALIDATE_ARGS_JSON` is not set, validation defaults to:
  `run -test -config {config_path}`
- run args are still explicit; starting a long-running Xray process requires `HYDRA_NODE_XRAY_RUN_ARGS_JSON`
- there are intentionally no hardcoded default launch flags in the agent
- if external process mode is not configured, the safe default remains validation-oriented apply behavior

If the external process exits unexpectedly:

- node marks runtime as `failed`
- schedules bounded automatic restart with backoff
- records the event in runtime event history

## Xray Core Update

The local node API supports:

- `POST /xray/update`

Behavior:

- fetch latest release metadata from official `XTLS/Xray-core`
- choose the asset for the current platform
- download and extract the `xray` binary
- replace the configured local binary path
- detect the installed version from `xray version`
- validate current `xray.json` with the updated binary before restart
- restart the runtime if it was running before the update
- persist `last_xray_update_detail` and `xray_core_update_failed` runtime event before returning update errors
- do not restart runtime blindly if validation fails

Current supported auto-update asset mapping:

- `linux/x86_64 -> Xray-linux-64.zip`
- `linux/aarch64 -> Xray-linux-arm64-v8a.zip`
- `linux/arm -> Xray-linux-arm32-v7a.zip`

## Apply Safety

During config apply:

- previous final `xray.json` is backed up to a `.bak` file when present
- if runtime transition fails after config write, node writes a rollback marker file
- successful runtime transition clears the rollback marker
- local operator can explicitly restore the latest backup through `POST /runtime/rollback`

This is intended to make failed apply states explicit for operators and future repair/provisioning flows.

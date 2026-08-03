# The panel-node contract

The panel owns configuration; the agent applies it. Everything crossing that
boundary is a versioned document, and the boundary is deliberately narrow: the
agent learns what it needs to run its own traffic and nothing about the rest of
the deployment.

## Documents

| Document | Direction | Contents |
| --- | --- | --- |
| `node_runtime_config` | panel to node | the node-local view of what should be running |
| `apply_plan` | panel to node | the ordered steps of an apply, with validation gates |
| route credentials | panel to node | mTLS and Reality material for this node's routes |
| sync report | node to panel | applied revision, health, bounded runtime alerts |

`node_runtime_config` is also rendered for administrators, so it must not carry
secret material. Reality private keys travel through
`/api/node-agent/route-credentials` instead — a `node_agent`-exposure route that
the admin surface never reaches.

Route credential documents carry references, never keys: a `credential_ref` names
material, and the material itself is written to the node's own credential
directory. Manifests hold file paths and metadata at mode `0600`; private keys
and certificates are `0600` inside a `0700` directory.

## Apply semantics

The effective apply boundary is the panel revision **plus** the node-local
runtime inputs. Changed cluster targets, changed least-knowledge route
assignments or changed route credential material force an apply even when the
panel revision is unchanged. An unchanged revision with unchanged runtime inputs
reports as synced without re-applying, so the loop does not churn.

Failure never advances state. A failed validate, start or restart does not mark a
new revision as applied. A validation or runtime failure leaves a rollback
marker, a render summary and an operator-visible detail. A rollback validates and
applies the restored backup through the normal runtime path rather than a
shortcut; a successful rollback clears the marker, and a failed one keeps it and
records why.

Failed ticks back off exponentially within a bound; a successful tick resets to
the normal poll interval.

Credential installation is idempotent and compares local certificate, key and CA
contents in a single pass. Short-circuit change checks that could skip writing
part of the material are not used — installing changed material forces an apply,
while unchanged material must not.

## Fail-closed readiness

Readiness is never assumed from the presence of a binary. A Hysteria2 or
WireGuard protocol stays blocked until the generated payload exists, the
generated config file exists, the sidecar component is ready, and the accepted
executor session still matches the current requirement. Sidecar binary readiness
and sidecar protocol readiness are separate things.

Blockage is visible rather than silent: local status becomes degraded and sync
reports are sent as `drifted` with the blocking reason in the detail. A required
secure route is never downgraded to plaintext forwarding for convenience, and
missing certificate or key material fails closed rather than falling back.

## Session enforcement

A local session adapter may submit bounded snapshots for panel policy
evaluation. An observation-only adapter declares no capabilities and never
receives executable destructive behaviour. An adapter orchestrating a trusted
runtime driver declares the exact-session capability set only after a successful
driver handshake.

`node-session-driver-wireguard` is the first exact driver: a recently handshaken
WireGuard peer is one device and one session, opaque identifiers are HMAC-derived,
and termination removes only the matched peer. Exact actions are matched against
the latest runtime table, executed for one opaque handle, verified separately,
and checked against a refreshed runtime table before `applied` is reported.

Xray process lifecycle remains non-exact. A runtime-specific driver that cannot
terminate one session and prove its absence must not enable exact mode.

Only one live adapter instance holds the lease. A competing instance cannot take
over active runtime handles or pending commands; an expired lease fails
outstanding actions back to the panel, clears staged observations and requires a
new registration. Every exact action carries its own bounded completion deadline,
so an active lease does not let an enforcement request stay pending forever.

The agent-side detail of all of this is in
[`node/docs/protocol.md`](../node/docs/protocol.md).

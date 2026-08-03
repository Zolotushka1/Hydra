# Repository layout

Hydra is a monorepo holding two independent Cargo workspaces:

```
panel/   the control plane: HTTP API, orchestration, configuration rendering
node/    the node agent: applies what the panel renders, reports back
docs/    product documentation (this directory)
LICENSE  AGPL-3.0, repeated at panel/LICENSE and node/LICENSE
```

The workspaces are separate rather than merged into one. They are deployed on
different machines with different dependency sets and different release cadence,
and a shared lockfile would tie the agent's dependency graph to the panel's for
no benefit. Each carries its own `Cargo.toml`, `Cargo.lock` and `target/`.

## Panel workspace

| Crate | Contents |
| --- | --- |
| `panel-app` | HTTP server bootstrap, routing, auth extraction |
| `panel-config` | runtime configuration, persistence paths, bounded runtime limits |
| `panel-core` | application services, state transitions, persistence helpers |
| `panel-domain` | domain models and API payload types |
| `panel-installer-executor` | the installer-side executor binary |

`panel-core/src/routes.rs` holds `ROUTE_TABLE`, the single source of truth for
the HTTP surface; `panel-core/src/schemas.rs` holds the schema version registry;
`panel-domain/src/registry.rs` holds the enum registry. `panel/web/` is a
temporary SolidJS shell, to be replaced by a Leptos CSR frontend served as static
assets.

## Node workspace

| Crate | Contents |
| --- | --- |
| `node-app` | binary entrypoint and polling loop |
| `node-config` | runtime configuration |
| `node-core` | panel client and node runtime orchestration |
| `node-domain` | typed request and response models |
| `node-session-adapter-client` | typed helper client for local runtime session adapters |
| `node-session-adapter` | fail-closed observation-only or exact runtime-driver adapter process |
| `node-session-driver-wireguard` | WireGuard peer-per-device exact runtime driver |

## Why `panel/` and `node/` and not `Hydra-Panel/` and `Hydra-node/`

The directory names are short and lower-case for four reasons, all of which
outlast the preference that motivated them:

- The repository is already called `Hydra`, so `Hydra/Hydra-Panel/crates/panel-core/`
  repeats the product name twice in every path.
- The crates are already named `panel-*` and `node-*`. `panel/crates/panel-core`
  agrees with that naming; `Hydra-Panel/crates/panel-core` does not.
- Lower-case removes a case-sensitivity trap between Windows, a supported target,
  and Linux. `Hydra-node` against `Hydra-Node` breaks silently on a
  case-insensitive filesystem and works everywhere else.
- Windows path length is a real limit.
  `node/crates/node-session-driver-wireguard/src/main.rs` is already deep, and
  twelve extra characters in every path are not free against `MAX_PATH`.

Renaming these directories back to the long form would reintroduce all four.

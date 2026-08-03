# Hydra-node

Rust node agent for Hydra. This repository is the remote-node runtime, not the
main panel. The main server can run `Hydra-Panel` standalone; this agent is
used for remote nodes, scaling, cluster relays, and node-local runtime
operations.

The agent-side protocol is documented in [`docs/protocol.md`](docs/protocol.md).
Product documentation covering both halves of Hydra — resource budget, security
model, persistence, protocol policy and the panel-node contract — is in
[`../docs/`](../docs/README.md).

## Local Dev / Operator Launch

The easiest verified launch path is from the sibling `Hydra-Panel`
repository:

```powershell
cd \\wsl.localhost\Ubuntu\home\root1\projects\Hydra-Panel
powershell -ExecutionPolicy Bypass -File scripts/smoke-panel-node.ps1
```

Strict mode with real Xray validation:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/smoke-panel-node.ps1 -XrayBinaryPath "<path-to>\xray.exe"
```

The smoke script starts the panel, creates a node, rotates a one-time node auth
token, starts this Rust node agent, waits for poll/apply/sync, and verifies the
panel apply status against node-local state.

## Manual Node Run

For manual debugging, start `Hydra-Panel` first and create/rotate a node auth
token from the panel API. Then run:

```powershell
$env:HYDRA_PANEL_URL = "http://127.0.0.1:18080"
$env:HYDRA_NODE_TOKEN = "<one-time-node-auth-token-from-panel>"
$env:HYDRA_NODE_LOCAL_API_BIND = "127.0.0.1:18081"
$env:HYDRA_NODE_LOCAL_API_TOKEN = "local-dev-token"
$env:HYDRA_NODE_XRAY_APPLY_MODE = "noop"
$env:HYDRA_NODE_POLL_INTERVAL_SECONDS = "1"

$env:HYDRA_NODE_STATE_PATH = ".dev/node-state.json"
$env:HYDRA_NODE_CONFIG_PATH = ".dev/generated-config.json"
$env:HYDRA_NODE_RUNTIME_CONFIG_PATH = ".dev/node-runtime-config.json"
$env:HYDRA_NODE_SIDECAR_RUNTIME_CONFIG_PATH = ".dev/sidecar-runtime-config.json"
$env:HYDRA_NODE_XRAY_CONFIG_PATH = ".dev/xray.json"
$env:HYDRA_NODE_ROUTE_CREDENTIALS_PATH = ".dev/route-credentials.json"
$env:HYDRA_NODE_ROUTE_CREDENTIALS_DIR = ".dev/route-credentials"
$env:HYDRA_NODE_APPLY_HISTORY_PATH = ".dev/apply-history.json"
$env:HYDRA_NODE_RUNTIME_EVENTS_PATH = ".dev/runtime-events.json"

cargo run -p node-app
```

Use strict validation mode when an Xray binary is available:

```powershell
$env:HYDRA_NODE_XRAY_APPLY_MODE = "external_validate_only"
$env:HYDRA_NODE_XRAY_BINARY_PATH = "<path-to>\xray.exe"
cargo run -p node-app
```

`external_validate_only` runs `xray run -test -config` and reports restart
safety, but intentionally does not start a long-running Xray process.

## Local API

Health is public on the configured local bind:

```powershell
Invoke-RestMethod http://127.0.0.1:18081/health
```

Protected runtime/operator endpoints require `HYDRA_NODE_LOCAL_API_TOKEN`:

```powershell
$headers = @{ "X-Hydra-Local-Token" = "local-dev-token" }
Invoke-RestMethod http://127.0.0.1:18081/state -Headers $headers
Invoke-RestMethod http://127.0.0.1:18081/runtime/artifacts -Headers $headers
Invoke-RestMethod http://127.0.0.1:18081/runtime/validation-report -Headers $headers
```

Do not expose the node local API to the public internet. Bind it to loopback for
development.

## WSL Notes

If running Cargo inside WSL Ubuntu, use Linux paths. The Windows Xray bundle at
`<path-to>\xray.exe` is visible inside WSL as:

```bash
/path/to/xray
```

The real-Xray compatibility test can be run from WSL with:

```bash
cd /home/root1/projects/Hydra-node
HYDRA_TEST_XRAY_BINARY=/path/to/xray cargo test -p node-core real_xray_accepts_generated_production_protocol_documents_when_configured -- --nocapture
```

Required WSL build packages:

```bash
sudo apt-get update
sudo apt-get install -y build-essential pkg-config
```

## Sidecar Integration Preflight

Hysteria2/WireGuard runtime checks are opt-in because normal developer hosts may
not have those tools installed. On a real Linux node, after provisioning sidecar
packages/services, run:

```bash
cd /home/root1/projects/Hydra-node
HYDRA_NODE_HYSTERIA2_BINARY_PATH=/usr/local/bin/hysteria \
HYDRA_NODE_HYSTERIA2_SERVICE_NAME=hydra-hysteria2@hysteria2-in.service \
HYDRA_NODE_WIREGUARD_BINARY_PATH=/usr/bin/wg \
HYDRA_NODE_WG_QUICK_BINARY_PATH=/usr/bin/wg-quick \
sh scripts/integration-sidecars-linux.sh
```

The script probes Hysteria2 version output, optional systemd service metadata,
and WireGuard `wg`/`wg-quick strip` syntax handling without starting VPN
interfaces. Missing optional tools are reported as `SKIP`; configured-but-broken
tools fail closed.

## WireGuard Exact Device Enforcement

WireGuard can use exact device enforcement when every device has its own peer
key. The keypair is generated on the client device; Panel and Node receive only
the public key. Build the adapter and driver:

```bash
cargo build --release -p node-session-adapter -p node-session-driver-wireguard
```

Run `node-app` with the same dedicated adapter token, then start the adapter:

```bash
export HYDRA_NODE_SESSION_ADAPTER_TOKEN='<dedicated-random-adapter-token>'
export HYDRA_NODE_SESSION_ADAPTER_DRY_RUN_OBSERVATION_ONLY=false
export HYDRA_NODE_SESSION_ADAPTER_DRIVER_PATH="$PWD/target/release/node-session-driver-wireguard"
export HYDRA_NODE_WIREGUARD_BINARY_PATH=/usr/bin/wg
export HYDRA_NODE_WIREGUARD_SESSION_REF_KEY='<persistent-random-secret-of-at-least-32-bytes>'
export HYDRA_NODE_WIREGUARD_SESSION_MAP_PATH="$PWD/data/sidecar-generated/wireguard-session-map.json"
cargo run --release -p node-session-adapter
```

Generate the reference key once, store it as a protected service secret, and
reuse it across adapter restarts. Do not generate a new key on every service
start. The production installer must create separate services for `node-app`
and `node-session-adapter`, with the WireGuard driver invoked only by the
adapter. Panel SSH provisioning does this only when WireGuard installation is
explicitly selected; a baseline Linux node does not download the adapter or
WireGuard driver. One subscription client may have multiple device peers, and
Node renders all of them while rejecting duplicate public keys or AllowedIPs.

Xray and Hysteria2 stay fail-closed for exact per-device termination because
their supported management actions disconnect a complete principal rather than
one selected device.

## License

Copyright (C) 2026 Hydra contributors

Hydra is licensed under the **GNU Affero General Public License v3.0 only**
(`AGPL-3.0-only`). The full text is in [LICENSE](LICENSE), and identically at the repository root.

AGPL is a deliberate choice, not a default. Section 13 extends copyleft across the
network: anyone who runs a modified Hydra as a service for others must offer those
users the modified source. A panel is hosted software, so a permissive licence would
let a commercial fork run a closed derivative as a service and give nothing back —
the one scenario worth preventing here.

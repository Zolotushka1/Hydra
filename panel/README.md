# Hydra-Panel

Rust rewrite of the Hydra control plane.

Current goal:

- preserve mandatory Marzban functionality;
- preserve implemented Hydra security and node-management improvements;
- design for low-resource deployments:
  `1 vCPU / 512 MB RAM / 10 GB disk`;
- enforce professional handling of secrets, session state, and operator data from the start.

This repository starts with:

- a Rust workspace for the panel rewrite;
- a minimal `axum` application skeleton;
- architecture and parity documents to constrain the rewrite before feature work starts.

Initial priorities:

1. finalize parity inventory;
2. lock panel architecture and memory/security constraints;
3. implement security/admin foundation first.

Deployment access modes:

- recommended production path: domain + trusted HTTPS;
- supported quick path: IP-only panel access for users without a domain;
- supported hardened quick path: IP + self-signed HTTPS;
- supported advanced path: operator-managed reverse proxy.

See `docs/deployment-access-modes.md`.

Product documentation covering both halves of Hydra — resource budget, security
model, persistence, protocol policy and the panel-node contract — is in
[`../docs/`](../docs/README.md).

## Local Dev / Operator Launch

These commands are the supported pre-installer launch path. They use isolated
smoke data under `.smoke/` and a persistent build cache under `.target/`, so the
first run may compile for several minutes but later runs should be much faster.

### Panel Standalone

Use this when testing the main-server mode: panel and local core on the same
machine, without `Hydra-node`.

```powershell
powershell -ExecutionPolicy Bypass -File scripts/smoke-standalone.ps1
```

This starts `Hydra-Panel` on `127.0.0.1:18080` with isolated smoke data, logs in with the bootstrap admin, checks generated Xray config validation, applies the generated config, and verifies core runtime state/history. It does not require `Hydra-node`.

Useful options:

- `-BindAddr "127.0.0.1:18081"` to use another local port.
- `-DataDir ".smoke/my-panel-run"` to use another isolated runtime data directory.
- `-TargetDir ".target/my-panel-build"` to use another persistent Cargo target cache.
- `-KeepData` to keep `.smoke/panel-standalone` after the run for debugging.

What this proves:

- the panel process starts and answers `/health`;
- bootstrap admin login works;
- generated Xray config passes internal validation;
- standalone `apply-generated` works without a remote node;
- core runtime state and apply history are updated.

### Panel + Remote Node

```powershell
powershell -ExecutionPolicy Bypass -File scripts/smoke-panel-node.ps1
```

This starts `Hydra-Panel`, creates a remote node through the admin API, rotates a one-time node auth token, starts the sibling `Hydra-node` agent, waits for node poll/apply/sync, and verifies that the panel sees the node as synced with local node state available.

By default it uses node `noop` Xray apply mode, so it verifies the panel-node contract but does not prove real Xray validation or restart safety.

Strict real-Xray mode:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/smoke-panel-node.ps1 -XrayBinaryPath "C:\path\to\xray.exe"
```

When `-XrayBinaryPath` is set, the smoke uses node `external_validate_only` mode and requires the panel apply-status to report synced state, local node state, passed external Xray validation, and `safe_to_restart=true`. This validates with the real Xray binary but intentionally does not start a long-running Xray process.

In the current Windows-hosted development flow, use an Xray binary that the started `node-app` process can execute. For Windows PowerShell runs, that normally means a Windows `xray.exe`.

Useful options:

- `-PanelBindAddr "127.0.0.1:18080"` to change the panel smoke port.
- `-NodeBindAddr "127.0.0.1:18081"` to change the node local API port.
- `-NodeRepo "../Hydra-node"` if the sibling node repository is elsewhere.
- `-XrayBinaryPath "C:\path\to\xray.exe"` to require real Xray validation and restart-safety gating.
- `-PanelTargetDir ".target/my-panel-build"` to use another persistent panel Cargo target cache.
- `-NodeTargetDir ".target/my-node-build"` to use another persistent node Cargo target cache.
- `-KeepData` to keep `.smoke/panel-node` after the run for debugging.

What this proves:

- the panel process starts;
- the panel creates a remote node through the admin API;
- a one-time node auth token is rotated;
- the Rust `Hydra-node` agent starts against the panel;
- node poll/apply/sync reaches the panel;
- panel-side apply status sees local node state;
- strict mode additionally proves real Xray validation and restart-safety gating.

### WSL Notes

The verified Windows-hosted development flow runs the smoke scripts from
PowerShell. The repositories live inside WSL, but the scripts build and run
Windows debug binaries through Windows Cargo.

For WSL-native Rust checks, install Rust and build tools inside Ubuntu, then use
Linux paths:

```bash
cd /home/root1/projects/Hydra-node
HYDRA_TEST_XRAY_BINARY=/path/to/xray cargo test -p node-core real_xray_accepts_generated_production_protocol_documents_when_configured -- --nocapture
```

The downloaded Windows Xray binary at `<path-to>\xray.exe` is available
inside WSL as `/path/to/xray`.

## Release Packaging

Release packaging is deliberately OS-specific. Linux release jobs build MUSL
binaries so the same artifact is not tied to a recent distribution glibc;
Windows release jobs produce only Windows executables. Operators never download
artifacts for the other OS.

Linux, on a native x86_64 or aarch64 release runner:

```bash
rustup target add x86_64-unknown-linux-musl
sudo apt-get install musl-tools
./scripts/package-release.sh 0.1.0 https://github.com/Zolotushka1/Hydra-Panel/releases/download/v0.1.0
```

Windows x86_64, from an elevated PowerShell release runner:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/package-release.ps1 `
  -Version 0.1.0 `
  -ReleaseBaseUrl https://github.com/Zolotushka1/Hydra-Panel/releases/download/v0.1.0
```

Each command produces the panel binary, the managed installer executor, the
platform bootstrap script, SHA-256 sidecars, and a typed release-manifest
fragment under `dist/`. Release publication must combine the platform fragments
without changing artifact metadata.

Windows packaging currently exists for contract/release staging only. The
managed executor rejects Windows before any host mutation until service
environment propagation, ACL hardening, and certificate lifecycle are complete.
Do not advertise Windows installation as production-ready yet.

### First-Host Linux Install

`scripts/install.sh` defaults to `first_host`. It asks whether a domain is
available, recommends self-signed HTTPS when only an IP is available, verifies
both the executor and panel binary SHA-256, builds the same typed install session
locally, asks for an explicit `YES`, and only then mutates the host. It does not
call or expose an unauthenticated panel bootstrap endpoint.

After downloading the release script and its checksum from a pinned HTTPS
release, verify it before execution:

```bash
sha256sum --check install-linux-x86_64.sh.sha256
sudo ./install-linux-x86_64.sh
```

For a non-mutating contract check:

```bash
sudo env \
  HYDRA_INSTALLER_MODE=first_host \
  HYDRA_INSTALLER_DRY_RUN=1 \
  HYDRA_INSTALLER_ACCESS_MODE=ip_self_signed_tls \
  HYDRA_INSTALLER_PUBLIC_IP=203.0.113.10 \
  HYDRA_INSTALLER_PANEL_BINARY_URL=https://example.test/hydra-panel-linux-x86_64 \
  HYDRA_INSTALLER_PANEL_BINARY_SHA256=<64-hex-sha256> \
  ./install-linux-x86_64.sh
```

`domain_tls` installs the allowlisted `certbot` package through `apt-get`,
`dnf`, or `yum` when needed. DNS must already point to the server and TCP port
80 must be reachable for the standalone ACME challenge. Certificate issuance
fails closed; it never silently falls back to HTTP.

Set `HYDRA_INSTALLER_MODE=managed` to use an existing panel job id and
one-time executor token for managed install/reinstall.

## License

Copyright (C) 2026 Hydra contributors

Hydra is licensed under the **GNU Affero General Public License v3.0 only**
(`AGPL-3.0-only`). The full text is in [LICENSE](LICENSE), and identically at the repository root.

AGPL is a deliberate choice, not a default. Section 13 extends copyleft across the
network: anyone who runs a modified Hydra as a service for others must offer those
users the modified source. A panel is hosted software, so a permissive licence would
let a commercial fork run a closed derivative as a service and give nothing back —
the one scenario worth preventing here.

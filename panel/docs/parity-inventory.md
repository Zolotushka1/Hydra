# Parity Inventory

This document is the first parity baseline for the Rust rewrite.

It will be refined into a full matrix, but the rewrite should already treat these as mandatory tracks.

## Marzban Baseline To Preserve

- admin management
- user management
- usage and reset flows
- inbounds / proxies / hosts
- subscription delivery
- Xray config management
- node registration
- node communication
- system actions and logs

## Hydra Additions Already Considered Mandatory

- login protection
- smart ban
- login / ban history
- blocked IP export
- settings tabs
- optional 2FA
- optional 2FA 2-step login
- immediate persistence for security toggles
- config-only save flow
- unsaved-config warning
- disk usage monitoring
- disk-full handling
- SSH node auto-provisioning baseline

## Future Mandatory Platform Tracks

- trusted proxy CIDR management
- active ban management
- retry / reprovision for nodes
- monitoring thresholds and alerts
- Telegram operational integration
- subscription/device/session control
- Leptos CSR frontend replacement
- glass-style operator UI
- cluster orchestration with entry/relay/exit topology
- domain-less panel access modes for operators without a domain
- automatic Let's Encrypt / ACME certificate lifecycle
- scan defense with invalid-handshake detection and firewall backend abstraction
- ready-made routing presets with preview/diff/apply flow

## New Product Differentiators

These are not Marzban parity items. They are intended to make the Rust product better than the existing fork family.

### Cluster Orchestration

Target:

- visual editor for multi-hop server paths
- entry nodes, relay nodes, and exit nodes
- revisioned cluster apply
- health and drift visibility per hop
- remediation when a hop fails

Primary risks:

- accidental open relay behavior
- route complexity that is hard to debug
- unbounded health checks or telemetry
- unclear failure behavior during partial cluster apply

Memory notes:

- cluster graph state must be bounded and paged
- path health probes need concurrency limits
- UI graph should use summarized state instead of raw event streams

### Certificate Automation

Target:

- optional Let's Encrypt certificates
- renewal scheduling
- domain/cert status in UI
- cert deployment to affected nodes

Primary risks:

- private key leakage
- renewal failure close to expiry
- implicit changes to runtime config without operator visibility

### Panel Access Modes

Target:

- guided installer question: domain or no domain
- recommended `domain_tls` mode with trusted HTTPS
- quick `ip_http` mode for users without a domain
- hardened `ip_self_signed_tls` mode with fingerprint display
- advanced `reverse_proxy` mode with explicit trusted proxy CIDRs
- visible security posture in installer and UI

Primary risks:

- presenting plaintext IP HTTP as equally secure
- trusting `X-Forwarded-For` without trusted proxy ranges
- leaking certificate private keys through status APIs or logs
- confusing browser self-signed certificate warnings with installation failure

### Scan Defense

Target:

- detect repeated invalid handshakes
- block abusive IPs through firewall backend where available
- expose scan-defense events and active blocks in UI

Primary risks:

- false positives
- unsafe firewall mutations
- unbounded log parsing

### Routing Presets

Target:

- built-in routing presets
- custom operator presets
- preview diff before apply
- auditable route policy changes

Primary risks:

- incorrect route assumptions per region/operator
- difficult rollback if presets directly mutate config without revisioning

## Inventory Work Still Required

The next documentation pass should expand this file into:

- endpoint/API parity
- DB/domain parity
- operator workflow parity
- migration/import requirements
- memory-risk notes per feature family
- data-sensitivity notes per feature family

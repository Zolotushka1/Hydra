# Hydra documentation

Hydra is a VPN control plane in two parts: a panel that owns configuration,
users and orchestration, and a node agent that applies what the panel renders.
Both are Rust workspaces in this repository.

This directory describes the product as a whole — the constraints it is built
against, the security and persistence model, the protocol policy, and the
contract between the two halves. Documentation specific to one workspace lives
beside it.

## Product documentation

| Page | Subject |
| --- | --- |
| [Repository layout](repository-layout.md) | how the two workspaces are arranged and why |
| [Resource budget](resource-budget.md) | the 1 vCPU / 512 MB / 10 GB baseline and what follows from it |
| [Deployment](deployment.md) | supported operating systems and panel access modes |
| [Security model](security-model.md) | what is encrypted, under which keys, and the secret inventory |
| [Persistence](persistence.md) | on-disk files, environment variables, durability and permissions |
| [Protocols](protocols.md) | supported protocols, transports and deployment scenarios |
| [Panel-node contract](panel-node-contract.md) | runtime config, apply plans, route credentials, least knowledge |
| [Relay privacy](relay-privacy.md) | what a relay node is allowed to know |
| [Schema versioning](schema-versioning.md) | how document versions change and what consumers must do |
| [Testing policy](testing-policy.md) | how validation rules are tested, and why a green local run is not evidence |

## Workspace documentation

| Page | Subject |
| --- | --- |
| [`panel/docs/api.md`](../panel/docs/api.md) | HTTP API reference and the schema registry table |
| [`panel/docs/architecture.md`](../panel/docs/architecture.md) | panel-internal architecture notes |
| [`panel/docs/deployment-access-modes.md`](../panel/docs/deployment-access-modes.md) | access mode reference |
| [`panel/docs/load-memory-validation.md`](../panel/docs/load-memory-validation.md) | load and memory acceptance procedure |
| [`node/docs/protocol.md`](../node/docs/protocol.md) | agent-side protocol, local debug surface and runtime model |

## Licence

Copyright (C) 2026 Hydra contributors

Hydra is distributed under `AGPL-3.0-only`. The licence text is at the
repository root and repeated identically in each workspace root, because a crate
packaged on its own must carry the licence beside its manifest. CI compares the
three copies by hash, since a requirement to keep files identical does not
survive on its own.

The choice of AGPL over a permissive licence is deliberate. Section 13 extends
copyleft across the network: running a modified Hydra as a service for others
obliges the operator to offer those users the modified source. A control panel is
hosted software, so under a permissive licence a commercial fork could operate a
closed derivative as a service and return nothing. That is the one scenario AGPL
prevents and MIT does not.

It also settles provenance. Upstream Marzban, which this project treats as a
functional parity baseline and from which the node agent was originally forked,
is itself AGPL; matching the licence removes any question about whether copyleft
obligations were carried across.

There are no per-file licence headers. AGPL does not require them, and forty of
them would drift out of sync with the manifests.

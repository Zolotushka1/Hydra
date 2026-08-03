# Relay privacy

A cluster is a graph of nodes forwarding traffic to an exit. The panel knows the
whole graph. A relay node does not, and that is the point.

Hydra has one production cluster operating mode: **least-knowledge route
assignments**. Relay nodes follow it by default and by design, not as a hardening
option an operator may switch on.

## What a node receives

The node payload is a node-specific projection of the graph, not a filtered copy
of it. A relay receives:

- its `NodeRouteAssignment` entries
- the local route id
- the local role
- an optional local listen definition
- an optional previous peer
- an optional next peer

Enough to forward its own leg, and nothing else.

## What a node does not receive

- the full cluster graph
- all upstream and downstream nodes
- route edge lists that reveal topology
- the exit topology
- users, subscriptions or subscription tokens
- the node inventory

A compromised relay therefore discloses one hop, not the deployment.

## Peer identity and hop security

Route assignment `security` and `auth` metadata are part of the production
contract. `auth.identity_ref` is the opaque peer identity used for VLESS hop
client identifiers — an opaque reference rather than a user-bearing value.
Outbound hop security uses local mTLS material, and the outbound hop's auth
identity targets the next peer.

A required secure hop fails closed until the node holds renderable mTLS or
Reality material. It is never downgraded to plaintext forwarding.

`credential_ref` fields are references only. They never contain private keys or
raw secret material; the material is resolved locally through
`HYDRA_NODE_ROUTE_CREDENTIALS_PATH`, with the manifest defaulting to
`data/route-credentials.json` and the material directory to
`data/route-credentials`.

## Cluster targets

A graph-shaped `cluster_targets` document still exists as a development bridge
while panel-side route assignment generation is completed. It is not a selectable
production cluster mode, and production apply and rendering use
`node_route_assignments`. `NodeRuntimeConfigDocument` already carries
`route_assignments`, and the renderer prefers them over graph-like cluster
targets. Once panel-side generation is complete, graph-like cluster target usage
is removed from runtime decisions.

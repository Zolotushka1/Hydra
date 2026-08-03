# Security model

## Invariants

Passwords are never stored in plaintext: the bootstrap admin password is
persisted as an Argon2 hash. Node authentication tokens and provisioning executor
tokens are persisted only as SHA-256 hashes, and executor tokens are never
exposed through registry or list APIs — a rotated executor token is returned once
and never again.

Secrets, tokens and private keys are never written to logs or to the audit trail.
Proxy-trust logic and authentication headers are treated as high-risk code, and
`X-Forwarded-For` is deny-by-default.

Every event and state history is bounded, because an unbounded history is both a
memory failure and a disclosure surface.

## Secret inventory

| Material | Storage |
| --- | --- |
| bootstrap admin password | Argon2 hash |
| panel-wide admin 2FA secret | encrypted, AES-256-GCM |
| node authentication token | SHA-256 hash only |
| provisioning executor token | SHA-256 hash only; returned once on rotation |
| node local API token | write-only API field; encrypted when configured |
| Telegram bot token | encrypted; never returned by the public settings response |
| route material private keys | encrypted, AES-256-GCM |
| Reality private keys | encrypted, AES-256-GCM, under their own master key |

Each encrypted class has its own master key, so compromising one does not unlock
the others:

| Environment variable | Key file variable | Default path |
| --- | --- | --- |
| `HYDRA_ADMIN_SECRETS_MASTER_KEY_B64` | `HYDRA_ADMIN_SECRETS_KEY_PATH` | `data/admin-secrets.key` |
| `HYDRA_NODE_SECRETS_MASTER_KEY_B64` | `HYDRA_NODE_SECRETS_KEY_PATH` | `data/node-secrets.key` |
| `HYDRA_TELEGRAM_SECRETS_MASTER_KEY_B64` | `HYDRA_TELEGRAM_SECRETS_KEY_PATH` | `data/telegram-secrets.key` |
| `HYDRA_ROUTE_MATERIALS_MASTER_KEY_B64` | `HYDRA_ROUTE_MATERIALS_KEY_PATH` | `data/route-materials.key` |

Losing a master key makes the corresponding encrypted material unrecoverable.
There is no escrow and no recovery path; this is a property of the design, not an
oversight. `GET /api/system/secret-readiness` reports non-secret diagnostics for
the readiness of each master key, so an operator can see that a key is present
without the key being disclosed.

## Reality material

Reality key material is issued per inbound: an x25519 pair plus a short id, held
in `reality_material_store` under its own master key at mode `0600`. The public
half is derived on read rather than stored, so the pair cannot drift apart.

Private keys reach nodes through `/api/node-agent/route-credentials`, a
`node_agent`-exposure route. The key therefore never touches the admin surface
and cannot leak through `/api/core/xray-config` or a golden snapshot. This
material does not belong in `node_runtime_config`, which is also rendered for
administrators.

The node substitutes the private key at render time, the same way it substitutes
certificate paths. `dest` and `serverNames` are derived from the host SNI and are
never configured separately: Reality masquerades as the site whose name it
presents, so a second source of truth for that pair would show up as a mismatch
on the first active probe.

A Reality host missing an SNI, a public key or a short id is refused at link
issuance. A Reality host is never silently downgraded to plain TLS — a link
issued without `pbk` and `sid` does not connect, and the failure surfaces at the
user rather than at the operator.

A host that would serve more than one Reality inbound is refused fail-closed.
Material is per inbound while the public half is stored on the host, so with more
than one inbound there is no single correct answer; picking one hands clients
another inbound's keys.

## Client fingerprint

Subscription links carry `fp=chrome` for both `tls` and `reality`. Without a uTLS
fingerprint the client presents Go's `crypto/tls`, whose JA3/JA4 matches no
browser and is classified automatically. The identifier is spelled
`TLS_FINGERPRINT` in code, because `fingerprint` already means subscription
device fingerprint in the panel and the two must not be conflated.

## Trust boundaries

Every route carries an `exposure` that names the credential it enforces:

| Exposure | Credential |
| --- | --- |
| `admin_ui` | admin session |
| `node_agent` | node token in `x-hydra-node-token` |
| `executor` | provisioning or installer executor token, in a header or the request body |
| `public` | none |
| `debug` | reserved; must not exist in a production build |

`exposure` reflects the authentication a handler actually enforces, never the
path prefix. `POST /api/installer/jobs/result` sits in the `installer` group but
authenticates an executor token carried in the request body; labelling it by
prefix would have published an executor endpoint to the browser. The functional
group a route belongs to is a separate, independent field.

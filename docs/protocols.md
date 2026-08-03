# Protocols, transports and deployment scenarios

## Supported protocols

Hydra supports three protocols: **VLESS**, **Hysteria2** and **WireGuard**.
VMess, Trojan and Shadowsocks were removed together with their classification
aliases, render arms and constants.

The reasoning is per protocol:

- **VMess** has no masquerade of its own and a recognizable TLS handshake
  fingerprint.
- **Trojan** is passively detectable as TLS-in-TLS above 70% (USENIX Security
  2024), while its deployment bar is *higher* than Reality's: it needs a domain,
  a certificate and a decoy site for fallback. A worse result for more work.
- **Shadowsocks** never rendered a valid config. AEAD-2022 requires an
  inbound-level pre-shared key and base64 user keys, neither of which was
  modelled, so `xray -test` refused the output with
  `proxy/shadowsocks_2022: missing key`. What was removed had never worked.

Removing an enum variant is a breaking change and costs a version bump; adding
one is not. That asymmetry is why all three went in a single pass. Bringing
Shadowsocks back for transit legs later is additive and cheap — its low CPU cost
is a real argument at 1 vCPU — but it needs the pre-shared key work first.

Credential derivation has one protocol per branch: VLESS derives a UUID,
Hysteria2 derives a password. The branches are kept as match arms rather than
collapsed into direct checks, because they widen again naturally if a protocol
returns.

### Structural guard

The real-Xray test derives its protocol list from `classify_runtime_protocol`
rather than hardcoding it, and asserts that the derived set matches the fixture.
A protocol that declares itself Xray-backed but is never checked against the
binary is precisely what let the Shadowsocks defect survive a green test run.

## Transports

`InboundTransport` covers `tcp`, `udp`, `ws`, `grpc`, `httpupgrade`, `quic` and
`xhttp`. `XhttpMode` is one of `auto`, `packet-up`, `stream-up`, `stream-one`.

XHTTP splits upstream and downstream into separate HTTP transactions, so the
connection profile after the handshake stops looking like a tunnel. Reality
covers the handshake; XHTTP covers what follows. They are complementary rather
than alternatives, and XHTTP padding (`extra.xPaddingBytes`) normalizes packet
sizes so lengths do not give the tunnel away even behind a disguised handshake.

**Vision is a `flow` flag, not a transport.** It lives on plain TCP and inside
XHTTP `stream-one` alike; the axis of choice is `tcp` against `xhttp`. Modelling
Vision as a transport makes the three deployment scenarios below inexpressible.

## Validation is the panel's job

`xray run -test` accepts every forbidden combination — verified on 26.6.27, where
`packet-up` with `flow: xtls-rprx-vision` returns `Configuration OK` and does not
work. These are semantic rules rather than parsing ones, so the binary does not
check them and will not. The panel enforces six:

| Rule | Source |
| --- | --- |
| Vision requires `stream-one` | `mode` description in the XHTTP transport documentation: `stream-one` keeps one connection per request-response, so only it can splice |
| Reality cannot sit behind a CDN, on any transport | the CDN terminates TLS, so there is no handshake left to substitute |
| behind a CDN the transport must be XHTTP with `packet-up` | `packet-up` uses a separate POST per packet and needs no long-lived stream support |
| behind a CDN Vision is impossible | a consequence of the two rules above, reported separately because operators arrive with this pair |
| `xhttp_mode` on a non-XHTTP transport is an error | not a silently ignored field |
| `auto` with Reality is accepted with an `info` issue naming the resolved mode | Xray resolves `auto` to `stream-one` under Reality |

## Declaring a CDN

`Host.behind_cdn` says that the address clients connect to is served through a
CDN. It sits on the host rather than on the inbound because a CDN stands in front
of a connection address, and that is what a host is. The field is optional and
defaults to `false`, so a host that never declared it keeps its meaning and the
document shape is unchanged — it does not raise the schema version.

One inbound can be published through several hosts, and only some of them may be
fronted. **A single fronted host is enough**: the inbound has to work over that
path too, so it must be XHTTP with `packet-up`, and Reality is refused on it. The
conflict resolves towards the restriction rather than towards whichever host
happens to be first in the list.

Which hosts serve which inbound is decided by the same node-and-cluster predicate
that scopes Reality material, so the two cannot disagree about the binding.

Every rule carries a source citation in the code. The requirement is not a
formality: a hand-written rule without one already asserted that Shadowsocks
client entries must carry a specific method, the exact opposite of what Xray
requires, and it survived until the renderer was first run against the real
binary.

`auto` is emitted resolved rather than literally. Xray turns it into `stream-one`
under Reality, and publishing `auto` would leave the operator guessing which mode
is actually in effect.

## Deployment scenarios

`deployment_scenarios` sits on top of the capability matrix and never beside it.
Each scenario points at a production-ready capability row; two tests enforce that,
one checking the matrix reference and one running each scenario through the
panel's own XHTTP validation. A scenario carrying a combination the matrix does
not have is how a parallel second model starts.

The layer paid for itself on introduction: it exposed that `tcp + reality`, the
most common Reality deployment, was missing from the VLESS rows.

## Xray version is a working condition

`GET /api/ui/protocols` reports `xray_version` read from the panel's own binary.
XHTTP is under active development and server and client versions must match; a
mismatch fails with no useful error. The version is read per request on purpose:
the Xray update flow swaps the binary without restarting the panel, and a cached
value in a field that exists to detect mismatches is worse than no value at all.

CI pins one Xray version for both workspaces and runs the real-Xray test with
`HYDRA_REQUIRE_XRAY_TEST=1`, which forbids the check from skipping silently. That
silent skip is what let a removed `allowInsecure` flag and a missing Reality
renderer survive green runs.

## Never emit unset fields to Xray

Xray distinguishes an absent key from a key carrying a value. The removed
`allowInsecure` is rejected by its **presence**, not by what it holds — a field
serialized with its default instead of being omitted is what broke the renderer.

The rule generalizes, and needs two mechanics because the config is built two
ways:

- structs serialized whole carry
  `#[serde(default, skip_serializing_if = "Option::is_none")]` on every `Option`;
- the raw config is assembled by hand with `serde_json::json!`, where that
  attribute does not apply. Those sites use `insert_if_some`, which writes a key
  only when a value exists. `json!` with an `Option` writes `null`, which is
  exactly the failure mode.

A test walks the rendered config and fails on any `null`.

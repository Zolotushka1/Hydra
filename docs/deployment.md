# Deployment

## Supported targets

The panel and the agent are supported on:

- CentOS 7, 8, 9
- Debian 10, 11, 12
- Ubuntu 18.04, 20.04, 22.04, 24.04
- AlmaLinux 8, 9
- Astra Linux
- Windows 10, 11
- Windows Server 2019, 2022

Windows is a first-class target, not an afterthought. Platform differences are
modelled explicitly rather than hidden behind Linux-only assumptions: service
managers, package managers, firewall backends, filesystem permissions and
Windows service and runtime behaviour each have their own representation.

## Installation

Installation is a one-line operator flow, comparable in convenience to tools such
as `3x-ui`, and first-run setup is guided. The installer asks explicitly whether
the operator has a domain, and adding one later is part of the normal operator
workflow rather than a reinstall. Certificate issuance and renewal are available
as a normal operator path, not only as a manual expert flow.

## Panel access modes

Not every operator can buy or delegate a domain, so domain-less panel access is a
supported product path rather than a degraded corner. Four modes exist:

| Mode | Description |
| --- | --- |
| `domain_tls` | domain with a trusted certificate; the recommended mode |
| `reverse_proxy` | the panel behind an operator-managed reverse proxy |
| `ip_self_signed_tls` | IP address with a self-signed certificate |
| `ip_http` | IP address without TLS |

IP-only modes use stricter defaults and carry visible risk labels. They are not
presented as equivalent to a domain with trusted TLS, because they are not — but
they are supported, tested and documented rather than left as an undocumented
workaround.

The full reference, including what each mode changes in panel behaviour, is in
[`panel/docs/deployment-access-modes.md`](../panel/docs/deployment-access-modes.md).

## Node artifacts

The node install step downloads the agent binary from a release artifact URL
supplied by the provisioning executor, so the panel never hardcodes a release
URL. Because the panel therefore never sees the value, the install script itself
pins all three node artifact URLs to
`https://github.com/Zolotushka1/Hydra/releases/download/node-v*`, and the check
runs before the download rather than after it. A URL pointing anywhere else
aborts the step instead of installing an unrelated binary under the node's
systemd unit.

Releases use two independent tag families, `panel-vX.Y.Z` and `node-vX.Y.Z`,
because the two halves ship on their own cadence. A release body carries the
schema versions of the documents that cross the panel-node boundary, so an
operator upgrading one half can tell in advance whether the other is recent
enough. The runtime compatibility check is fail-closed, but it only fires after
installation.

## Packaging

Each workspace owns its packaging in `scripts/package-release.{sh,ps1}`, and the
release workflow calls those scripts rather than assembling artifacts itself.
What a release contains and what its artifacts are called is decided in one
place; a second copy inside CI would be the one nobody runs by hand, and
therefore the one that drifts.

| Artifact | Contents |
| --- | --- |
| `hydra-panel-linux-<arch>` | panel binary |
| `hydra-panel-web-linux-<arch>.tar.gz` | frontend bundle, unpacks to `web/` beside the binary |
| `panel-installer-executor-linux-<arch>` | installer executor |
| `install-linux-<arch>.sh` | first-host installer script |
| `hydra-node-linux-<arch>` | node agent; this is what `HYDRA_NODE_ARTIFACT_URL` points at |
| `node-session-adapter-linux-<arch>` | session adapter, WireGuard deployments only |
| `node-session-driver-wireguard-linux-<arch>` | WireGuard exact-session driver |

Every artifact carries a `.sha256` sidecar, and each package writes a typed
release manifest fragment.

The node packaging script refuses a release base URL outside
`https://github.com/Zolotushka1/Hydra/releases/download/node-v*`, because that is
the only path the panel will download from. A package built for anywhere else
cannot be installed, so it fails at packaging time rather than at provisioning
time.

Both packages are checked as artifacts rather than as code. Each workspace has a
`scripts/verify-release-package.sh` that builds a package, deploys it to a
directory unrelated to the source tree and starts it there: the panel must serve
`/dashboard` and the asset that page references, and the agent must answer
`/health` with no panel reachable. CI runs both. A test suite cannot see either
property, because neither is a property of the code.

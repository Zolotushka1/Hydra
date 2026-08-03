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

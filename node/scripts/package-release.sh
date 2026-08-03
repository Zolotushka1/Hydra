#!/bin/sh
# Packages the node agent for release.
#
# Artifact names are part of the contract, not a convention: the panel's install
# step downloads whatever `HYDRA_NODE_ARTIFACT_URL` points at and installs it as
# `/opt/hydra-node/hydra-node`, and the URL is pinned to the project's own
# `node-v*` release path. Renaming an artifact here breaks provisioning, so the
# names match what the release workflow publishes and nothing else.
set -eu

fail() {
  printf 'hydra node release packaging: %s\n' "$1" >&2
  exit 1
}

command -v cargo >/dev/null 2>&1 || fail "cargo is required"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"

version="${1:-${HYDRA_RELEASE_VERSION:-}}"
release_base="${2:-${HYDRA_RELEASE_BASE_URL:-}}"
[ -n "$version" ] || fail "usage: package-release.sh VERSION HTTPS_RELEASE_BASE_URL"
[ -n "$release_base" ] || fail "release base URL is required"
case "$version" in *[!0-9A-Za-z._+-]*|'') fail "version contains unsafe characters" ;; esac
case "$release_base" in https://*) ;; *) fail "release base URL must use HTTPS" ;; esac
release_base="${release_base%/}"

# The agent is installed by a panel that refuses any URL outside the project's
# own node release path, so a package published anywhere else cannot be
# installed. Checked here rather than only in the workflow: a hand-run packaging
# that produces an uninstallable artifact should say so.
case "$release_base" in
  https://github.com/Zolotushka1/Hydra/releases/download/node-v*) ;;
  *)
    fail "release base URL must be https://github.com/Zolotushka1/Hydra/releases/download/node-v*, or the panel will refuse the artifact"
    ;;
esac

case "$(uname -m)" in
  x86_64|amd64)
    arch="x86_64"
    target="${HYDRA_RELEASE_TARGET:-x86_64-unknown-linux-musl}"
    ;;
  aarch64|arm64)
    arch="aarch64"
    target="${HYDRA_RELEASE_TARGET:-aarch64-unknown-linux-musl}"
    ;;
  *) fail "unsupported CPU architecture" ;;
esac

dist="${HYDRA_RELEASE_DIST_DIR:-dist/linux-$arch}"
case "$dist" in
  dist/*) ;;
  *) fail "release output must stay under the repository dist directory" ;;
esac
case "$dist" in *..*) fail "release output must not contain parent traversal" ;; esac
rm -rf "$dist"
mkdir -p "$dist"

cargo build --locked --release --target "$target" \
  -p node-app -p node-session-adapter -p node-session-driver-wireguard

node_name="hydra-node-linux-$arch"
adapter_name="node-session-adapter-linux-$arch"
driver_name="node-session-driver-wireguard-linux-$arch"

install -m 0755 "target/$target/release/node-app" "$dist/$node_name"
install -m 0755 "target/$target/release/node-session-adapter" "$dist/$adapter_name"
install -m 0755 "target/$target/release/node-session-driver-wireguard" "$dist/$driver_name"

for artifact in "$node_name" "$adapter_name" "$driver_name"; do
  (
    cd "$dist"
    sha256sum "$artifact" > "$artifact.sha256"
  )
done

node_sha="$(sha256sum "$dist/$node_name" | awk '{print $1}')"
adapter_sha="$(sha256sum "$dist/$adapter_name" | awk '{print $1}')"
driver_sha="$(sha256sum "$dist/$driver_name" | awk '{print $1}')"

cat > "$dist/release-manifest-linux-$arch.json" <<EOF
{
  "manifest_version": 1,
  "artifacts": [
    {
      "name": "$node_name",
      "artifact_kind": "node_binary",
      "target_os": "linux",
      "target_arch": "$arch",
      "package_channel": "stable",
      "version": "$version",
      "url": "$release_base/$node_name",
      "sha256": "$node_sha"
    },
    {
      "name": "$adapter_name",
      "artifact_kind": "node_session_adapter_binary",
      "target_os": "linux",
      "target_arch": "$arch",
      "package_channel": "stable",
      "version": "$version",
      "url": "$release_base/$adapter_name",
      "sha256": "$adapter_sha"
    },
    {
      "name": "$driver_name",
      "artifact_kind": "node_session_driver_wireguard_binary",
      "target_os": "linux",
      "target_arch": "$arch",
      "package_channel": "stable",
      "version": "$version",
      "url": "$release_base/$driver_name",
      "sha256": "$driver_sha"
    }
  ]
}
EOF

printf 'Packaged Linux %s node release artifacts in %s\n' "$arch" "$dist"

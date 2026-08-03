#!/bin/sh
set -eu

fail() {
  printf 'hydra release packaging: %s\n' "$1" >&2
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
  -p panel-app -p panel-installer-executor

panel_name="hydra-panel-linux-$arch"
executor_name="panel-installer-executor-linux-$arch"
installer_name="install-linux-$arch.sh"

install -m 0755 "target/$target/release/panel-app" "$dist/$panel_name"
install -m 0755 "target/$target/release/panel-installer-executor" "$dist/$executor_name"
install -m 0755 scripts/install.sh "$dist/$installer_name"

# The frontend is read from disk at run time, not embedded, so the bundle has to
# travel with the binary. `web/` beside the executable is exactly where the panel
# looks by default. Without this the operator gets a panel that serves the API
# and an empty dashboard.
[ -f web/dist/index.html ] || fail "web/dist is not built; run 'npm ci && npm run build' in web/"
rm -rf "$dist/web"
mkdir -p "$dist/web"
cp -R web/dist/. "$dist/web/"
find "$dist/web" -type d -exec chmod 0755 {} +
find "$dist/web" -type f -exec chmod 0644 {} +

for artifact in "$panel_name" "$executor_name" "$installer_name"; do
  (
    cd "$dist"
    sha256sum "$artifact" > "$artifact.sha256"
  )
done

panel_sha="$(sha256sum "$dist/$panel_name" | awk '{print $1}')"
installer_sha="$(sha256sum "$dist/$installer_name" | awk '{print $1}')"

cat > "$dist/release-manifest-linux-$arch.json" <<EOF
{
  "manifest_version": 1,
  "artifacts": [
    {
      "name": "$installer_name",
      "artifact_kind": "installer_script",
      "target_os": "linux",
      "target_arch": "$arch",
      "package_channel": "stable",
      "version": "$version",
      "url": "$release_base/$installer_name",
      "sha256": "$installer_sha"
    },
    {
      "name": "$panel_name",
      "artifact_kind": "panel_binary",
      "target_os": "linux",
      "target_arch": "$arch",
      "package_channel": "stable",
      "version": "$version",
      "url": "$release_base/$panel_name",
      "sha256": "$panel_sha"
    }
  ]
}
EOF

printf 'Packaged Linux %s release artifacts in %s\n' "$arch" "$dist"

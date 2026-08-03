#!/bin/sh
# Proves that a packaged panel serves the real frontend, not a placeholder.
#
# The dashboard is read from disk at run time. That makes two things breakable
# without any test noticing: the packaging scripts can omit the bundle, and the
# panel can look for it in a directory that only exists on the build host. Both
# happened. A unit test cannot catch either, because both are properties of the
# artifact rather than of the code, so the check builds a package, deploys it to
# a directory unrelated to the source tree, starts the binary and asks it for the
# page.
set -eu

fail() {
  printf 'release package verification: %s\n' "$1" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
port="${HYDRA_VERIFY_PORT:-18099}"
panel_pid=""

cleanup() {
  [ -n "$panel_pid" ] && kill "$panel_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

[ -f web/dist/index.html ] || fail "web/dist is not built; run 'npm ci && npm run build' in web/"

# A gnu target keeps the check about packaging layout rather than about having a
# musl toolchain installed; the script itself defaults to musl for real releases.
HYDRA_RELEASE_DIST_DIR="dist/verify" \
HYDRA_RELEASE_TARGET="${HYDRA_RELEASE_TARGET:-x86_64-unknown-linux-gnu}" \
  sh scripts/package-release.sh 0.0.0-verify https://example.test/verify >/dev/null

deployed="$work/opt/hydra"
mkdir -p "$deployed"
cp -R dist/verify/. "$deployed/"
rm -rf dist/verify

panel="$deployed/hydra-panel-linux-x86_64"
[ -x "$panel" ] || fail "packaged panel binary is missing or not executable"
[ -f "$deployed/web/index.html" ] || fail "the package does not carry web/index.html"

# Started from the deployment directory with no access to the source tree: if the
# binary still resolves the bundle through a build-time path, it cannot find it
# here, which is the failure this check exists for.
cd "$deployed"
HYDRA_BIND_ADDR="127.0.0.1:$port" ./hydra-panel-linux-x86_64 >"$work/panel.log" 2>&1 &
panel_pid=$!
cd "$repo_root"

ready=""
i=0
while [ "$i" -lt 60 ]; do
  if curl -fsS "http://127.0.0.1:$port/health" >/dev/null 2>&1; then
    ready=1
    break
  fi
  kill -0 "$panel_pid" 2>/dev/null || fail "panel exited early:$(printf '\n')$(cat "$work/panel.log")"
  i=$((i + 1))
  sleep 1
done
[ -n "$ready" ] || fail "panel did not answer /health:$(printf '\n')$(cat "$work/panel.log")"

dashboard="$work/dashboard.html"
curl -fsS "http://127.0.0.1:$port/dashboard" -o "$dashboard" || fail "/dashboard did not answer"

if grep -q 'The frontend bundle was not found' "$dashboard"; then
  fail "/dashboard served the placeholder, so the package has no usable frontend"
fi
grep -q 'id="root"' "$dashboard" || fail "/dashboard did not serve the built bundle"

# The entry script has a content hash in its name, so it can only be read out of
# the served page. Fetching it proves the asset directory resolves too — that was
# broken independently of index.html.
asset="$(sed -n 's/.*src="\(\/assets\/[^"]*\.js\)".*/\1/p' "$dashboard" | head -1)"
[ -n "$asset" ] || fail "the served page references no bundled script"
curl -fsS "http://127.0.0.1:$port$asset" -o /dev/null \
  || fail "the bundled asset $asset is not served from the package"

printf 'Release package serves the built frontend: %s and %s\n' "/dashboard" "$asset"

#!/bin/sh
# Proves that a packaged node agent runs where it is installed.
#
# The panel downloads one artifact from a release and runs it as a systemd unit
# on a machine that has no source tree and no cargo. Nothing in the test suite
# covers that: the tests run the crate, not the artifact. This builds a package,
# deploys it to a directory unrelated to the source tree, starts the binary and
# waits for its local API.
#
# The panel is deliberately not running. The agent must come up and answer
# /health regardless — an agent that only starts when its panel is reachable
# would be undeployable, since provisioning installs the agent first.
set -eu

fail() {
  printf 'node release package verification: %s\n' "$1" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

work="$(mktemp -d)"
port="${HYDRA_VERIFY_PORT:-18095}"
node_pid=""

cleanup() {
  [ -n "$node_pid" ] && kill "$node_pid" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT INT TERM

# A gnu target keeps the check about the artifact rather than about having a musl
# toolchain installed; the script itself defaults to musl for real releases.
HYDRA_RELEASE_DIST_DIR="dist/verify" \
HYDRA_RELEASE_TARGET="${HYDRA_RELEASE_TARGET:-x86_64-unknown-linux-gnu}" \
  sh scripts/package-release.sh 0.0.0-verify \
    https://github.com/Zolotushka1/Hydra/releases/download/node-v0.0.0-verify >/dev/null

deployed="$work/opt/hydra-node"
mkdir -p "$deployed"
cp -R dist/verify/. "$deployed/"
rm -rf dist/verify

case "$(uname -m)" in
  x86_64|amd64) arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *) fail "unsupported CPU architecture" ;;
esac

# The name the panel install step downloads and installs. If it changes here
# without changing there, provisioning fetches a file that does not exist.
agent_name="hydra-node-linux-$arch"
[ -x "$deployed/$agent_name" ] || fail "the package has no executable $agent_name"
for extra in "node-session-adapter-linux-$arch" "node-session-driver-wireguard-linux-$arch"; do
  [ -x "$deployed/$extra" ] || fail "the package is missing $extra"
done

# Checksums travel with the artifacts and must describe them, since the panel
# provisioning flow verifies downloads against the release manifest.
( cd "$deployed" && sha256sum -c "$agent_name.sha256" >/dev/null ) \
  || fail "the packaged checksum does not match the packaged binary"

# Started from the deployment directory, with the panel absent and state written
# under the deployment root rather than the source tree.
#
# A token is required: the agent refuses to start without one, which is why
# provisioning writes node.env before enabling the unit. The value is irrelevant
# here because no panel answers, but its absence would stop the agent for a
# reason unrelated to what this check is about.
cd "$deployed"
HYDRA_NODE_TOKEN="verification-token-not-a-credential" \
HYDRA_NODE_LOCAL_API_BIND="127.0.0.1:$port" \
HYDRA_PANEL_URL="http://127.0.0.1:1" \
HYDRA_NODE_POLL_INTERVAL_SECONDS="3600" \
HYDRA_NODE_STATE_PATH="$work/state/node-state.json" \
HYDRA_NODE_CONFIG_PATH="$work/state/generated-config.json" \
HYDRA_NODE_RUNTIME_CONFIG_PATH="$work/state/node-runtime-config.json" \
HYDRA_NODE_XRAY_CONFIG_PATH="$work/state/xray.json" \
HYDRA_NODE_APPLY_HISTORY_PATH="$work/state/apply-history.json" \
HYDRA_NODE_RUNTIME_EVENTS_PATH="$work/state/runtime-events.json" \
  "./$agent_name" >"$work/node.log" 2>&1 &
node_pid=$!
cd "$repo_root"

ready=""
i=0
while [ "$i" -lt 60 ]; do
  if curl -fsS "http://127.0.0.1:$port/health" >/dev/null 2>&1; then
    ready=1
    break
  fi
  kill -0 "$node_pid" 2>/dev/null \
    || fail "the packaged agent exited early:$(printf '\n')$(cat "$work/node.log")"
  i=$((i + 1))
  sleep 1
done
[ -n "$ready" ] \
  || fail "the packaged agent never answered /health:$(printf '\n')$(cat "$work/node.log")"

printf 'Node release package runs where it is deployed: /health answered on port %s\n' "$port"

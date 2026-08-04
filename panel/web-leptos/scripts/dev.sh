#!/bin/sh
# Runs a panel and `trunk serve` against it, for looking at the frontend by hand.
#
# Development only. In production the panel is the origin and serves the bundle
# itself; this exists so the frontend can be opened without that wiring being
# finished, and so the browser talks to one origin instead of tripping over CORS.
#
# Data is seeded because a screen checked against zero rows is a check that
# cannot fail: an empty list renders the same whether the list works or not.
set -eu

panel_port="${HYDRA_DEV_PANEL_PORT:-18080}"
serve_port="${HYDRA_DEV_SERVE_PORT:-8082}"
admin_user="dev-admin"
admin_password="dev-password-that-is-long-enough"

here="$(cd "$(dirname "$0")" && pwd)"
panel_root="$(cd "$here/../.." && pwd)"
state="${HYDRA_DEV_STATE_DIR:-$panel_root/.dev-leptos}"

panel_pid=""
cleanup() {
  [ -n "$panel_pid" ] && kill "$panel_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# 0700, because the panel refuses to start on a data directory any wider — the
# same check that protects a real deployment, and it has no exception for
# development.
mkdir -p "$state"
chmod 700 "$state"
cd "$panel_root"
cargo build -p panel-app

HYDRA_BIND_ADDR="127.0.0.1:$panel_port" \
HYDRA_BOOTSTRAP_ADMIN_USERNAME="$admin_user" \
HYDRA_BOOTSTRAP_ADMIN_PASSWORD="$admin_password" \
HYDRA_ADMIN_PATH="$state/admin.json" \
HYDRA_ADMIN_SECRETS_KEY_PATH="$state/admin.key" \
HYDRA_SECURITY_SETTINGS_PATH="$state/security.json" \
HYDRA_AUDIT_LOG_PATH="$state/audit.ndjson" \
HYDRA_OPERATIONAL_LOG_PATH="$state/operational.ndjson" \
HYDRA_USERS_PATH="$state/users.json" \
HYDRA_USER_ACTIVITY_LOG_PATH="$state/user-activity.ndjson" \
HYDRA_USER_TEMPLATES_PATH="$state/user-templates.json" \
HYDRA_NODES_PATH="$state/nodes.json" \
HYDRA_NODE_SECRETS_KEY_PATH="$state/nodes.key" \
HYDRA_NODE_SYNC_HISTORY_PATH="$state/node-sync.ndjson" \
HYDRA_CLUSTERS_PATH="$state/clusters.json" \
HYDRA_NETWORK_RESOURCES_PATH="$state/network.json" \
HYDRA_CORE_CONFIG_PATH="$state/core.json" \
  ./target/debug/panel-app >"$state/panel.log" 2>&1 &
panel_pid=$!

i=0
while [ "$i" -lt 60 ]; do
  curl -fsS "http://127.0.0.1:$panel_port/health" >/dev/null 2>&1 && break
  kill -0 "$panel_pid" 2>/dev/null || { echo "panel exited:"; cat "$state/panel.log"; exit 1; }
  i=$((i + 1))
  sleep 1
done

token="$(curl -fsS -X POST "http://127.0.0.1:$panel_port/api/admin/login" \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"$admin_user\",\"password\":\"$admin_password\"}" \
  | sed -n 's/.*"token":"\([^"]*\)".*/\1/p')"
[ -n "$token" ] || { echo "could not log in to seed data"; exit 1; }

for name in alice bob carol; do
  curl -fsS -o /dev/null -X POST "http://127.0.0.1:$panel_port/api/users" \
    -H "Authorization: Bearer $token" \
    -H 'Content-Type: application/json' \
    -d "{\"username\":\"$name\",\"note\":\"seeded for frontend development\"}" \
    || echo "note: user $name already exists"
done

printf '\n'
printf 'Panel  : http://127.0.0.1:%s   (log: %s/panel.log)\n' "$panel_port" "$state"
printf 'Sign in: %s / %s\n' "$admin_user" "$admin_password"
printf 'Open   : http://127.0.0.1:%s\n' "$serve_port"
printf '\n'

cd "$here/.."
exec trunk serve \
  --port "$serve_port" \
  --proxy-backend "http://127.0.0.1:$panel_port/api/"

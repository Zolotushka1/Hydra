#!/usr/bin/env sh
set -eu

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 1
}

info() {
  printf '%s\n' "$*"
}

hysteria_bin="${HYDRA_NODE_HYSTERIA2_BINARY_PATH:-/usr/local/bin/hysteria}"
hysteria_service="${HYDRA_NODE_HYSTERIA2_SERVICE_NAME:-}"
wg_bin="${HYDRA_NODE_WIREGUARD_BINARY_PATH:-/usr/bin/wg}"
wg_quick_bin="${HYDRA_NODE_WG_QUICK_BINARY_PATH:-/usr/bin/wg-quick}"

info "== Hysteria2 binary preflight =="
if [ -x "$hysteria_bin" ]; then
  if "$hysteria_bin" version >/dev/null 2>&1; then
    "$hysteria_bin" version
  elif "$hysteria_bin" --version >/dev/null 2>&1; then
    "$hysteria_bin" --version
  else
    fail "Hysteria2 binary exists but version probe failed: $hysteria_bin"
  fi
else
  info "SKIP: Hysteria2 binary is not executable at $hysteria_bin"
fi

if [ -n "$hysteria_service" ]; then
  info "== Hysteria2 systemd service preflight =="
  command -v systemctl >/dev/null 2>&1 || fail "systemctl is required for Hysteria2 service check"
  load_state="$(systemctl show "$hysteria_service" -p LoadState --value 2>/dev/null || true)"
  [ "$load_state" != "not-found" ] && [ -n "$load_state" ] || fail "Hysteria2 service is not installed: $hysteria_service"
  printf 'SERVICE=%s\nLOAD_STATE=%s\nACTIVE_STATE=%s\n' \
    "$hysteria_service" \
    "$load_state" \
    "$(systemctl show "$hysteria_service" -p ActiveState --value 2>/dev/null || true)"
else
  info "SKIP: HYDRA_NODE_HYSTERIA2_SERVICE_NAME is not set"
fi

info "== WireGuard toolchain preflight =="
if [ -x "$wg_bin" ] && [ -x "$wg_quick_bin" ]; then
  private_key="$("$wg_bin" genkey)"
  tmp="$(mktemp)"
  trap 'rm -f "$tmp"' EXIT
  cat >"$tmp" <<CONF
[Interface]
PrivateKey = $private_key
Address = 10.255.0.1/32
ListenPort = 51820
CONF
  "$wg_quick_bin" strip "$tmp" >/dev/null
  "$wg_bin" --version 2>/dev/null || "$wg_bin" help 2>/dev/null || true
  info "WireGuard wg/wg-quick syntax validation passed"
else
  info "SKIP: WireGuard tools are not both executable: wg=$wg_bin wg-quick=$wg_quick_bin"
fi

info "sidecar integration preflight completed"

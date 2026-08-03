#!/bin/sh
set -eu

fail() {
  printf 'hydra installer: %s\n' "$1" >&2
  exit 1
}

prompt_value() {
  message="$1"
  default_value="${2:-}"
  [ -t 0 ] || fail "interactive input is unavailable; provide installer environment variables"
  if [ -n "$default_value" ]; then
    printf '%s [%s]: ' "$message" "$default_value" >&2
  else
    printf '%s: ' "$message" >&2
  fi
  IFS= read -r entered_value
  [ -n "$entered_value" ] || entered_value="$default_value"
  [ -n "$entered_value" ] || fail "required value must not be empty"
  printf '%s' "$entered_value"
}

read_checksum() {
  checksum_file="$1"
  checksum_value="$(awk 'NR == 1 { print $1 }' "$checksum_file")"
  case "$checksum_value" in *[!0-9a-fA-F]*|'') fail "checksum file is invalid" ;; esac
  [ "${#checksum_value}" -eq 64 ] || fail "checksum must contain 64 hex characters"
  printf '%s' "$checksum_value"
}

[ "$(id -u)" -eq 0 ] || fail "run this installer as root"
command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v sha256sum >/dev/null 2>&1 || fail "sha256sum is required"

case "$(uname -m)" in
  x86_64|amd64) executor_arch="x86_64" ;;
  aarch64|arm64) executor_arch="aarch64" ;;
  *) fail "unsupported CPU architecture" ;;
esac

release_base="${HYDRA_INSTALLER_RELEASE_BASE_URL:-https://github.com/Zolotushka1/Hydra-Panel/releases/latest/download}"
case "$release_base" in https://*) ;; *) fail "release base URL must use HTTPS" ;; esac
release_base="${release_base%/}"

if [ -z "${HYDRA_INSTALLER_MODE:-}" ]; then
  HYDRA_INSTALLER_MODE="$(prompt_value "Installation mode (first_host or managed)" "first_host")"
  export HYDRA_INSTALLER_MODE
fi

case "$HYDRA_INSTALLER_MODE" in
  managed)
    if [ -z "${HYDRA_INSTALLER_PANEL_URL:-}" ]; then HYDRA_INSTALLER_PANEL_URL="$(prompt_value "Existing Hydra Panel URL (HTTPS)")"; export HYDRA_INSTALLER_PANEL_URL; fi
    if [ -z "${HYDRA_INSTALLER_JOB_ID:-}" ]; then HYDRA_INSTALLER_JOB_ID="$(prompt_value "Installer job ID")"; export HYDRA_INSTALLER_JOB_ID; fi
    if [ -z "${HYDRA_INSTALLER_EXECUTOR_TOKEN:-}" ]; then
      [ -t 0 ] || fail "HYDRA_INSTALLER_EXECUTOR_TOKEN is required for non-interactive installation"
      printf 'One-time installer token: '
      stty -echo
      IFS= read -r HYDRA_INSTALLER_EXECUTOR_TOKEN
      stty echo
      printf '\n'
      export HYDRA_INSTALLER_EXECUTOR_TOKEN
    fi
    case "$HYDRA_INSTALLER_PANEL_URL" in
      https://*) ;;
      http://127.0.0.1:*|http://localhost:*) ;;
      *) fail "panel URL must use HTTPS except localhost development" ;;
    esac
    ;;
  first_host)
    if [ -z "${HYDRA_INSTALLER_ACCESS_MODE:-}" ]; then
      [ -t 0 ] || fail "HYDRA_INSTALLER_ACCESS_MODE is required for non-interactive installation"
      printf 'Do you have a domain pointed to this server? [y/N]: '
      IFS= read -r has_domain
      case "$has_domain" in
        y|Y|yes|YES)
          HYDRA_INSTALLER_ACCESS_MODE="domain_tls"
          ;;
        *)
          printf 'Use recommended self-signed HTTPS for IP access? [Y/n]: '
          IFS= read -r use_tls
          case "$use_tls" in
            n|N|no|NO) HYDRA_INSTALLER_ACCESS_MODE="ip_http" ;;
            *) HYDRA_INSTALLER_ACCESS_MODE="ip_self_signed_tls" ;;
          esac
          ;;
      esac
      export HYDRA_INSTALLER_ACCESS_MODE
    fi
    case "$HYDRA_INSTALLER_ACCESS_MODE" in
      domain_tls)
        if [ -z "${HYDRA_INSTALLER_DOMAIN:-}" ]; then HYDRA_INSTALLER_DOMAIN="$(prompt_value "Panel domain")"; export HYDRA_INSTALLER_DOMAIN; fi
        if [ -z "${HYDRA_INSTALLER_BIND_PORT:-}" ]; then HYDRA_INSTALLER_BIND_PORT=443; export HYDRA_INSTALLER_BIND_PORT; fi
        ;;
      ip_self_signed_tls|ip_http)
        if [ -z "${HYDRA_INSTALLER_PUBLIC_IP:-}" ]; then HYDRA_INSTALLER_PUBLIC_IP="$(prompt_value "Public server IP")"; export HYDRA_INSTALLER_PUBLIC_IP; fi
        if [ -z "${HYDRA_INSTALLER_BIND_PORT:-}" ]; then HYDRA_INSTALLER_BIND_PORT=2053; export HYDRA_INSTALLER_BIND_PORT; fi
        if [ "$HYDRA_INSTALLER_ACCESS_MODE" = "ip_http" ]; then
          export HYDRA_INSTALLER_CONFIRM_PUBLIC_HTTP=1
        fi
        ;;
      reverse_proxy)
        if [ -z "${HYDRA_INSTALLER_BIND_PORT:-}" ]; then HYDRA_INSTALLER_BIND_PORT=8080; export HYDRA_INSTALLER_BIND_PORT; fi
        if [ -z "${HYDRA_INSTALLER_BIND_HOST:-}" ]; then HYDRA_INSTALLER_BIND_HOST=127.0.0.1; export HYDRA_INSTALLER_BIND_HOST; fi
        ;;
      *) fail "unsupported HYDRA_INSTALLER_ACCESS_MODE" ;;
    esac
    if [ -z "${HYDRA_INSTALLER_FIREWALL_ALLOWLIST+x}" ] && [ -t 0 ]; then
      printf 'Optional panel firewall allowlist (comma-separated IP/CIDR, empty to skip): '
      IFS= read -r HYDRA_INSTALLER_FIREWALL_ALLOWLIST
      export HYDRA_INSTALLER_FIREWALL_ALLOWLIST
    fi
    export HYDRA_INSTALLER_PACKAGE_CHANNEL="${HYDRA_INSTALLER_PACKAGE_CHANNEL:-stable}"
    export HYDRA_INSTALLER_PANEL_BINARY_VERSION="${HYDRA_INSTALLER_PANEL_BINARY_VERSION:-latest}"
    export HYDRA_INSTALLER_PANEL_BINARY_URL="${HYDRA_INSTALLER_PANEL_BINARY_URL:-$release_base/hydra-panel-linux-$executor_arch}"
    ;;
  *) fail "HYDRA_INSTALLER_MODE must be first_host or managed" ;;
esac

if [ "${HYDRA_INSTALLER_DRY_RUN:-0}" != "1" ] && [ -z "${HYDRA_INSTALLER_CONFIRM_DESTRUCTIVE:-}" ]; then
  [ -t 0 ] || fail "HYDRA_INSTALLER_CONFIRM_DESTRUCTIVE=1 is required for non-interactive installation"
  printf 'The installer will modify system files, firewall/certificates when selected, and systemd. Type YES to continue: '
  IFS= read -r confirmation
  [ "$confirmation" = "YES" ] || fail "installation cancelled"
  export HYDRA_INSTALLER_CONFIRM_DESTRUCTIVE=1
fi
if [ "${HYDRA_INSTALLER_DRY_RUN:-0}" != "1" ]; then
  [ "$HYDRA_INSTALLER_CONFIRM_DESTRUCTIVE" = "1" ] || fail "destructive installation was not confirmed"
fi

executor_url="${HYDRA_INSTALLER_EXECUTOR_URL:-$release_base/panel-installer-executor-linux-$executor_arch}"
checksum_url="${HYDRA_INSTALLER_EXECUTOR_SHA256_URL:-$executor_url.sha256}"
case "$executor_url" in https://*) ;; *) fail "executor URL must use HTTPS" ;; esac
case "$checksum_url" in https://*) ;; *) fail "checksum URL must use HTTPS" ;; esac

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM
executor_path="$work_dir/panel-installer-executor"
checksum_path="$work_dir/panel-installer-executor.sha256"

curl --fail --show-error --silent --location --proto '=https' --proto-redir '=https' --tlsv1.2 "$executor_url" -o "$executor_path"
curl --fail --show-error --silent --location --proto '=https' --proto-redir '=https' --tlsv1.2 "$checksum_url" -o "$checksum_path"
expected_sha256="$(read_checksum "$checksum_path")"
actual_sha256="$(sha256sum "$executor_path" | awk '{ print $1 }')"
[ "$actual_sha256" = "$expected_sha256" ] || fail "executor SHA-256 mismatch"

if [ "$HYDRA_INSTALLER_MODE" = "first_host" ] && [ -z "${HYDRA_INSTALLER_PANEL_BINARY_SHA256:-}" ]; then
  panel_checksum_url="${HYDRA_INSTALLER_PANEL_BINARY_SHA256_URL:-$HYDRA_INSTALLER_PANEL_BINARY_URL.sha256}"
  case "$panel_checksum_url" in https://*) ;; *) fail "panel checksum URL must use HTTPS" ;; esac
  panel_checksum_path="$work_dir/panel.sha256"
  curl --fail --show-error --silent --location --proto '=https' --proto-redir '=https' --tlsv1.2 "$panel_checksum_url" -o "$panel_checksum_path"
  HYDRA_INSTALLER_PANEL_BINARY_SHA256="$(read_checksum "$panel_checksum_path")"
  export HYDRA_INSTALLER_PANEL_BINARY_SHA256
fi

chmod 0700 "$executor_path"
if "$executor_path"; then executor_status=0; else executor_status=$?; fi
unset HYDRA_INSTALLER_EXECUTOR_TOKEN HYDRA_BOOTSTRAP_ADMIN_PASSWORD
exit "$executor_status"

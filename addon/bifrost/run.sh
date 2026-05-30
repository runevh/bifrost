#!/usr/bin/env bash
set -euo pipefail

CONFIG_DIR=/data
OPTIONS_FILE="${CONFIG_DIR}/options.json"
CONFIG_FILE=/app/config.yaml
STATE_FILE="${CONFIG_DIR}/state.yaml"
CERT_FILE="${CONFIG_DIR}/cert.pem"

log_info() {
  printf '[%(%H:%M:%S)T] INFO: %s\n' -1 "$*"
}

log_fatal() {
  printf '[%(%H:%M:%S)T] FATAL: %s\n' -1 "$*" >&2
}

yaml_quote() {
  local value="${1:-}"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '"%s"' "${value}"
}

default_iface() {
  ip route show default 2>/dev/null | awk 'NR == 1 { for (i = 1; i <= NF; i++) if ($i == "dev") print $(i + 1) }'
}

default_cidr() {
  local iface="$1"
  ip -4 -o addr show dev "${iface}" scope global 2>/dev/null | awk 'NR == 1 { print $4 }'
}

default_mac() {
  local iface="$1"
  cat "/sys/class/net/${iface}/address" 2>/dev/null || true
}

default_gateway() {
  ip route show default 2>/dev/null | awk 'NR == 1 { for (i = 1; i <= NF; i++) if ($i == "via") print $(i + 1) }'
}

cidr_ip() {
  printf '%s' "${1%%/*}"
}

cidr_prefix() {
  local cidr="$1"
  if [[ "${cidr}" == */* ]]; then
    printf '%s' "${cidr##*/}"
  fi
}

prefix_to_netmask() {
  local prefix="${1:-}"
  local mask octet value

  if ! [[ "${prefix}" =~ ^[0-9]+$ ]] || (( prefix < 0 || prefix > 32 )); then
    return 1
  fi

  mask=$(( 0xffffffff << (32 - prefix) & 0xffffffff ))
  for octet in 24 16 8 0; do
    value=$(( (mask >> octet) & 255 ))
    if [[ "${octet}" == 24 ]]; then
      printf '%s' "${value}"
    else
      printf '.%s' "${value}"
    fi
  done
}

supervisor_get() {
  local path="$1"

  curl -fsS \
    -H "Authorization: Bearer ${supervisor_token}" \
    -H "Content-Type: application/json" \
    "http://supervisor${path}" 2>/dev/null || true
}

read_env_value() {
  local name="$1"
  local file="/run/s6/container_environment/${name}"

  if [[ -n "${!name:-}" ]]; then
    printf '%s' "${!name}"
  elif [[ -r "${file}" ]]; then
    cat "${file}"
  fi
}

require_value() {
  local name="$1"
  local value="$2"

  if [[ -z "${value}" ]]; then
    log_fatal "Missing required value: ${name}"
    exit 1
  fi
}

option() {
  local key="$1"
  local default_value="${2:-}"

  jq -r --arg key "${key}" --arg default_value "${default_value}" \
    '.[$key] // $default_value' "${OPTIONS_FILE}"
}

bridge_name="$(option bridge_name Bifrost)"
bridge_mac="$(option bridge_mac)"
bridge_ipaddress="$(option bridge_ipaddress)"
bridge_netmask="$(option bridge_netmask)"
bridge_gateway="$(option bridge_gateway)"
bridge_timezone="$(option bridge_timezone)"
disable_tls_verify="$(option disable_tls_verify false)"
transition_ms="$(option transition_ms 350)"
light_update_buffer_ms="$(option light_update_buffer_ms 80)"

supervisor_token="$(read_env_value SUPERVISOR_TOKEN)"
if [[ -z "${supervisor_token}" ]]; then
  supervisor_token="$(read_env_value HASSIO_TOKEN)"
fi
require_value SUPERVISOR_TOKEN "${supervisor_token}"

network_info="$(supervisor_get '/network/info')"
host_info="$(supervisor_get '/host/info')"
supervisor_info="$(supervisor_get '/info')"

primary_iface="$(jq -r '(.data.interfaces // .interfaces // []) | map(select((.primary == true) and (.enabled == true) and (.connected == true))) | .[0].interface // empty' <<<"${network_info}" 2>/dev/null || true)"
primary_cidr="$(jq -r '(.data.interfaces // .interfaces // []) | map(select((.primary == true) and (.enabled == true) and (.connected == true))) | .[0].ipv4.ip_address // .[0].ipv4.address[0] // empty' <<<"${network_info}" 2>/dev/null || true)"
primary_gateway="$(jq -r '(.data.interfaces // .interfaces // []) | map(select((.primary == true) and (.enabled == true) and (.connected == true))) | .[0].ipv4.gateway // empty' <<<"${network_info}" 2>/dev/null || true)"
host_timezone="$(jq -r '.data.timezone // .timezone // empty' <<<"${host_info}" 2>/dev/null || true)"
if [[ -z "${host_timezone}" ]]; then
  host_timezone="$(jq -r '.data.timezone // .timezone // empty' <<<"${supervisor_info}" 2>/dev/null || true)"
fi

iface="$(default_iface)"
if [[ -z "${bridge_ipaddress}" && -n "${primary_cidr}" ]]; then
  bridge_ipaddress="$(cidr_ip "${primary_cidr}")"
fi
if [[ -z "${bridge_ipaddress}" && -n "${iface}" ]]; then
  bridge_ipaddress="$(cidr_ip "$(default_cidr "${iface}")")"
fi
if [[ -z "${bridge_netmask}" && -n "${primary_cidr}" ]]; then
  bridge_netmask="$(prefix_to_netmask "$(cidr_prefix "${primary_cidr}")" || true)"
fi
if [[ -z "${bridge_netmask}" && -n "${iface}" ]]; then
  bridge_netmask="$(prefix_to_netmask "$(cidr_prefix "$(default_cidr "${iface}")")" || true)"
fi
if [[ -z "${bridge_mac}" && -n "${primary_iface}" ]]; then
  bridge_mac="$(default_mac "${primary_iface}")"
fi
if [[ -z "${bridge_mac}" && -n "${iface}" ]]; then
  bridge_mac="$(default_mac "${iface}")"
fi
if [[ -z "${bridge_gateway}" && -n "${primary_gateway}" ]]; then
  bridge_gateway="${primary_gateway}"
fi
if [[ -z "${bridge_gateway}" ]]; then
  bridge_gateway="$(default_gateway)"
fi
if [[ -z "${bridge_timezone}" && -n "${host_timezone}" ]]; then
  bridge_timezone="${host_timezone}"
fi

require_value bridge_ipaddress "${bridge_ipaddress}"
require_value bridge_mac "${bridge_mac}"
require_value bridge_netmask "${bridge_netmask}"
require_value bridge_gateway "${bridge_gateway}"
require_value bridge_timezone "${bridge_timezone}"

cat >"${CONFIG_FILE}" <<EOF
bifrost:
  state_file: "${STATE_FILE}"
  cert_file: "${CERT_FILE}"

bridge:
  name: $(yaml_quote "${bridge_name}")
  mac: ${bridge_mac}
  ipaddress: ${bridge_ipaddress}
  netmask: ${bridge_netmask}
  gateway: ${bridge_gateway}
  timezone: $(yaml_quote "${bridge_timezone}")

homeassistant:
  url: http://supervisor/core/api/
  websocket_url: ws://supervisor/core/websocket
  token: $(yaml_quote "${supervisor_token}")
  disable_tls_verify: ${disable_tls_verify}
  transition_ms: ${transition_ms}
  light_update_buffer_ms: ${light_update_buffer_ms}
EOF

log_info "Starting Bifrost on ${bridge_ipaddress} using the Home Assistant Supervisor proxy"
exec /app/bifrost

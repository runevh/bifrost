# Bifrost Bridge Add-on

## Configuration

The add-on UI options are converted into the `config.yaml` format expected by Bifrost when the container starts.

The add-on uses Home Assistant's Supervisor proxy and the runtime `SUPERVISOR_TOKEN`, so you do not need to create a long-lived access token.

Recommended settings:

```yaml
bridge_name: Bifrost
bridge_ipaddress: ""
bridge_mac: ""
bridge_netmask: ""
bridge_gateway: ""
bridge_timezone: ""
disable_tls_verify: false
transition_ms: 350
light_update_buffer_ms: 80
```

When the network fields are empty, startup reads Supervisor's `/network/info` and `/host/info` endpoints and fills:

- `bridge_ipaddress` from the primary IPv4 address.
- `bridge_netmask` from the primary IPv4 CIDR prefix.
- `bridge_gateway` from the primary IPv4 gateway.
- `bridge_timezone` from the host timezone.
- `bridge_mac` from the primary host interface.

If the wrong interface is selected, set those values manually in the add-on options.

## Network Requirements

Bifrost emulates a Philips Hue bridge, so it needs:

- TCP 80 for Hue HTTP discovery/API traffic.
- TCP 443 for Hue HTTPS API traffic.
- UDP 2100 for Hue entertainment streaming.
- Host networking for LAN discovery.

Do not run another service on the Home Assistant host that already binds ports 80 or 443.

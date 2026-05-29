# Bifrost Bridge Add-on

Runs Bifrost as a Home Assistant add-on.

This add-on uses host networking because Hue discovery, mDNS/SSDP, ports 80/443, and entertainment UDP traffic must be visible on the LAN.

## Required Options

No option is normally required. Leave the network fields empty to auto-detect them from Home Assistant Supervisor.

## Notes

- The generated Bifrost runtime config is written to `/data/config.yaml`.
- Bifrost state and the generated Hue-style certificate are persisted in `/data`.
- The add-on uses Home Assistant's Supervisor proxy and does not need a long-lived access token.
- If auto-detection picks the wrong interface, set `bridge_ipaddress`, `bridge_mac`, `bridge_netmask`, and `bridge_gateway` manually.

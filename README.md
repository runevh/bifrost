![](doc/logo-title-640x160.png)

# Bifrost Bridge

This repository is a personal fork of the original Bifrost project.

The goal of this fork is very specific:
- emulate a Philips Hue Bridge
- use **Home Assistant** as backend instead of Zigbee2MQTT
- make **WLED** devices behave like Hue gradient light strips in the Hue app

If you are looking for the generic upstream project, see:
- [chrivers/bifrost](https://github.com/chrivers/bifrost)

## Status

Current implementation in this fork:
- Home Assistant backend over websocket
- automatic import of HA lights and areas
- mapping of HA light capabilities to Hue resources
- WLED detection and direct handling for gradient-style updates

This is tailored for one environment and one workflow. Expect rough edges.

## Configuration (Home Assistant path)

Minimal `config.yaml` for this fork:

```yaml
bridge:
  name: Bifrost
  mac: 00:11:22:33:44:55
  ipaddress: 10.12.0.20
  netmask: 255.255.255.0
  gateway: 10.12.0.1
  timezone: Europe/Copenhagen

homeassistant:
  url: http://192.168.1.6:8123
  token: YOUR_LONG_LIVED_ACCESS_TOKEN
  disable_tls_verify: false
  transition_ms: 350
  light_update_buffer_ms: 80
```

Notes:
- `bridge.ipaddress` must be an IP currently assigned to this machine.
- `bridge.mac` should match the interface you use to expose the bridge.
- `homeassistant.url` can be `http://...` or `https://...`.
- `token` must be a Home Assistant long-lived access token.
- `light_update_buffer_ms` controls how long to buffer bursty light updates before flushing them together.

`z2m` can still be configured, but if both `homeassistant` and `z2m` are present, this fork prefers `homeassistant`.

## Running

From project root:

```sh
cargo run
```

## WLED gradient-strip behavior

In this fork, WLED support is designed around Hue gradient-strip UX:
- WLED is detected and exposed as Hue-style gradient-capable light resources.
- Hue app gradient updates are translated into WLED-compatible updates.
- State handling is biased toward preserving Hue app gradient intent.

This is intentionally optimized for a personal setup, not for broad device compatibility.

## Upstream references

- Original project: [chrivers/bifrost](https://github.com/chrivers/bifrost)
- DiyHue comparison from upstream docs: [doc/comparison-with-diyhue.md](doc/comparison-with-diyhue.md)
- General config reference: [doc/config-reference.md](doc/config-reference.md)

## Disclaimer

This fork is purpose-built for a specific home setup. Breaking changes are expected while iterating.

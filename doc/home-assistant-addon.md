# Home Assistant Add-on

This repo includes a Home Assistant add-on wrapper in `addon/bifrost`.

## What The Add-on Does

The add-on does not change Bifrost's internal configuration model. Instead, it:

1. Reads Home Assistant add-on options from `/data/options.json`.
2. Writes Bifrost's normal runtime config to `/data/config.yaml`.
3. Stores Bifrost state and the generated Hue-style certificate in `/data`.
4. Starts `/app/bifrost`.

This means the add-on UI replaces the hand-written `config.yaml` for add-on installs.

## Do You Still Need `config.yaml`?

For add-on installs, no. Configure the add-on through Home Assistant's add-on options.

For local development, Docker Compose, or `cargo run`, yes. Bifrost still reads `config.yaml` from the working directory.

## Home Assistant Token

The add-on does not need a Home Assistant long-lived access token. It sets `homeassistant_api: true` and uses the Supervisor-provided `SUPERVISOR_TOKEN` against the internal websocket proxy at `ws://supervisor/core/websocket`.

Local development, Docker Compose, and `cargo run` still need a token in `config.yaml` unless you run them inside the Home Assistant add-on environment.

## Automatic Host Values

Leave these add-on options empty to let startup fill them from Home Assistant Supervisor:

- `bridge_ipaddress`
- `bridge_mac`
- `bridge_netmask`
- `bridge_gateway`
- `bridge_timezone`

The startup script reads Supervisor's `/network/info` endpoint for the primary interface's IPv4 address and gateway, converts the CIDR prefix to a netmask, reads the host timezone from Supervisor, and falls back to Linux route/interface detection if needed.

## Network Requirements

The add-on uses `host_network: true` because Hue emulation depends on LAN-visible discovery and fixed ports:

- TCP 80 for Hue HTTP API/discovery traffic.
- TCP 443 for Hue HTTPS API traffic.
- UDP 2100 for Hue entertainment streaming.

Make sure nothing else on the Home Assistant host is already binding TCP 80 or TCP 443.

## Repository Layout Caveat

Home Assistant add-on repositories are discovered by looking for add-on `config.yaml` files. This repo's runtime `config.yaml` is ignored by git, so it will not be present when installed from GitHub.

If you add this repo to Home Assistant as a local add-on repository from a working tree, do not keep your local runtime `config.yaml` in that same tree. Supervisor can mistake it for an add-on metadata file.

## Build Source

The add-on Dockerfile builds Bifrost from git because Home Assistant builds add-ons with the add-on folder as the Docker build context.

The source repository and branch are configured in `addon/bifrost/build.yaml`:

```yaml
args:
  BIFROST_REPOSITORY: https://github.com/runevh/bifrost.git
  BIFROST_REF: master
```

If you test an unpublished branch, update `BIFROST_REF` before building the add-on.

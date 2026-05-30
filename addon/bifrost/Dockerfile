ARG BUILD_FROM
ARG RUST_VERSION=1.85

FROM rust:${RUST_VERSION}-slim-bookworm AS build

ARG BIFROST_REPOSITORY
ARG BIFROST_REF

WORKDIR /src

RUN apt-get update \
    && apt-get install -y --no-install-recommends git pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch "${BIFROST_REF}" "${BIFROST_REPOSITORY}" .
RUN cargo build --locked --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends bash ca-certificates curl iproute2 jq libssl3 tzdata \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/bifrost /app/bifrost
COPY run.sh /run.sh

RUN chmod a+x /run.sh /app/bifrost

WORKDIR /app
CMD ["/run.sh"]

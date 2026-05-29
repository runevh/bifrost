ARG BUILD_FROM
ARG RUST_VERSION=1.85

FROM rust:${RUST_VERSION}-alpine AS build

ARG BIFROST_REPOSITORY
ARG BIFROST_REF

WORKDIR /src

RUN apk add --no-cache git musl-dev pkgconf openssl-dev
RUN git clone --depth 1 --branch "${BIFROST_REF}" "${BIFROST_REPOSITORY}" .
RUN cargo build --locked --release

FROM ${BUILD_FROM}

RUN apk add --no-cache bash libgcc libssl3 ca-certificates curl iproute2 jq tzdata

COPY --from=build /src/target/release/bifrost /app/bifrost
COPY run.sh /run.sh

RUN chmod a+x /run.sh /app/bifrost

WORKDIR /app
CMD ["/run.sh"]

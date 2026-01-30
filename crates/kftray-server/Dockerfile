# syntax=docker/dockerfile:1

FROM rust:alpine@sha256:69d7b9d9aeaf108a1419d9a7fcf7860dcc043e9dbd1ab7ce88e44228774d99e9 AS builder

ARG TARGETARCH

RUN apk add --no-cache musl-dev

RUN rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl

WORKDIR /build

COPY crates/kftray-server ./

RUN case "$TARGETARCH" in \
        amd64) RUST_TARGET="x86_64-unknown-linux-musl" ;; \
        arm64) RUST_TARGET="aarch64-unknown-linux-musl" ;; \
        *) echo "Unsupported architecture: $TARGETARCH" && exit 1 ;; \
    esac && \
    cargo build --release --target $RUST_TARGET && \
    cp /build/target/$RUST_TARGET/release/kftray-server /kftray-server

FROM scratch

ARG VERSION="0.0.0"
ARG REVISION="unknown"
ARG CREATED="1970-01-01T00:00:00Z"

LABEL org.opencontainers.image.title="kftray-server" \
      org.opencontainers.image.description="KFtray Server - UDP/TCP traffic relay for Kubernetes port forwarding" \
      org.opencontainers.image.url="https://kftray.app" \
      org.opencontainers.image.source="https://github.com/hcavarsan/kftray" \
      org.opencontainers.image.documentation="https://github.com/hcavarsan/kftray#readme" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}" \
      org.opencontainers.image.licenses="GPL-3.0" \
      org.opencontainers.image.vendor="KFtray" \
      org.opencontainers.image.authors="Henrique Cavarsan <hencavarsan@gmail.com>" \
      org.opencontainers.image.base.name="scratch"

COPY --from=builder /kftray-server /kftray-server

ENV REMOTE_ADDRESS=127.0.0.1
ENV REMOTE_PORT=8080
ENV LOCAL_PORT=8080
ENV PROXY_TYPE=tcp

EXPOSE 8080

USER 65532:65532

ENTRYPOINT ["/kftray-server"]

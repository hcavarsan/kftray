FROM rust:1.93.0-alpine3.23 AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /build

# Copy kftray-server
COPY crates/kftray-server ./

# Build kftray-server
RUN cargo build --release

FROM alpine:3.21@sha256:56fa17d2a7e7f168a043a2712e63aed1f8543aeafdcee47c58dcffe38ed51099

RUN apk add --no-cache ca-certificates libgcc openssl

COPY --from=builder /build/target/release/kftray-server /kftray-server

ENV REMOTE_ADDRESS=127.0.0.1
ENV REMOTE_PORT=8080
ENV LOCAL_PORT=8080
ENV PROXY_TYPE=tcp

EXPOSE 8080

CMD ["/kftray-server"]

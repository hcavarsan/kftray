FROM rust:1.90.0-alpine3.20 AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /workspace

# Copy workspace Cargo.toml and .cargo directory if it exists
COPY Cargo.toml ./
COPY .cargo ./.cargo 2>/dev/null || true

# Copy the crates needed for kftray-server
COPY crates/kftray-commons ./crates/kftray-commons
COPY crates/kftray-server ./crates/kftray-server

# Build kftray-server
WORKDIR /workspace/crates/kftray-server
RUN cargo build --release
RUN cargo install --path .

FROM alpine:3.22

RUN apk add --no-cache ca-certificates libgcc openssl

COPY --from=builder /usr/local/cargo/bin/kftray-server /kftray-server

ENV REMOTE_ADDRESS=127.0.0.1
ENV REMOTE_PORT=8080
ENV LOCAL_PORT=8080
ENV PROXY_TYPE=tcp

EXPOSE 8080

CMD ["/kftray-server"]

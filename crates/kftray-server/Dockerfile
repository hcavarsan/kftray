FROM rust:1.92.0-alpine3.20 AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /build

# Copy kftray-server
COPY crates/kftray-server ./

# Build kftray-server
RUN cargo build --release

FROM alpine:3.23

RUN apk add --no-cache ca-certificates libgcc openssl

COPY --from=builder /build/target/release/kftray-server /kftray-server

ENV REMOTE_ADDRESS=127.0.0.1
ENV REMOTE_PORT=8080
ENV LOCAL_PORT=8080
ENV PROXY_TYPE=tcp

EXPOSE 8080

CMD ["/kftray-server"]

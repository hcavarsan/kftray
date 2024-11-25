FROM rust:1.82.0-alpine3.20 AS builder

RUN apk add --no-cache musl-dev

WORKDIR /build

RUN cargo new --bin kftray-server
WORKDIR /build/kftray-server

COPY crates/kftray-server/Cargo.toml ./Cargo.toml

RUN touch Cargo.lock

RUN cargo build --release
RUN rm src/*.rs

COPY crates/kftray-server/src ./src/

RUN touch src/main.rs && cargo build --release
RUN cargo install --path .

FROM alpine:3.20

COPY --from=builder /usr/local/cargo/bin/kftray-server /kftray-server

ENV REMOTE_ADDRESS=localhost
ENV REMOTE_PORT=8080
ENV LOCAL_PORT=8080
ENV PROXY_TYPE=tcp

RUN mkdir -p /run/sshd

EXPOSE 2222

CMD ["/kftray-server"]

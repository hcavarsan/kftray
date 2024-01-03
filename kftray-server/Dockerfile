# Use a newer Rust base image
FROM rust:buster as builder

RUN USER=root cargo new --bin kftray-server
WORKDIR /kftray-server

COPY ./Cargo.toml ./Cargo.toml

RUN echo "fn main() {}" > src/main.rs

RUN cargo build --release

RUN rm -f target/release/deps/kftray_server*

COPY ./src ./src

RUN cargo build --release

FROM debian:buster-slim

COPY --from=builder /kftray-server/target/release/kftray-server .

ENTRYPOINT ["./kftray-server"]

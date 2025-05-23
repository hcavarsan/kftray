FROM rust:1.87.0-alpine3.20 AS builder

RUN apk add --no-cache musl-dev


RUN USER=root cargo new --bin kftray-server
WORKDIR /kftray-server

COPY . .

RUN cargo build --release
RUN cargo install --path .

FROM alpine:3.21

COPY --from=builder /usr/local/cargo/bin/kftray-server /kftray-server

ENV REMOTE_ADDRESS=127.0.0.1
ENV REMOTE_PORT=8080
ENV LOCAL_PORT=8080
ENV PROXY_TYPE=tcp

EXPOSE 8080

CMD ["/kftray-server"]

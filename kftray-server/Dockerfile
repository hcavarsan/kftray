FROM golang:alpine as builder

WORKDIR /app
COPY . .

RUN go mod tidy
RUN go mod download
RUN go get github.com/miladrahimi/netrelay
RUN CGO_ENABLED=0 go build -o kftray-server

FROM alpine
WORKDIR /root/
COPY --from=builder /app .


CMD ["./kftray-server"]

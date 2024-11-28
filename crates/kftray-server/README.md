# KFtray Server

A network proxy server written in Rust that forwards traffic between clients and target servers.

## Introduction

KFtray Server helps solve network connectivity issues by acting as an intermediary between clients and servers. It can handle TCP, UDP, and SSH protocols.

## How It Works

The server operates in three modes:

```mermaid
graph TD
    subgraph TCP Mode
        A[Kftray App] -->|TCP| B[KFtray Server]
        B -->|TCP| C[Target Server]
    end
```

```mermaid
graph TD
    subgraph UDP Mode
        D[Kftray App] -->|TCP Connection| E[KFtray Server]
        E -->|UDP Packets| F[Target Server]
    end
```

```mermaid
graph TD
    subgraph SSH Mode
        A[SSH Client] -->|Reverse Tunnel| B[KFtray Server]
        B -->|Forward Connection| C[Target SSH Server]
        C -->|Return Traffic| B
        B -->|Tunneled Response| A
     end
```

## Configuration

The server uses environment variables for configuration:

```bash
REMOTE_ADDRESS=target.host    # The address of your target server
REMOTE_PORT=8080             # The port on your target server
LOCAL_PORT=8080             # The port KFtray listens on
PROXY_TYPE=tcp             # Either 'tcp', 'udp', or 'ssh'
SSH_AUTH=false            # Enable/disable SSH authentication
SSH_AUTHORIZED_KEYS=""    # Comma-separated list of authorized public keys
```

### SSH Authentication Example

To enable SSH authentication:

```bash
# Enable SSH authentication
export SSH_AUTH=true

# Add authorized public keys
export SSH_AUTHORIZED_KEYS="ssh-rsa AAAA...,ssh-ed25519 AAAA..."
```

When SSH_AUTH is false, all SSH connections will be accepted without authentication (not recommended for production use).

## Running with Docker

```bash
docker run -e REMOTE_ADDRESS=target.host \
          -e REMOTE_PORT=8080 \
          -e LOCAL_PORT=8080 \
          -e PROXY_TYPE=tcp \
          -e SSH_AUTH=false \
          -e SSH_AUTHORIZED_KEYS="" \
          -p 8080:8080 \
          kftray-server
```



## Links

Documentation: [kftray.app](https://kftray.app)

Source Code: [github.com/hcavarsan/kftray](https://github.com/hcavarsan/kftray)

## 🏗 Architecture

### Server

KFtray Server is a Rust application that relays UDP/TCP traffic to an upstream server. Check the source code [here](https://github.com/hcavarsan/kftray/tree/main/crates/kftray-server).

### Forwarding Flows

- **TCP Forwarding:** A local TCP socket, similar to kubectl, can be used to communicate with a Kubernetes pod. This approach offers parallel execution and improved resilience.

```mermaid
sequenceDiagram
Application->>Kubernetes Pod: Opens TCP socket, starts port-forwarding
Kubernetes Pod-->>Application: Responds with TCP Packet
```

- **Proxy TCP Forwarding:** The local TCP connects to the kftray-server pod, which then sends TCP packet to the upstream server.

```mermaid
sequenceDiagram
Application->>Kubernetes Pod: Socket to kftray-server, facilitates TCP relay
Kubernetes Pod->>Remote Service: Relays TCP Packet
Remote Service-->>Kubernetes Pod: Responds
Kubernetes Pod-->>Application: Returns TCP Packet
```

- **UDP Forwarding:** The KFtray client opens a local UDP socket and connects a local TCP socket to the kftray-server pod. The TCP socket sends UDP packets over TCP, which are then forwarded to the upstream server.

```mermaid
sequenceDiagram
Application->>Kubernetes Pod: UDP socket, TCP port-forward to kftray-server
Kubernetes Pod->>Service/Remote: Converts to UDP, sends packet
Service/Remote-->>Kubernetes Pod: Responds with UDP Packet
Kubernetes Pod-->>Application: Relays as TCP
```

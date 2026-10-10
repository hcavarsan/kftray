## 🏗 Architecture

### Server

KFtray Server is a Rust application that relays UDP/TCP traffic to an upstream server. Check the source code [here](https://github.com/hcavarsan/kftray/tree/main/crates/kftray-server).

### Forwarding Flows

- **TCP Forwarding:** A local TCP socket, similar to kubectl, can be used to communicate with a Kubernetes pod. This approach offers parallel execution and improved resilience.

![Sequence diagram: the Local App connects to kftray on localhost:8080, kftray establishes a port-forward stream with the K8s API Server, which forwards traffic to the Target Pod; responses return along the same path.](https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/public/diagrams/tcp-forwarding.png)

- **Proxy TCP Forwarding:** The local TCP connects to the kftray-server pod, which then sends TCP packet to the upstream server.

![Sequence diagram: the Application opens a socket to kftray-server in the Kubernetes Pod, which relays the TCP packet to the Remote Service; the Remote Service responds and the Kubernetes Pod returns the TCP packet to the Application.](https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/public/diagrams/proxy-tcp-forwarding.png)

- **UDP Forwarding:** The KFtray client opens a local UDP socket and connects a local TCP socket to the kftray-server pod. The TCP socket sends UDP packets over TCP, which are then forwarded to the upstream server.

![Sequence diagram: the Local App sends a UDP packet to kftray, which converts it to TCP and forwards it to the in-cluster kftray-server via the Kubernetes port-forward; kftray-server converts it back to UDP for the Target Service, and the response returns to the Local App as UDP.](https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/public/diagrams/udp-forwarding.png)

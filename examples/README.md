# Configuration Examples

This directory contains example configurations for all kftray workload types. Each example demonstrates a specific use case with clear, concise comments.

## Service Examples

### [service-tcp.json](./service-tcp.json)
Basic TCP port-forward to a Kubernetes service. Forwards localhost:8080 to api-service:80 in the production namespace.

### [service-udp.json](./service-udp.json)
UDP port-forward to a Kubernetes service. Forwards UDP traffic from localhost:5353 to coredns:53 in kube-system namespace.

## Pod Examples

### [pod-tcp.json](./pod-tcp.json)
TCP port-forward directly to a pod using label selectors. Forwards localhost:8080 to pod:80 matching labels app=nginx,tier=frontend.

### [pod-tcp-with-http-logs.json](./pod-tcp-with-http-logs.json)
TCP port-forward to pod with HTTP traffic logging enabled. Logs HTTP requests/responses up to 20MB per file, retains logs for 14 days with auto-cleanup.

## Proxy Examples

### [proxy-tcp.json](./proxy-tcp.json)
TCP proxy to external resource via Kubernetes cluster. Creates a tunnel through the cluster to reach postgresql.external.example.com:5432 from localhost:5432.

### [proxy-udp.json](./proxy-udp.json)
UDP proxy to external DNS server via Kubernetes cluster. Tunnels UDP DNS queries from localhost:5353 to 8.8.8.8:53 through the cluster.

## Expose Examples

### [expose-public.json](./expose-public.json)
Expose local service to the internet via Kubernetes ingress with SSL. Reverse tunnel from localhost:3000 to public domain myapp.example.com with Let's Encrypt TLS certificate.

### [expose-internal.json](./expose-internal.json)
Expose local service to internal cluster network only. Reverse tunnel from localhost:8080 accessible only within the Kubernetes cluster (no public ingress).

## Usage

Import any example configuration into kftray:

```bash
# Using kftray GUI
# File → Import Configs → Select example JSON file

# Using kftui CLI with local file
kftui --config /path/to/example.json

# Using kftui CLI with GitHub URL
kftui --github-url https://raw.githubusercontent.com/hcavarsan/kftray/main/examples/service-tcp.json
```

## Field Reference

See the [main README](../README.md) for complete documentation of all available configuration fields.

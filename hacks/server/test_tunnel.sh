#!/bin/bash
set -e
# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

log_debug() {
    echo -e "${YELLOW}[DEBUG]${NC} $1"
}

NAMESPACE="test-ssh"
EXPECTED_CONTEXT="kind-kind-cluster"

log_info "Checking kubernetes context..."
CURRENT_CONTEXT=$(kubectl config current-context)
if [ "$CURRENT_CONTEXT" != "$EXPECTED_CONTEXT" ]; then
    log_error "Current context is $CURRENT_CONTEXT, expected $EXPECTED_CONTEXT"
    log_error "Please switch to the correct context using: kubectl config use-context $EXPECTED_CONTEXT"
    exit 1
fi
log_success "Using correct kubernetes context: $CURRENT_CONTEXT"

log_info "Building Docker image..."
cd $(git rev-parse --show-toplevel)
docker build -t kftray-server:latest -f crates/kftray-server/Dockerfile .
log_success "Docker image built successfully"

log_info "Cleaning up old images from kind cluster..."
NODE_NAME="kind-cluster-control-plane"


IMAGES=$(docker exec $NODE_NAME crictl images | grep 'kftray-server' | awk '{print $3}')
if [ ! -z "$IMAGES" ]; then
    for IMG_ID in $IMAGES; do
        log_debug "Removing image $IMG_ID"
        docker exec $NODE_NAME crictl rmi $IMG_ID 2>/dev/null || true
    done
fi
log_success "Old images removed from kind cluster"

log_info "Loading new image into kind cluster..."
kind load docker-image kftray-server:latest --name kind-cluster --quiet
log_success "Image loaded into kind cluster"

log_info "Setting up test environment..."

# Add new function to generate SSH keys
generate_ssh_keys() {
    log_info "Generating SSH keys..."

    # Create temporary directory for keys
    SSH_KEY_DIR=$(mktemp -d)
    SSH_KEY_FILE="$SSH_KEY_DIR/id_ed25519"

    # Generate SSH key pair
    ssh-keygen -t ed25519 -f "$SSH_KEY_FILE" -N "" -C "test@kftray.app"

    # Get the public key content
    SSH_PUB_KEY=$(cat "${SSH_KEY_FILE}.pub")

    log_success "SSH keys generated at $SSH_KEY_DIR"
    echo "Private key: $SSH_KEY_FILE"
    echo "Public key: ${SSH_KEY_FILE}.pub"
}

generate_ssh_keys

kubectl create namespace $NAMESPACE 2>/dev/null || true
log_success "Namespace $NAMESPACE ready"


log_info "Cleaning up any previous instances..."
kubectl delete service kftray-server -n $NAMESPACE 2>/dev/null || true
kubectl delete pod kftray-server --force --grace-period=0 -n $NAMESPACE 2>/dev/null || true
kubectl delete pod curl-test --force --grace-period=0 -n $NAMESPACE 2>/dev/null || true
kill $(lsof -t -i:8085) 2>/dev/null || true
kill $(lsof -t -i:2222) 2>/dev/null || true
sleep 2
log_success "Cleanup completed"

log_info "Starting local HTTP server..."
cat > /tmp/test_server.py << EOF
import http.server
import socketserver
from http import HTTPStatus

class Handler(http.server.SimpleHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def do_GET(self):
        self.send_response(HTTPStatus.OK)
        response = b'Hello, World!'
        self.send_header('Content-Type', 'text/plain')
        self.send_header('Content-Length', str(len(response)))
        self.send_header('Connection', 'close')
        self.end_headers()
        self.wfile.write(response)
        self.wfile.flush()

class TCPServer(socketserver.TCPServer):
    allow_reuse_address = True

httpd = TCPServer(('', 8085), Handler)
httpd.serve_forever()
EOF

python3 /tmp/test_server.py &
HTTP_PID=$!

sleep 2

log_debug "HTTP server PID: $HTTP_PID"
log_success "Local HTTP server started"


log_info "Verifying HTTP server..."
for i in {1..3}; do
    echo "Test $i:"
    curl -v --max-time 5 localhost:8085
    echo
    sleep 1
done

log_info "Creating kftray-server service and pod..."
cat <<EOF | kubectl apply -n $NAMESPACE -f -
apiVersion: v1
kind: Service
metadata:
  name: kftray-server
spec:
  selector:
    app: kftray-server
  ports:
    - name: ssh
      port: 2222
      targetPort: 2222
    - name: proxy
      port: 8085
      targetPort: 8085
---
apiVersion: v1
kind: Pod
metadata:
  name: kftray-server
  labels:
    app: kftray-server
spec:
  containers:
  - name: kftray-server
    image: kftray-server:latest
    imagePullPolicy: Never
    ports:
    - containerPort: 2222
      name: ssh
    - containerPort: 8085
      name: proxy
    env:
    - name: PROXY_TYPE
      value: "ssh"
    - name: LOCAL_PORT
      value: "2222"
    - name: REMOTE_PORT
      value: "8085"
    - name: REMOTE_ADDRESS
      value: "0.0.0.0"
    - name: RUST_LOG
      value: "trace"
    - name: SSH_AUTH
      value: "true"
    - name: SSH_AUTHORIZED_KEYS
      value: "${SSH_PUB_KEY}"
    securityContext:
      capabilities:
        add: ["NET_BIND_SERVICE"]
EOF

log_info "Waiting for pod to be ready..."
kubectl wait --for=condition=ready pod/kftray-server -n $NAMESPACE --timeout=60s
sleep 2
log_success "kftray-server pod is ready"

wait_for_port() {
    local port=$1
    local max_attempts=30
    local attempt=1

    while [ $attempt -le $max_attempts ]; do
        if ! lsof -i :$port > /dev/null 2>&1; then
            return 0
        fi
        log_warning "Port $port is still in use, waiting... (attempt $attempt/$max_attempts)"
        sleep 1
        attempt=$((attempt + 1))
    done
    return 1
}

log_info "Waiting for ports to be available..."
wait_for_port 2222 || (log_error "Port 2222 is still in use" && exit 1)

log_info "Setting up port forwarding..."
kubectl port-forward pod/kftray-server -n $NAMESPACE 2222:2222 &
PORTFORWARD_PID=$!
sleep 5

if ! ps -p $PORTFORWARD_PID > /dev/null; then
    log_error "Port forwarding failed to start"
    exit 1
fi
log_success "Port forwarding established"

log_info "Creating SSH reverse tunnel..."
SSH_CMD="ssh \
    -i $SSH_KEY_FILE \
    -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=/dev/null \
    -o ExitOnForwardFailure=yes \
    -o ConnectTimeout=10 \
    -o ServerAliveInterval=30 \
    -o ServerAliveCountMax=3 \
    -R 0.0.0.0:8085:localhost:8085 \
    -p 2222 \
    -N localhost"

$SSH_CMD &
SSH_PID=$!

log_info "Waiting for SSH tunnel to establish..."
sleep 15

log_success "SSH tunnel established"
log_info "Waiting for tunnel to stabilize..."
sleep 15


log_info "Verifying SSH tunnel..."
kubectl exec -n $NAMESPACE kftray-server -- ss -tlnp || true

log_info "Testing connection through the tunnel..."
cat <<EOF | kubectl apply -n $NAMESPACE -f -
apiVersion: v1
kind: Pod
metadata:
  name: curl-test
spec:
  containers:
  - name: curl
    image: curlimages/curl
    command:
    - "/bin/sh"
    - "-c"
    - |
      for i in {1..3}; do
        echo "=== Test attempt $i ==="
        curl -v --connect-timeout 5 --max-time 10 \
             -H "Connection: close" \
             -H "User-Agent: curl-test" \
             http://kftray-server:8085/
        echo "=== End of attempt $i ==="
        sleep 2
      done
      sleep infinity
    env:
    - name: NO_PROXY
      value: "kftray-server"
EOF

log_info "Waiting for curl-test pod to be ready..."
kubectl wait --for=condition=ready pod/curl-test -n $NAMESPACE --timeout=30s
log_success "curl-test pod is ready"


log_info "Executing curl test..."
kubectl exec -n $NAMESPACE curl-test -- curl -v --max-time 5 http://kftray-server:8085

#log_debug "kftray-server pod logs after test:"
#kubectl logs -n $NAMESPACE kftray-server

#log_info "Cleaning up..."
#kill $SSH_PID 2>/dev/null || true
#kill $HTTP_PID 2>/dev/null || true
#kill $PORTFORWARD_PID 2>/dev/null || true
#kubectl delete pod kftray-server -n $NAMESPACE
#kubectl delete pod curl-test -n $NAMESPACE
#log_success "Cleanup completed"

log_success "Test complete"

# Add authentication test cases
log_info "Testing SSH authentication..."

# Test with valid key
log_info "Testing with valid SSH key..."
ssh -i "$SSH_KEY_FILE" -p 2222 -o StrictHostKeyChecking=no localhost exit
if [ $? -eq 0 ]; then
    log_success "Authentication successful with valid key"
else
    log_error "Authentication failed with valid key"
    exit 1
fi

# Test with invalid key
log_info "Testing with invalid SSH key..."
ssh-keygen -t ed25519 -f "/tmp/invalid_key" -N "" -C "invalid@test.com" > /dev/null 2>&1
if ssh -i "/tmp/invalid_key" -p 2222 -o StrictHostKeyChecking=no localhost exit 2>/dev/null; then
    log_error "Authentication succeeded with invalid key (should fail)"
    exit 1
else
    log_success "Authentication correctly rejected invalid key"
fi

# Add cleanup for SSH keys
log_info "Cleaning up SSH keys..."
rm -rf "$SSH_KEY_DIR" "/tmp/invalid_key" "/tmp/invalid_key.pub"

# Cleanup section
cleanup() {
    log_info "Cleaning up..."
    kill $SSH_PID 2>/dev/null || true
    kill $HTTP_PID 2>/dev/null || true
    kill $PORTFORWARD_PID 2>/dev/null || true
    kubectl delete pod kftray-server -n $NAMESPACE 2>/dev/null || true
    kubectl delete pod curl-test -n $NAMESPACE 2>/dev/null || true
    rm -rf "$SSH_KEY_DIR" "/tmp/invalid_key" "/tmp/invalid_key.pub" 2>/dev/null || true
    log_success "Cleanup completed"
}

trap cleanup EXIT

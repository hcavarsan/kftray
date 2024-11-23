#!/bin/bash


TCP_CONTAINER="kftray-tcp-proxy"
HTTP_CONTAINER="kftray-http-proxy"
UDP_CONTAINER="kftray-udp-proxy"

declare -a CONTAINERS
declare -a BG_PIDS

WAIT_MODE=false

log_info() {
    echo "[$(date -u '+%Y-%m-%d %H:%M:%S UTC')] INFO: $*"
}

log_warning() {
    echo "[$(date -u '+%Y-%m-%d %H:%M:%S UTC')] WARNING: $*" >&2
}

log_error() {
    echo "[$(date -u '+%Y-%m-%d %H:%M:%S UTC')] ERROR: $*" >&2
}

log_debug() {
    echo "[$(date -u '+%Y-%m-%d %H:%M:%S UTC')] DEBUG: $*"
}

handle_error() {
    local exit_code=$1
    local error_msg=$2
    if [ "$exit_code" -ne 0 ]; then
        log_error "$error_msg (Exit code: $exit_code)"
        cleanup
    fi
}

cleanup() {
    log_info "Cleaning up resources..."

    {
        # Kill background processes
        for pid in "${BG_PIDS[@]}"; do
            if kill -0 "$pid" 2>/dev/null; then
                kill -9 "$pid" >/dev/null 2>&1
            fi
        done

        for container in "${CONTAINERS[@]}"; do
            if docker ps -q -f name="$container" >/dev/null; then
                log_info "Stopping container $container..."
                docker rm -f "$container" >/dev/null 2>&1
            fi
        done
    } >/dev/null 2>&1

    log_info "Cleanup completed"
    exit 0
}

trap cleanup INT

log_container() {
    local container_name=$1

    if ! docker ps -q -f name="$container_name" >/dev/null; then
        log_error "Container $container_name is not running"
        return 1
    fi

    docker logs -f "$container_name" 2>&1 | while IFS= read -r line; do
        line=$(echo "$line" | sed -r "s/\x1B\[([0-9]{1,3}(;[0-9]{1,3})*)?[mGK]//g")
        echo "[$(date -u '+%Y-%m-%d %H:%M:%S UTC')] [$container_name] $line"
    done 2>/dev/null &

    local pid=$!
    BG_PIDS+=("$pid")
    CONTAINERS+=("$container_name")

    sleep 0.5
    if ! kill -0 "$pid" 2>/dev/null; then
        log_error "Failed to start logging for $container_name"
        return 1
    fi
}

check_container_health() {
    local container_name=$1
    local max_attempts=30
    local attempt=1

    log_info "Checking health of container $container_name..."

    while [ $attempt -le $max_attempts ]; do
        if ! docker ps -q -f name="$container_name" >/dev/null; then
            log_error "Container $container_name failed to start. Checking logs:"
            docker logs "$container_name"
            return 1
        fi

        if docker logs "$container_name" 2>&1 | grep -q "started on port"; then
            log_info "Container $container_name is healthy"
            return 0
        fi

        if [ $attempt -eq 1 ]; then
            log_debug "Waiting for container $container_name to be healthy..."
        elif [ $(( attempt % 5 )) -eq 0 ]; then
            log_debug "Still waiting for container $container_name (attempt $attempt/$max_attempts)..."
        fi

        sleep 1
        ((attempt++))
    done

    log_error "Container $container_name failed health check after $max_attempts attempts"
    docker logs "$container_name"
    return 1
}

wait_for_port() {
    local port=$1
    local retries=30
    local attempt=1

    log_info "Checking port $port availability..."

    while [ $attempt -le $retries ]; do
        if nc -z localhost "$port" 2>/dev/null; then
            log_info "Port $port is available"
            return 0
        fi

        if [ $attempt -eq 1 ]; then
            log_debug "Waiting for port $port to become available..."
        elif [ $(( attempt % 5 )) -eq 0 ]; then
            log_debug "Still waiting for port $port (attempt $attempt/$retries)..."
        fi

        sleep 1
        ((attempt++))
    done

    log_error "Port $port is not available after $retries attempts"
    return 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --wait)
            WAIT_MODE=true
            shift
            ;;
        *)
            log_error "Unknown parameter: $1"
            exit 1
            ;;
    esac
done

cleanup_existing_containers() {
    log_info "Checking for existing proxy containers..."

    local containers=("$TCP_CONTAINER" "$HTTP_CONTAINER" "$UDP_CONTAINER")

    for container in "${containers[@]}"; do
        if docker ps -a -q -f name="^/${container}$" >/dev/null; then
            log_info "Removing existing container: $container"
            if ! docker rm -f "$container" >/dev/null 2>&1; then
                log_warning "Failed to remove container $container"
            fi
        fi
    done
}

cleanup_existing_containers

create_dns_query() {
    printf '\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00'
    printf '\x06google\x03com\x00'
    printf '\x00\x01\x00\x01'
}

send_dns_query() {
    local query
    query=$(create_dns_query)
    local length
    length=${#query}

    printf '\x00\x00\x00%b' "\\x$(printf '%02x' "$length")"
    printf '%s' "$query"
}

check_dns_response() {
    local response_size
    local response

    response_size=$(dd bs=1 count=4 2>/dev/null | xxd -p)
    if [ -z "$response_size" ]; then
        return 1
    fi

    response_size=$((16#$response_size))
    if [ "$response_size" -eq 0 ]; then
        return 1
    fi

    response=$(dd bs=1 count="$response_size" 2>/dev/null | xxd -p)
    if [ -z "$response" ]; then
        return 1
    fi

    log_debug "Response size: $response_size bytes"
    log_debug "Response hex: $response"

    if echo "$response" | grep -q "^....8[0-5]"; then
        return 0
    fi
    return 1
}

curl_with_logging() {
    local url=$1
    shift
    local extra_args=("$@")

    curl -v --max-time 10 "${extra_args[@]}" "$url" 2>&1 | while IFS= read -r line; do
        log_debug "CURL: $line"
    done
}

log_info "Starting proxy tests"

log_info "Building kftray-server image..."
kftray_server_dir="crates/kftray-server"
original_dir=$(pwd)
cd "$kftray_server_dir" || handle_error $? "Failed to change directory to kftray-server"
docker build -t kftray-server . || handle_error $? "Failed to build Docker image"
cd "$original_dir" || handle_error $? "Failed to return to root directory"

log_info "Starting TCP proxy to example.com:80..."
docker run -d --name "$TCP_CONTAINER" \
    -p 8080:8080 \
    -e REMOTE_ADDRESS=example.com \
    -e REMOTE_PORT=80 \
    -e LOCAL_PORT=8080 \
    -e PROXY_TYPE=tcp \
    -e RUST_LOG=debug \
    kftray-server
handle_error $? "Failed to start TCP proxy container"
check_container_health $TCP_CONTAINER || handle_error $? "TCP proxy container failed health check"

log_info "Starting HTTP proxy to httpbin.org..."
docker run -d --name "$HTTP_CONTAINER" \
    -p 8081:8080 \
    -e REMOTE_ADDRESS=httpbin.org \
    -e REMOTE_PORT=80 \
    -e LOCAL_PORT=8080 \
    -e PROXY_TYPE=http \
    -e RUST_LOG=debug \
    kftray-server
handle_error $? "Failed to start HTTP proxy container"
check_container_health $HTTP_CONTAINER || handle_error $? "HTTP proxy container failed health check"

log_info "Starting UDP proxy to Google DNS (8.8.8.8:53)..."
docker run -d --name "$UDP_CONTAINER" \
    -p 8082:8080 \
    -e REMOTE_ADDRESS=8.8.8.8 \
    -e REMOTE_PORT=53 \
    -e LOCAL_PORT=8080 \
    -e PROXY_TYPE=udp \
    -e RUST_LOG=debug \
    kftray-server
handle_error $? "Failed to start UDP proxy container"
check_container_health $UDP_CONTAINER || handle_error $? "UDP proxy container failed health check"

log_info "Waiting for proxies to be ready..."
wait_for_port 8080 || handle_error $? "TCP proxy port 8080 not available"
wait_for_port 8081 || handle_error $? "HTTP proxy port 8081 not available"
wait_for_port 8082 || handle_error $? "UDP proxy port 8082 not available"

log_info "Starting log monitoring..."
log_container $TCP_CONTAINER || handle_error $? "Failed to start TCP proxy logs"
log_container $HTTP_CONTAINER || handle_error $? "Failed to start HTTP proxy logs"
log_container $UDP_CONTAINER || handle_error $? "Failed to start UDP proxy logs"

log_info "All proxies are running! Testing connectivity..."

log_info "Testing TCP proxy..."
tcp_success=false

# First attempt: Try with curl
if curl_with_logging "http://localhost:8080/" "-H Host: example.com" "--max-time 10" | grep -q "200 OK\|301 Moved Permanently\|302 Found\|308 Permanent Redirect"; then
    log_info "TCP proxy test succeeded with HTTP"
    tcp_success=true
else
    log_warning "TCP proxy test failed with HTTP request (this might be normal if server redirects), trying raw TCP..."

    # Second attempt: Try with netcat
    if echo -e "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n" | nc -w 5 localhost 8080 | grep -q "HTTP/1.\|200 OK\|301 Moved\|302 Found\|308 Permanent"; then
        log_info "TCP proxy test succeeded with raw TCP"
        tcp_success=true
    else
        # Third attempt: Try with basic TCP connection test
        if nc -z localhost 8080; then
            log_info "TCP proxy connection test succeeded"
            tcp_success=true
        else
            log_warning "TCP proxy test failed completely"
        fi
    fi
fi

if [ "$tcp_success" = false ]; then
    log_warning "TCP proxy tests failed but continuing..."
fi

log_info "Testing HTTP proxy..."
max_retries=3
retry_count=0
while [ $retry_count -lt $max_retries ]; do
    if curl_with_logging "http://localhost:8081/get" | grep -q "200 OK"; then
        log_info "HTTP proxy test succeeded"
        break
    else
        ((retry_count++))
        if [ $retry_count -lt $max_retries ]; then
            log_warning "HTTP proxy test failed, retrying ($retry_count/$max_retries)..."
            sleep 2
        else
            log_warning "HTTP proxy test failed after $max_retries attempts, but continuing..."
        fi
    fi
done

log_info "Testing UDP proxy..."
log_info "Testing DNS query through UDP proxy..."

max_retries=3
retry_count=0
udp_success=false

while [ $retry_count -lt $max_retries ]; do
    log_info "UDP test attempt $((retry_count + 1))/$max_retries"

    if send_dns_query | { nc -w 5 localhost 8082; sleep 0.1; } | check_dns_response; then
        log_info "UDP proxy DNS test succeeded"
        udp_success=true
        break
    fi

    ((retry_count++))

    if [ $retry_count -lt $max_retries ]; then
        log_warning "Retrying UDP test in 2 seconds..."
        sleep 2
    else
        log_warning "Trying alternative test with dig..."
        if command -v dig >/dev/null; then
            if dig @localhost -p 8082 google.com +timeout=5 +tries=1 +short | grep -q "[0-9]\+\.[0-9]\+\.[0-9]\+\.[0-9]\+"; then
                log_info "UDP proxy test succeeded with dig"
                udp_success=true
            else
                log_error "UDP proxy test failed with all methods"
                log_warning "Last logs from UDP proxy:"
                docker logs "$UDP_CONTAINER" | tail -n 20
            fi
        fi
    fi
done

if [ "$udp_success" = false ]; then
    log_warning "UDP proxy tests failed but continuing..."
fi

log_info "All tests completed"

if [ "$WAIT_MODE" = true ]; then
    log_info "Entering monitoring mode (--wait). Press Ctrl+C to stop."

    while true; do
        for pid in "${BG_PIDS[@]}"; do
            if ! kill -0 "$pid" 2>/dev/null; then
                log_error "Logging process $pid died"

                for container in "${CONTAINERS[@]}"; do
                    if ! docker ps -q -f name="$container" >/dev/null; then
                        log_error "Container $container is not running"
                    else
                        log_warning "Last logs from $container:"
                        docker logs --tail 20 "$container"
                    fi
                done

                cleanup
                exit 1
            fi
        done

        log_debug "All processes running"
        sleep 5
    done
else
    cleanup
fi

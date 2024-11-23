#!/bin/bash

START_TIME=$(date +%s)

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
		elif [ $((attempt % 5)) -eq 0 ]; then
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
		elif [ $((attempt % 5)) -eq 0 ]; then
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
	local temp_file
	temp_file=$(mktemp)
	local headers_file
	headers_file=$(mktemp)

	if curl -v --max-time 10 -D "$headers_file" "${extra_args[@]}" "$url" >"$temp_file" 2>&1; then
		local curl_status=$?

		local status_code
		status_code=$(head -n1 "$headers_file" | grep -oE '[0-9]{3}' | head -n1)

		{
			local first_header
			first_header=$(head -n1 "$headers_file")
			log_info "HTTP Response Status: $first_header"

			while IFS= read -r line; do
				if echo "$line" | grep -qi "^content-\|^location:\|^server:\|^date:"; then
					log_info "HTTP Header: $line"
				fi
			done <"$headers_file"

			while IFS= read -r line; do
				if echo "$line" | grep -q "^* Connected to\|^* HTTP\|^* Connection.*closed"; then
					log_info "CURL: $line"
				fi
			done <"$temp_file"
		} >&2

		rm -f "$temp_file" "$headers_file"

		if [ -n "$status_code" ]; then
			printf "%s" "$status_code"
		fi

		return $curl_status
	else
		local curl_status=$?
		log_error "Curl failed with status $curl_status" >&2
		log_error "Full curl output:" >&2
		cat "$temp_file" >&2
		rm -f "$temp_file" "$headers_file"
		return $curl_status
	fi
}

log_info "Starting proxy test suite"
log_info "Phase 1: Building server image"
log_info "Phase 2: Starting proxy containers"
log_info "Phase 3: Verifying container health"
log_info "Phase 4: Testing proxy connectivity"
log_info "Phase 5: Running individual proxy tests"

log_info "Building kftray-server image..."
kftray_server_dir="crates/kftray-server"
original_dir=$(pwd)
cd "$kftray_server_dir" || handle_error $? "Failed to change directory to kftray-server"
docker build -t kftray-server . || handle_error $? "Failed to build Docker image"
cd "$original_dir" || handle_error $? "Failed to return to root directory"

log_info "Starting TCP proxy to httpbin.org:80..."
docker run -d --name "$TCP_CONTAINER" \
	-p 8080:8080 \
	-e REMOTE_ADDRESS=httpbin.org \
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
tcp_http_status="FAILED"
tcp_connect_status="FAILED"
tcp_attempts=0
max_retries=3
retry_count=0

log_info "TCP Proxy Test: HTTP Request"
while [ "$retry_count" -lt "$max_retries" ]; do
	((tcp_attempts++))
	log_info "TCP HTTP test attempt $((retry_count + 1))/$max_retries"

	if ! docker ps -q -f name="$TCP_CONTAINER" >/dev/null; then
		log_error "TCP proxy container is not running!"
		docker logs "$TCP_CONTAINER"
		break
	fi

	log_info "Sending HTTP request to TCP proxy..."
	response=$(curl_with_logging "http://localhost:8080/" "-H" "Host: httpbin.org" "--fail" "-s")
	curl_status=$?

	if [ $curl_status -eq 0 ]; then
		log_info "Received HTTP status code: $response"

		if [[ "$response" =~ ^(200|301|302|308)$ ]]; then
			log_info "TCP proxy HTTP test: PASSED (Status code: $response)"
			tcp_http_status="PASSED"
			break
		else
			log_info "TCP proxy HTTP test: FAILED (Status code: $response)"
		fi
	else
		log_error "Curl command failed with exit code $curl_status"
	fi

	((retry_count++))
	if [ "$retry_count" -lt "$max_retries" ]; then
		log_warning "Retrying TCP HTTP test in 2 seconds... ($retry_count/$max_retries)"
		log_info "Recent TCP proxy container logs:"
		docker logs --tail 10 "$TCP_CONTAINER"
		sleep 2
	fi
done

log_info "Running basic TCP connection test..."
if nc -z localhost 8080; then
	log_info "TCP proxy connection test: PASSED"
	tcp_connect_status="PASSED"
else
	log_info "TCP proxy connection test: FAILED"
fi

if [ "$tcp_http_status" = "FAILED" ] && [ "$tcp_connect_status" = "FAILED" ]; then
	log_error "All TCP proxy tests failed"
	log_error "Last TCP proxy container logs:"
	docker logs --tail 20 "$TCP_CONTAINER"
	cleanup
	exit 1
fi

log_info "Testing HTTP proxy..."
http_status="FAILED"
http_attempts=0
retry_count=0

while [ "$retry_count" -lt "$max_retries" ]; do
	((http_attempts++))
	log_info "HTTP test attempt $((retry_count + 1))/$max_retries"

	if ! docker ps -q -f name="$HTTP_CONTAINER" >/dev/null; then
		log_error "HTTP proxy container is not running!"
		docker logs "$HTTP_CONTAINER"
		break
	fi

	if docker logs "$HTTP_CONTAINER" 2>&1 | grep -i "error\|panic\|fatal" >/dev/null; then
		log_error "Found errors in HTTP proxy container logs:"
		docker logs "$HTTP_CONTAINER" | grep -i "error\|panic\|fatal"
	fi

	log_info "Sending HTTP request to proxy..."
	response=$(curl_with_logging "http://localhost:8081/get" "-H" "Host: httpbin.org" "--fail" "-s")
	curl_status=$?

	if [ $curl_status -eq 0 ]; then
		log_info "Received HTTP status code: $response"

		if [ "$response" = "200" ]; then
			log_info "HTTP proxy test: PASSED (Status code: $response)"
			http_status="PASSED"
			break
		else
			log_info "HTTP proxy test: FAILED (Status code: $response)"
		fi
	else
		log_error "Curl command failed with exit code $curl_status"
	fi

	((retry_count++))
	if [ "$retry_count" -lt "$max_retries" ]; then
		log_warning "Retrying HTTP test in 2 seconds... ($retry_count/$max_retries)"
		log_info "Recent HTTP proxy container logs:"
		docker logs --tail 10 "$HTTP_CONTAINER"
		sleep 2
	fi
done

if [ "$http_status" = "FAILED" ]; then
	log_error "HTTP proxy test failed after $max_retries attempts"
	log_error "Last HTTP proxy container logs:"
	docker logs --tail 20 "$HTTP_CONTAINER"
	cleanup
	exit 1
fi

log_info "Testing UDP proxy..."
log_info "Testing DNS query through UDP proxy..."

max_retries=3
retry_count=0
udp_dns_status="FAILED"

while [ "$retry_count" -lt "$max_retries" ]; do
	log_info "UDP test attempt $((retry_count + 1))/$max_retries"

	if send_dns_query | {
		nc -w 5 localhost 8082
		sleep 0.1
	} | check_dns_response; then
		log_info "UDP proxy DNS test: PASSED"
		udp_dns_status="PASSED"
		break
	fi

	((retry_count++))
	if [ "$retry_count" -lt "$max_retries" ]; then
		log_warning "Retrying UDP test in 2 seconds..."
		sleep 2
	fi
done

log_info "All tests completed"

print_summary() {
	local total_time=$1

	echo
	echo "=== PROXY TESTS SUMMARY ==="
	echo "Test completed at: $(date '+%Y-%m-%d %H:%M:%S')"
	echo "Total duration: ${total_time}s"
	echo
	echo "TCP Proxy (localhost:8080 -> httpbin.org:80)"
	echo "  HTTP Request Test:    $tcp_http_status"
	echo "  Basic Connect Test:   $tcp_connect_status"
	echo
	echo "HTTP Proxy (localhost:8081 -> httpbin.org:80)"
	echo "  HTTP GET Test:        $http_status ${http_attempts:+"(${http_attempts} attempts)"}"
	echo
	echo "UDP Proxy (localhost:8082 -> 8.8.8.8:53)"
	echo "  DNS Query Test:       $udp_dns_status"
	echo "=================================="
	echo
}

END_TIME=$(date +%s)
TOTAL_TIME=$((END_TIME - START_TIME))

log_info "All tests completed successfully"
print_summary $TOTAL_TIME

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

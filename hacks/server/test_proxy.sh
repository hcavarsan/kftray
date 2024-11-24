#!/bin/bash

readonly TCP_CONTAINER="kftray-tcp-proxy"
readonly UDP_CONTAINER="kftray-udp-proxy"
readonly MAX_RETRIES=3
readonly HEALTH_CHECK_TIMEOUT=30
export LOG_PREFIX
LOG_PREFIX="[$(date -u '+%Y-%m-%d %H:%M:%S UTC')]"

declare -a CONTAINERS
declare -a BG_PIDS
declare -a PIPES
START_TIME=$(date +%s)
WAIT_MODE=false

TCP_TEST_RESULT="SKIPPED"
UDP_TEST_RESULT="SKIPPED"

log_info() { echo "$LOG_PREFIX INFO: $*" | sed 's/\x1B\[[0-9;]*[JKmsu]//g'; }
log_warning() { echo "$LOG_PREFIX WARNING: $*" >&2 | sed 's/\x1B\[[0-9;]*[JKmsu]//g'; }
log_error() { echo "$LOG_PREFIX ERROR: $*" >&2 | sed 's/\x1B\[[0-9;]*[JKmsu]//g'; }
log_debug() { echo "$LOG_PREFIX DEBUG: $*" | sed 's/\x1B\[[0-9;]*[JKmsu]//g'; }

handle_error() {
	local exit_code=$1
	local error_msg=$2
	if [ "$exit_code" -ne 0 ]; then
		log_error "$error_msg (Exit code: $exit_code)"
		cleanup
		exit 1
	fi
}

setup_containers() {
	log_info "Building kftray-server image..."
	(cd "crates/kftray-server" && docker build -t kftray-server . >/dev/null 2>&1) ||
		handle_error $? "Failed to build Docker image"

	start_proxy_container "$TCP_CONTAINER" "tcp" "httpbin.org" "80" "8080"
	start_proxy_container "$UDP_CONTAINER" "udp" "8.8.8.8" "53" "8082"
}

start_proxy_container() {
	local container_name=$1
	local proxy_type=$2
	local remote_addr=$3
	local remote_port=$4
	local local_port=$5

	log_info "Starting $proxy_type proxy to $remote_addr:$remote_port..."
	docker run -d --name "$container_name" \
		-p "$local_port:8080" \
		-e REMOTE_ADDRESS="$remote_addr" \
		-e REMOTE_PORT="$remote_port" \
		-e LOCAL_PORT=8080 \
		-e PROXY_TYPE="$proxy_type" \
		-e RUST_LOG=debug \
		kftray-server >/dev/null

	handle_error $? "Failed to start $proxy_type proxy container"
	check_container_health "$container_name"
	setup_container_logging "$container_name"
}

check_container_health() {
	local container_name=$1
	local attempt=1

	while [ $attempt -le $HEALTH_CHECK_TIMEOUT ]; do
		if ! docker ps -q -f name="$container_name" >/dev/null; then
			log_error "Container $container_name failed to start"
			docker logs "$container_name"
			return 1
		fi

		if docker logs "$container_name" 2>&1 | grep -q "started on port"; then
			log_info "Container $container_name is healthy"
			return 0
		fi

		[ $attempt -eq 1 ] && log_debug "Waiting for container $container_name..."
		[ $((attempt % 5)) -eq 0 ] && log_debug "Still waiting... (attempt $attempt/$HEALTH_CHECK_TIMEOUT)"

		sleep 1
		((attempt++))
	done

	log_error "Container $container_name health check failed"
	docker logs "$container_name"
	return 1
}

setup_container_logging() {
	local container_name=$1
	local log_file="/tmp/${container_name}.log"
	local pipe="/tmp/${container_name}.pipe"
	mkfifo "$pipe" 2>/dev/null

	{ docker logs -f "$container_name" >"$pipe"; } 2>&1 &

	(while read -r line; do
		local formatted_line
		formatted_line=${line//$'\x1B'[\[0-9;]*[JKmsu]//}
		echo "[$container_name] $formatted_line" >>"$log_file"
	done <"$pipe" >/dev/null 2>&1 &)

	PIPES+=("$pipe")
	CONTAINERS+=("$container_name")
}

create_dns_query() {
	printf '\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00'
	printf '\x06google\x03com\x00'
	printf '\x00\x01\x00\x01'
}

send_dns_query() {
	local query
	query=$(create_dns_query)
	local length=${#query}
	printf '\x00\x00\x00%b' "\\x$(printf '%02x' "$length")"
	printf '%s' "$query"
}

check_dns_response() {
	local response_size
	local response

	response_size=$(dd bs=1 count=4 2>/dev/null | xxd -p)
	[ -z "$response_size" ] && return 1

	response_size=$((16#$response_size))
	[ "$response_size" -eq 0 ] && return 1

	response=$(dd bs=1 count="$response_size" 2>/dev/null | xxd -p)
	[ -z "$response" ] && return 1

	if echo "$response" | grep -q "^....8[0-5]"; then
		return 0
	fi
	return 1
}

run_tcp_tests() {
	log_info "Running TCP proxy tests..."
	local status_code
	local response

	for i in $(seq 1 $MAX_RETRIES); do
		log_info "TCP test attempt $i/$MAX_RETRIES"

		# Use -i to show request headers
		response=$(curl -i -v -s -H "Host: httpbin.org" http://localhost:8080/get 2>&1)
		status_code=$?

		log_debug "Curl exit code: $status_code"
		log_debug "Full response:"
		log_debug "$response"

		if echo "$response" | grep -q '"url": "http://httpbin.org/get"'; then
			TCP_TEST_RESULT="PASSED"
			return 0
		fi

		[ "$i" -lt $MAX_RETRIES ] && sleep 2
	done

	TCP_TEST_RESULT="FAILED"
	return 1
}



run_udp_tests() {
	log_info "Running UDP proxy tests..."

	for i in $(seq 1 $MAX_RETRIES); do
		log_info "UDP test attempt $i/$MAX_RETRIES"

		if send_dns_query | nc -w 5 localhost 8082 | check_dns_response; then
			UDP_TEST_RESULT="PASSED"
			return 0
		fi

		[ "$i" -lt $MAX_RETRIES ] && sleep 2
	done

	UDP_TEST_RESULT="FAILED"
	return 1
}

cleanup_logs() {
	for container in "${CONTAINERS[@]}"; do
		rm -f "/tmp/${container}.log" 2>/dev/null
	done
}

cleanup() {
	log_info "Cleaning up resources..." >/dev/null

	for container in "${CONTAINERS[@]}"; do
		if docker ps -q -f name="$container" >/dev/null 2>&1; then
			docker rm -f "$container" >/dev/null 2>&1
		fi
	done

	for pipe in "${PIPES[@]}"; do
		rm -f "$pipe" >/dev/null 2>&1
	done

	cleanup_logs >/dev/null 2>&1

	exit 0
}

print_summary() {
	local end_time
	end_time=$(date +%s)
	local total_time=$((end_time - START_TIME))

	echo
	echo "=== PROXY TESTS SUMMARY ==="
	echo "Test completed at: $(date '+%Y-%m-%d %H:%M:%S')"
	echo "Total duration: ${total_time}s"
	echo
	echo "TCP Proxy (localhost:8080 -> httpbin.org:80): $TCP_TEST_RESULT"
	echo "UDP Proxy (localhost:8082 -> 8.8.8.8:53): $UDP_TEST_RESULT"
	echo "=================================="

	if [[ "$TCP_TEST_RESULT" == "FAILED" ||  "$UDP_TEST_RESULT" == "FAILED" ]]; then
		echo
		echo "=== DETAILED LOGS FOR FAILED TESTS ==="
		for container in "${CONTAINERS[@]}"; do
			if [[ -f "/tmp/${container}.log" ]]; then
				echo
				echo "Last 10 lines from $container:"
				tail -n 10 "/tmp/${container}.log"
			fi
		done
		echo "=================================="
	fi
	echo
}

main() {
	trap 'cleanup >/dev/null 2>&1' INT TERM EXIT

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

	setup_containers
	run_tcp_tests
	run_udp_tests

	print_summary

	if [ "$WAIT_MODE" = true ]; then
		log_info "Entering monitoring mode. Press Ctrl+C to stop."
		while true; do
			for pid in "${BG_PIDS[@]}"; do
				if ! kill -0 "$pid" 2>/dev/null; then
					log_error "Logging process $pid died"
					cleanup
					exit 1
				fi
			done
			sleep 5
		done
	else
		cleanup
	fi
}

main "$@"

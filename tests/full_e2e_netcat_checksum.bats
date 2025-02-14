#!/usr/bin/env bats

load tools.bats

ensure_deps() {
    check_command cargo
    check_command nc
    check_command md5sum
}

setup() {
    ensure_deps

    LOG_NAME="prrs_full_e2e_netcat_checksum"

    # Create log files base dir
    mkdir -p ./testlogs
    LOG_DIR=$(mktemp -p ./testlogs -d "${LOG_NAME}_$(date +%Y%m%d-%H%M%S).XXXXXX")

    # Build the project
    cargo build

    # Start portredirect server in background
    RUST_BACKTRACE=1 RUST_LOG=tracing=debug ./target/debug/portredirect_server \
        --local-host 127.0.0.1 --local-port 10002 \
        --quic-server-host 127.0.0.1 --quic-server-port 4433 --quic-psk ilovespezifisch \
        >"$LOG_DIR/portredirect_server.log" 2>&1 &
    SERVER_PID=$!

    # Wait a short time for the server to be ready
    sleep 1

    # Start portredirect client in background
    RUST_BACKTRACE=1 RUST_LOG=tracing=debug ./target/debug/portredirect_client \
        --destination-host 127.0.0.1 --destination-port 1234 \
        --quic-remote-host 127.0.0.1 --quic-remote-port 4433 \
        --quic-remote-hostname-match localhost --quic-psk ilovespezifisch \
        --provide-metrics \
        >"$LOG_DIR/portredirect_client.log" 2>&1 &
    CLIENT_PID=$!

    # Wait for services to start up
    sleep 5
}

teardown() {
    get_metrics "$LOG_DIR/portredirect_client_metrics.log"

    # Kill background processes
    kill $SERVER_PID $CLIENT_PID || true
}

send_and_verify() {
    local size=$1
    local filename="testfile_${size}MB"

    # Generate random file of specified size
    head -c ${size}M </dev/urandom >$filename

    # Compute original MD5 hash
    local original_md5=$(md5sum "$filename" | awk '{print $1}')

    # Start netcat listener on port 5201 (bridged by portredirect)
    nc -l -p 1234 >received_$filename &
    sleep 1 # Allow listener to start

    # Send the file via netcat to port 10002, which is redirected to 5201
    cat "$filename" | nc -N 127.0.0.1 10002
    sleep 1 # Allow data to be received

    # Compute received file MD5 hash
    local received_md5=$(md5sum "received_$filename" | awk '{print $1}')

    rm "$filename"
    pkill -f "nc -l -p 1234" || true

    # Compare hashes
    [ "$original_md5" = "$received_md5" ]
}

@test "Netcat data integrity test (1MB)" {
    send_and_verify 1
}

@test "Netcat data integrity test (100MB)" {
    send_and_verify 100
}

@test "Netcat repeated data integrity test (1MB x 100)" {
    for i in {1..100}; do
        send_and_verify 1
    done
}

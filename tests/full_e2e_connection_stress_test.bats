#!/usr/bin/env bats

load tools.bats

setup() {
    LOG_NAME="prrs_full_e2e_connection_stress_test"

    # Create log files base dir
    mkdir -p ./testlogs
    LOG_DIR=$(mktemp -p ./testlogs -d "${LOG_NAME}_$(date +%Y%m%d-%H%M%S).XXXXXX")

    # Build the project
    cargo build

    # Start portredirect server in background
    RUST_BACKTRACE=1 RUST_LOG=tracing=debug ./target/debug/portredirect_server \
        --local-host 127.0.0.1 --local-port 10003 \
        --quic-server-host 127.0.0.1 --quic-server-port 4433 --quic-psk ilovespezifisch \
        >"$LOG_DIR/portredirect_server.log" 2>&1 &
    SERVER_PID=$!

    # Wait a short time for the server to be ready
    sleep 1

    # Start portredirect client in background
    RUST_BACKTRACE=1 RUST_LOG=tracing=debug ./target/debug/portredirect_client \
        --destination-host 127.0.0.1 --destination-port 5201 \
        --quic-remote-host 127.0.0.1 --quic-remote-port 4433 \
        --quic-remote-hostname-match localhost --quic-psk ilovespezifisch \
        --provide-metrics \
        >"$LOG_DIR/portredirect_client.log" 2>&1 &
    CLIENT_PID=$!

    # Wait for services to start up
    sleep 1
}

teardown() {
    get_metrics "$LOG_DIR/portredirect_client_metrics.log"

    kill $SERVER_PID $CLIENT_PID || true
}

@test "Baseline test (direct connection)" {
    # make the benchmark connect to itself
    run python3 ./tests/connection_stress_test.py --server-port 1234 --listener-port 1234
    [ "$status" -eq 0 ]
}

@test "Tunneled test (via portredirect)" {
    run python3 ./tests/connection_stress_test.py --server-port 10003 --listener-port 5201
    [ "$status" -eq 0 ]

    # Check that no ERROR occurred in the portredirect logs
    if grep -q "ERROR" "$LOG_DIR/portredirect_server.log"; then
        echo "ERROR found in server log"
        exit 1
    fi
    if grep -q "ERROR" "$LOG_DIR/portredirect_client.log"; then
        echo "ERROR found in client log"
        exit 1
    fi
}

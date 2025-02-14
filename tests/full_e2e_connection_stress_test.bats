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

pr_log_error_check() {
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

@test "Baseline test (direct connection)" {
    # make the benchmark connect to itself,
    # use one big transfer of 1 GiB
    run python3 ./tests/connection_stress_test.py --server-port 1234 --listener-port 1234 \
        --log-file "$LOG_DIR/cst_baseline_1c_1gb.log" \
        --workers 1 \
        --total-bytes $((1 * 1024 ** 3))
    [ "$status" -eq 0 ]

    # 10 workers, 100 MiB each, up/down each
    run python3 ./tests/connection_stress_test.py --server-port 1234 --listener-port 1234 \
        --log-file "$LOG_DIR/cst_baseline_10c_100mb.log" \
        --workers 10 \
        --total-bytes $((100 * 1024 ** 2))
    [ "$status" -eq 0 ]

    # 50 workers, 20 MiB
    run python3 ./tests/connection_stress_test.py --server-port 1234 --listener-port 1234 \
        --log-file "$LOG_DIR/cst_baseline_50c_20mb.log" \
        --workers 50 \
        --total-bytes $((20 * 1024 ** 2))
    [ "$status" -eq 0 ]

    # 100 workers, 10 MiB
    run python3 ./tests/connection_stress_test.py --server-port 1234 --listener-port 1234 \
        --log-file "$LOG_DIR/cst_baseline_100c_10mb.log" \
        --workers 100 \
        --total-bytes $((10 * 1024 ** 2))
    [ "$status" -eq 0 ]
}

@test "Tunneled test (1 single connection, 1 GiB)" {
    run python3 ./tests/connection_stress_test.py --server-port 10003 --listener-port 5201 \
        --log-file "$LOG_DIR/cst_benchmark_1c_1gb.log" \
        --workers 1 \
        --total-bytes $((1 * 1024 ** 3))
    [ "$status" -eq 0 ]

    # Check that no ERROR occurred in the portredirect logs
    run pr_log_error_check
    [ "$status" -eq 0 ]
}

@test "Tunneled test (10 connections, 100 MiB)" {
    run python3 ./tests/connection_stress_test.py --server-port 10003 --listener-port 5201 \
        --log-file "$LOG_DIR/cst_benchmark_10c_100mb.log" \
        --workers 10 \
        --total-bytes $((100 * 1024 ** 2))
    [ "$status" -eq 0 ]

    run pr_log_error_check
    [ "$status" -eq 0 ]
}

@test "Tunneled test (50 connections, 20 MiB)" {
    run python3 ./tests/connection_stress_test.py --server-port 10003 --listener-port 5201 \
        --log-file "$LOG_DIR/cst_benchmark_50c_20mb.log" \
        --workers 50 \
        --total-bytes $((20 * 1024 ** 2))
    [ "$status" -eq 0 ]

    run pr_log_error_check
    [ "$status" -eq 0 ]
}

@test "Tunneled test (100 connections, 10 MiB)" {
    run python3 ./tests/connection_stress_test.py --server-port 10003 --listener-port 5201 \
        --log-file "$LOG_DIR/cst_benchmark_100c_10mb.log" \
        --workers 100 \
        --total-bytes $((10 * 1024 ** 2))
    [ "$status" -eq 0 ]

    run pr_log_error_check
    [ "$status" -eq 0 ]
}

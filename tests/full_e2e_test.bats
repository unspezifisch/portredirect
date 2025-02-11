#!/usr/bin/env bats
# This test is the same as the benchmark test, but without the baseline comparison and without the retries check.

setup() {
  cargo build

  # Start portredirect server in background
  RUST_BACKTRACE=1 RUST_LOG=tracing=debug cargo run --bin portredirect_server -- \
    --local-host 127.0.0.1 --local-port 10001 \
    --quic-server-host 127.0.0.1 --quic-server-port 4433 --quic-psk ilovespezifisch \
    >server.log 2>&1 &
  SERVER_PID=$!

  # Start portredirect client in background
  RUST_BACKTRACE=1 RUST_LOG=tracing=debug cargo run --bin portredirect_client -- \
    --destination-host 127.0.0.1 --destination-port 5201 \
    --quic-remote-host 127.0.0.1 --quic-remote-port 4433 \
    --quic-remote-hostname-match localhost --quic-psk ilovespezifisch \
    >client.log 2>&1 &
  CLIENT_PID=$!

  # Start iperf3 server in background
  iperf3 -s >iperf_server.log 2>&1 &
  IPERF_PID=$!

  # Wait for services to start up
  sleep 5
}

teardown() {
  kill $SERVER_PID $CLIENT_PID $IPERF_PID || true
}

@test "Tunneled iperf3 test (via portredirect)" {
  run iperf3 -c 127.0.0.1 -p 10001
  [ "$status" -eq 0 ]
}

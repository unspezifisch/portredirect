#!/usr/bin/env bats

setup() {
  # Start portredirect server in background
  RUST_LOG=tracing=debug cargo run --bin portredirect_server -- \
    --local-host 127.0.0.1 --local-port 10001 \
    --quic-server-host 127.0.0.1 --quic-server-port 4433 --quic-psk ilovespezifisch \
    > server.log 2>&1 &
  SERVER_PID=$!

  # Start portredirect client in background
  RUST_BACKTRACE=1 RUST_LOG=tracing=debug cargo run --bin portredirect_client -- \
    --destination-host 127.0.0.1 --destination-port 5201 \
    --quic-remote-host 127.0.0.1 --quic-remote-port 4433 \
    --quic-remote-hostname-match localhost --quic-psk ilovespezifisch \
    > client.log 2>&1 &
  CLIENT_PID=$!

  # Start iperf3 server in background
  iperf3 -s > iperf_server.log 2>&1 &
  IPERF_PID=$!

  # Wait for services to start up
  sleep 5
}

teardown() {
  kill $SERVER_PID $CLIENT_PID $IPERF_PID || true
}

@test "Baseline iperf3 test (direct connection)" {
  run iperf3 -c 127.0.0.1 -p 5201
  [ "$status" -eq 0 ]
  # Expect zero packet loss. Adjust the grep according to the iperf3 version output.
  run grep -q "0% packet loss" <<< "$output"
  [ "$status" -eq 0 ]
}

@test "Tunneled iperf3 test (via portredirect) shows no retries" {
  run iperf3 -c 127.0.0.1 -p 10001
  [ "$status" -eq 0 ]
  # Check for 0% packet loss or no retries in the output.
  run grep -q "0% packet loss" <<< "$output"
  [ "$status" -eq 0 ]
  run grep -q "0 retries" <<< "$output"
  [ "$status" -eq 0 ]
}


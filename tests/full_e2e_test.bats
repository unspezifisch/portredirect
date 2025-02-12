#!/usr/bin/env bats
# This test checks that portredirect runs and doesn't blatantly crash or exits with an error.

setup() {
  cargo build

  # Start portredirect server in background
  RUST_BACKTRACE=1 RUST_LOG=tracing=debug ./target/debug/portredirect_server \
    --local-host 127.0.0.1 --local-port 10001 \
    --quic-server-host 127.0.0.1 --quic-server-port 4433 --quic-psk ilovespezifisch \
    &
  SERVER_PID=$!

  # Wait a short time for the server to be ready
  sleep 1

  # Start portredirect client in background
  RUST_BACKTRACE=1 RUST_LOG=tracing=debug ./target/debug/portredirect_client \
    --destination-host 127.0.0.1 --destination-port 5201 \
    --quic-remote-host 127.0.0.1 --quic-remote-port 4433 \
    --quic-remote-hostname-match localhost --quic-psk ilovespezifisch \
    &
  CLIENT_PID=$!

  # Start iperf3 server in background
  iperf3 -s >iperf_server.log &
  IPERF_PID=$!

  # Wait for services to start up
  sleep 5
}

teardown() {
  kill $SERVER_PID $CLIENT_PID $IPERF_PID || true
}

@test "Tunneled iperf3 test (via portredirect)" {
  run iperf3 -c 127.0.0.1 -p 10001 -l 5
  [ "$status" -eq 0 ]
  run iperf3 -c 127.0.0.1 -p 10001 -l 5
  [ "$status" -eq 0 ]
}

#!/usr/bin/env bats
# This is a benchmark test that shows the performance of a direct connection and a tunneled connection for comparison.
# It ensures that all commands run successfully and that there are no transmission errors.
#
# Here are diagrams of the two connection setups:
#          Baseline Test
#  ------------------------------------------------------
# |                                                      |
# |   iperf3 Client              iperf3 Server           |
# |   (connect to port 5201)  -->  (listening on 5201)   |
# |                                                      |
#  ------------------------------------------------------
#          Tunneled Test
#  --------------------------------------------------------------
# |                                                              |
# |   iperf3 Client                                              |
# |   (connects to port 10001)                                   |
# |          |                                                   |
# |          v                                                   |
# |   portredirect_server                                        |
# |   (127.0.0.1:10001)                                          |
# |          |                                                   |
# |          |  Establishes a QUIC tunnel using port 4433        |
# |          v                                                   |
# |   portredirect_client                                        |
# |   (connects via QUIC to server on port 4433)                 |
# |          |                                                   |
# |          v                                                   |
# |   iperf3 Server                                              |
# |   (listening on port 5201)                                   |
# |                                                              |
#  --------------------------------------------------------------

setup() {
  cargo build

  # Start portredirect server in background
  RUST_BACKTRACE=1 RUST_LOG=tracing=debug ./target/debug/portredirect_server \
    --local-host 127.0.0.1 --local-port 10001 \
    --quic-server-host 127.0.0.1 --quic-server-port 4433 --quic-psk ilovespezifisch \
    >server.log 2>&1 &
  SERVER_PID=$!

  # Wait a short time for the server to be ready
  sleep 1

  # Start portredirect client in background
  RUST_BACKTRACE=1 RUST_LOG=tracing=debug ./target/debug/portredirect_client \
    --destination-host 127.0.0.1 --destination-port 5201 \
    --quic-remote-host 127.0.0.1 --quic-remote-port 4433 \
    --quic-remote-hostname-match localhost --quic-psk ilovespezifisch \
    --provide-metrics \
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

@test "Baseline iperf3 test (direct connection)" {
  run iperf3 -c 127.0.0.1 -p 5201
  [ "$status" -eq 0 ]
  run iperf3 -c 127.0.0.1 -p 5201 -R
  [ "$status" -eq 0 ]
  run curl http://127.0.0.1:9898/metrics >metrics1.log
  [ "$status" -eq 0 ]
}

@test "Tunneled iperf3 test (via portredirect)" {
  run iperf3 -c 127.0.0.1 -p 10001
  [ "$status" -eq 0 ]
  run iperf3 -c 127.0.0.1 -p 10001 -R
  [ "$status" -eq 0 ]

  run curl http://127.0.0.1:9898/metrics >metrics2.log
  [ "$status" -eq 0 ]

  # Check that no ERROR occurred in the portredirect logs
  if grep -q "ERROR" server.log; then
    echo "ERROR found in server.log"
    exit 1
  fi
  if grep -q "ERROR" client.log; then
    echo "ERROR found in client.log"
    exit 1
  fi
}

@test "Tunneled iperf3 test (via portredirect) parallel heavy load test" {
  run iperf3 -c 127.0.0.1 -p 10001 -P 20
  [ "$status" -eq 0 ]
  run iperf3 -c 127.0.0.1 -p 10001 -P 20 -R
  [ "$status" -eq 0 ]

  run curl http://127.0.0.1:9898/metrics >metrics3.log
  [ "$status" -eq 0 ]

  # Check that no ERROR occurred in the portredirect logs
  if grep -q "ERROR" server.log; then
    echo "ERROR found in server.log"
    exit 1
  fi
  if grep -q "ERROR" client.log; then
    echo "ERROR found in client.log"
    exit 1
  fi
}

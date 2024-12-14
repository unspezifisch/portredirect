# PortRedirect-RS

*Glue your frontend to the backend!*

PortRedirect-RS is split into *server* and *client* side.

**Server:** Redirects incoming TCP connections (e.g. port 443) to a remote *client* through a persistent QUIC connection. Acts as a QUIC server.

**Client:** Redirects incoming server-initiated QUIC streams to a *destination* TCP host and port (e.g. `localhost:443`). Acts as a QUIC client to the QUIC server, i.e. initiates the QUIC connection.

## Testing the server

```sh
RUST_LOG=tracing=debug cargo run --bin portredirect_server -- --local-host 127.0.0.1 --local-port 12345 --quic-server-host 127.0.0.1 --quic-server-port 4433 --quic-psk supersecret --mode quic
```

## Testing the client

```sh
RUST_LOG=tracing=debug cargo run --bin portredirect_client -- --destination-host 127.0.0.1 --destination-port 20000 --quic-server-host 127.0.0.1 --quic-server-port 4433 --quic-psk supersecret
```

## Limitations

- Currently IPv4-only

## License

`GPL-3.0-only`

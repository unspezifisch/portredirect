#!/usr/bin/env python3
"""
Benchmark script for testing performance and data integrity through a tunnel.
It spawns a TCP listener (which portredirect's client side will connect to)
and concurrently launches several workers that connect to a separate TCP port
(portredirect's server side). Once a connection is established (via PR tunnel),
both sides run a bidirectional data transfer test that sends pseudorandom blocks
with a CRC32 checksum appended.

The block protocol is defined as follows:
    - For each block, a header (8 bytes) is sent:
         • 4 bytes: current block length (unsigned int, network byte order)
         • 4 bytes: CRC32 checksum computed on the block data
    - Then the block data (of the indicated length) is sent.
The receiver reads the header, then the data block, computes the checksum on the fly,
and compares it with the transmitted value. This avoids having to store a copy of
the expected data for later verification.
"""

import asyncio
import struct
import zlib
import random
import time
import logging
import click

# -----------------------------
# Global statistics variables
# -----------------------------
# Total bytes transmitted in the up (send) and down (receive) directions.
global_total_up_bytes = 0
global_total_down_bytes = 0

# Dictionaries to keep per-connection instantaneous speed info.
# Keys are connection IDs; values are dicts with keys: "last_time", "last_bytes", "current_speed".
active_up_stats = {}
active_down_stats = {}

# Global connection counter (for listener connections)
connection_counter = 0


def get_new_connection_id(prefix: str = "L") -> str:
    """Generate a unique connection ID with a given prefix (e.g. 'L' for listener)."""
    global connection_counter
    connection_counter += 1
    return f"{prefix}{connection_counter}"


# -----------------------------
# Data transfer functions
# -----------------------------


async def send_blocks(
    writer: asyncio.StreamWriter,
    total_bytes: int,
    block_size: int,
    seed: int,
    conn_id: str,
) -> None:
    """
    Generate and send pseudorandom data blocks (with CRC32 checksum header)
    until a total number of bytes have been transmitted.
    Also update global stats for the up direction.
    """
    global global_total_up_bytes, active_up_stats
    rng = random.Random(seed)
    bytes_sent = 0
    # Initialize per-connection stats for up direction.
    active_up_stats[conn_id] = {
        "last_time": time.time(),
        "last_bytes": 0,
        "current_speed": 0,
    }

    while bytes_sent < total_bytes:
        current_block_size = min(block_size, total_bytes - bytes_sent)
        data = rng.getrandbits(current_block_size * 8).to_bytes(
            current_block_size, "big"
        )
        checksum = zlib.crc32(data) & 0xFFFFFFFF
        header = struct.pack("!II", current_block_size, checksum)
        writer.write(header + data)
        await writer.drain()

        bytes_sent += current_block_size
        # Update global counter and per-connection stats.
        global_total_up_bytes += current_block_size
        now = time.time()
        stats = active_up_stats[conn_id]
        delta = bytes_sent - stats["last_bytes"]
        dt = now - stats["last_time"]
        speed = delta / dt if dt > 0 else 0
        stats["current_speed"] = speed
        stats["last_bytes"] = bytes_sent
        stats["last_time"] = now

    try:
        writer.write_eof()
    except Exception:
        pass  # Some transports do not support write_eof()


async def receive_blocks(
    reader: asyncio.StreamReader, total_bytes: int, block_size: int, conn_id: str
) -> None:
    """
    Receive data blocks (with header) until a total number of bytes have been read.
    For each block, compute the CRC32 checksum on the fly and compare it to the transmitted value.
    Also update global stats for the down direction.
    """
    global global_total_down_bytes, active_down_stats
    bytes_received = 0
    # Initialize per-connection stats for down direction.
    active_down_stats[conn_id] = {
        "last_time": time.time(),
        "last_bytes": 0,
        "current_speed": 0,
    }

    while bytes_received < total_bytes:
        header = await reader.readexactly(8)
        current_block_size, expected_checksum = struct.unpack("!II", header)
        data = await reader.readexactly(current_block_size)
        computed_checksum = zlib.crc32(data) & 0xFFFFFFFF
        if computed_checksum != expected_checksum:
            logging.error(
                "Checksum mismatch on conn %s: expected %08x, got %08x",
                conn_id,
                expected_checksum,
                computed_checksum,
            )
        bytes_received += current_block_size
        # Update global counter and per-connection stats.
        global_total_down_bytes += current_block_size
        now = time.time()
        stats = active_down_stats[conn_id]
        delta = bytes_received - stats["last_bytes"]
        dt = now - stats["last_time"]
        speed = delta / dt if dt > 0 else 0
        stats["current_speed"] = speed
        stats["last_bytes"] = bytes_received
        stats["last_time"] = now


async def exercise_connection(
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
    total_bytes: int,
    block_size: int,
    seed_send: int,
    conn_id: str,
) -> None:
    """
    Run bidirectional transmission on a connection.
    Launch send and receive tasks concurrently and log the overall speed.
    """
    peer = writer.get_extra_info("peername")
    logging.info("Starting transfer on connection %s (peer: %s)", conn_id, peer)
    start_time = time.time()
    send_task = asyncio.create_task(
        send_blocks(writer, total_bytes, block_size, seed_send, conn_id)
    )
    recv_task = asyncio.create_task(
        receive_blocks(reader, total_bytes, block_size, conn_id)
    )
    await asyncio.gather(send_task, recv_task)
    elapsed = time.time() - start_time
    mb = total_bytes / (1024 * 1024)
    logging.info(
        "Connection %s complete in %.2f s (%.2f MB/s each direction)",
        conn_id,
        elapsed,
        mb / elapsed if elapsed > 0 else 0,
    )
    # Clean up per-connection stats
    active_up_stats.pop(conn_id, None)
    active_down_stats.pop(conn_id, None)


# -----------------------------
# Worker (client) side
# -----------------------------

async def worker(
    worker_id: int, server_addr: tuple, total_bytes: int, block_size: int
) -> None:
    """
    A worker that connects to the tunnel’s server side and runs the transmission test.
    Uses the worker's unique ID as part of its connection ID.
    """
    conn_id = f"W{worker_id}"
    logging.info(
        "Worker %s: connecting to %s:%d", conn_id, server_addr[0], server_addr[1]
    )
    try:
        reader, writer = await asyncio.open_connection(*server_addr)
    except Exception as e:
        logging.exception("Worker %s: failed to connect: %s", conn_id, e)
        return

    try:
        await exercise_connection(
            reader,
            writer,
            total_bytes,
            block_size,
            seed_send=worker_id,
            conn_id=conn_id,
        )
    except Exception as e:
        logging.exception("Worker %s encountered an error: %s", conn_id, e)
    finally:
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass
        logging.info("Worker %s: connection closed", conn_id)


async def run_workers(
    num_workers: int, server_addr: tuple, total_bytes: int, block_size: int
) -> None:
    """Launch worker coroutines with a slight stagger between startups."""
    tasks = []
    for i in range(num_workers):
        tasks.append(
            asyncio.create_task(worker(i, server_addr, total_bytes, block_size))
        )
        await asyncio.sleep(0.01)
    await asyncio.gather(*tasks)


# -----------------------------
# TCP Listener (server) side
# -----------------------------


async def handle_connection(
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
    total_bytes: int,
    block_size: int,
) -> None:
    """
    Handle an incoming connection on our TCP listener.
    Generate a random seed (based on current time) and a unique connection ID.
    """
    conn_id = get_new_connection_id("L")
    peer = writer.get_extra_info("peername")
    logging.info("Accepted connection %s from %s", conn_id, peer)
    seed_send = int(time.time() * 1000) & 0xFFFFFFFF
    try:
        await exercise_connection(
            reader, writer, total_bytes, block_size, seed_send, conn_id
        )
    except Exception as e:
        logging.exception("Error handling connection %s from %s: %s", conn_id, peer, e)
    finally:
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass
        logging.info("Closed connection %s from %s", conn_id, peer)


async def tcp_listener(host: str, port: int, total_bytes: int, block_size: int) -> None:
    """
    Create a TCP listener on (host, port) and serve forever.
    Each new connection is handled by handle_connection().
    """
    server = await asyncio.start_server(
        lambda r, w: handle_connection(r, w, total_bytes, block_size), host, port
    )
    addrs = ", ".join(str(sock.getsockname()) for sock in server.sockets)
    logging.info("TCP listener running on %s", addrs)
    async with server:
        await server.serve_forever()


# -----------------------------
# Monitor for realtime stats
# -----------------------------


async def monitor_stats(
    expected_up: int, expected_down: int, update_interval: float = 1.0
) -> None:
    """
    Periodically (every update_interval seconds) compute and log realtime stats:
      - For each direction: min/max/avg instantaneous speeds (MB/s) among active connections.
      - Total bytes transmitted and percent progress.
    """
    while True:
        await asyncio.sleep(update_interval)
        # Compute up direction stats.
        speeds_up = [stats["current_speed"] for stats in active_up_stats.values()]
        if speeds_up:
            min_speed_up = min(speeds_up)
            max_speed_up = max(speeds_up)
            avg_speed_up = sum(speeds_up) / len(speeds_up)
        else:
            min_speed_up = max_speed_up = avg_speed_up = 0

        # Compute down direction stats.
        speeds_down = [stats["current_speed"] for stats in active_down_stats.values()]
        if speeds_down:
            min_speed_down = min(speeds_down)
            max_speed_down = max(speeds_down)
            avg_speed_down = sum(speeds_down) / len(speeds_down)
        else:
            min_speed_down = max_speed_down = avg_speed_down = 0

        progress_up = (global_total_up_bytes / expected_up * 100) if expected_up else 0
        progress_down = (
            (global_total_down_bytes / expected_down * 100) if expected_down else 0
        )

        logging.info(
            "UP: %.2f MB transferred (%.2f%% complete) | Speed: min: %.2f MB/s, max: %.2f MB/s, avg: %.2f MB/s || "
            "DOWN: %.2f MB transferred (%.2f%% complete) | Speed: min: %.2f MB/s, max: %.2f MB/s, avg: %.2f MB/s",
            global_total_up_bytes / (1024 * 1024),
            progress_up,
            min_speed_up / (1024 * 1024),
            max_speed_up / (1024 * 1024),
            avg_speed_up / (1024 * 1024),
            global_total_down_bytes / (1024 * 1024),
            progress_down,
            min_speed_down / (1024 * 1024),
            max_speed_down / (1024 * 1024),
            avg_speed_down / (1024 * 1024),
        )


# -----------------------------
# Main function and CLI via Click
# -----------------------------


async def async_main(
    workers: int,
    block_size: int,
    total_bytes: int,
    server_host: str,
    server_port: int,
    listener_host: str,
    listener_port: int,
) -> None:
    # Configure logging.
    logging.basicConfig(
        level=logging.INFO, format="%(asctime)s %(levelname)s: %(message)s"
    )

    # Expected total bytes per direction across all connections.
    # There are two sides (workers and listener), each running send_blocks and receive_blocks.
    expected_total_up = 2 * workers * total_bytes
    expected_total_down = 2 * workers * total_bytes

    # Start TCP listener.
    listener_task = asyncio.create_task(
        tcp_listener(listener_host, listener_port, total_bytes, block_size)
    )
    # Allow listener to start.
    await asyncio.sleep(1)

    # Start realtime monitor.
    monitor_task = asyncio.create_task(
        monitor_stats(expected_total_up, expected_total_down, update_interval=1)
    )

    # Launch worker connections.
    await run_workers(workers, (server_host, server_port), total_bytes, block_size)

    # When workers are done, cancel the listener and monitor.
    listener_task.cancel()
    try:
        await listener_task
    except asyncio.CancelledError:
        logging.info("TCP listener cancelled.")

    monitor_task.cancel()
    try:
        await monitor_task
    except asyncio.CancelledError:
        logging.info("Monitor task cancelled.")

    logging.info("Benchmark complete.")


@click.command()
@click.option(
    "--workers",
    default=100,
    show_default=True,
    type=int,
    help="Number of worker connections to launch.",
)
@click.option(
    "--block-size",
    default=1024,
    show_default=True,
    type=int,
    help="Block size in bytes used when sending data.",
)
@click.option(
    "--total-bytes",
    default=1024 * 1024 * 4,
    show_default=True,
    type=int,
    help="Total bytes to send per direction per connection.",
)
@click.option(
    "--server-host",
    default="127.0.0.1",
    show_default=True,
    type=str,
    help="Tunnel's server side host (for worker connections).",
)
@click.option(
    "--server-port",
    default=10003,
    show_default=True,
    type=int,
    help="Tunnel's server side port.",
)
@click.option(
    "--listener-host",
    default="0.0.0.0",
    show_default=True,
    type=str,
    help="Host for the TCP listener (tunnel's client side).",
)
@click.option(
    "--listener-port",
    default=5201,
    show_default=True,
    type=int,
    help="Port for the TCP listener.",
)
def cli(
    workers,
    block_size,
    total_bytes,
    server_host,
    server_port,
    listener_host,
    listener_port,
):
    """
    Benchmark tool for testing tunnel performance and data integrity.
    """
    try:
        asyncio.run(
            async_main(
                workers,
                block_size,
                total_bytes,
                server_host,
                server_port,
                listener_host,
                listener_port,
            )
        )
    except KeyboardInterrupt:
        logging.info("Benchmark interrupted by user.")


if __name__ == "__main__":
    cli()

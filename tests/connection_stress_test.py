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

# =========================
# Configuration Parameters
# =========================

# Number of worker connections to launch.
NUM_WORKERS = 100

# Block size in bytes used when sending data.
BLOCK_SIZE = 1024 * 1024 * 1  # 1 MiB

# Total bytes to send in each direction per connection.
TOTAL_BYTES = 1024 * 1024 * 100  # 100 MiB per direction

# Addresses for the two tunnel endpoints:
# The tunnel’s server side (workers connect here)
SERVER_ADDR = ("127.0.0.1", 10003)
# The tunnel’s client side (our TCP listener runs here)
LISTENER_ADDR = (
    "0.0.0.0",
    5201,
)  # iperf3 default port, for convenience to use them alternatively

# =========================
# Block Generation & Protocol
# =========================


async def send_blocks(
    writer: asyncio.StreamWriter, total_bytes: int, block_size: int, seed: int
) -> None:
    """
    Generate and send pseudorandom data blocks (with CRC32 checksum header) until
    a total number of bytes have been transmitted.

    The seed is used to initialize a local random generator so that the data is
    reproducible (if needed) without keeping a copy in memory.
    """
    rng = random.Random(seed)
    bytes_sent = 0
    while bytes_sent < total_bytes:
        # Determine the size for this block (last block may be smaller)
        current_block_size = min(block_size, total_bytes - bytes_sent)
        # Generate pseudorandom block data.
        # (Using getrandbits to generate all block data at once.)
        data = rng.getrandbits(current_block_size * 8).to_bytes(
            current_block_size, "big"
        )
        # Compute CRC32 checksum (masked to 32 bits)
        checksum = zlib.crc32(data) & 0xFFFFFFFF
        # Pack header: 4 bytes block size and 4 bytes checksum (big-endian)
        header = struct.pack("!II", current_block_size, checksum)
        writer.write(header + data)
        await writer.drain()
        bytes_sent += current_block_size
    # Signal EOF on the writer's side (optional, depending on your protocol)
    try:
        writer.write_eof()
    except Exception:
        pass  # Some transports do not support write_eof()


async def receive_blocks(
    reader: asyncio.StreamReader, total_bytes: int, block_size: int
) -> None:
    """
    Receive data blocks (with header) until a total number of bytes have been read.
    For each block, compute the CRC32 checksum on the fly and compare it to the
    transmitted value. Any mismatch is logged as an error.
    """
    bytes_received = 0
    while bytes_received < total_bytes:
        # Read the fixed-size header (8 bytes)
        header = await reader.readexactly(8)
        current_block_size, expected_checksum = struct.unpack("!II", header)
        # Read the data block of the given size
        data = await reader.readexactly(current_block_size)
        computed_checksum = zlib.crc32(data) & 0xFFFFFFFF
        if computed_checksum != expected_checksum:
            logging.error(
                "Checksum mismatch: expected %08x, got %08x",
                expected_checksum,
                computed_checksum,
            )
        bytes_received += current_block_size


async def exercise_connection(
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
    total_bytes: int,
    block_size: int,
    seed_send: int,
) -> None:
    """
    For a given bidirectional connection, run two tasks concurrently:
      - send_blocks: sends pseudorandom data blocks with checksum.
      - receive_blocks: receives and verifies incoming blocks.
    The two directions run concurrently.
    """
    logging.info(
        "Starting bidirectional transfer on connection %s",
        writer.get_extra_info("peername"),
    )
    start_time = time.time()
    send_task = asyncio.create_task(
        send_blocks(writer, total_bytes, block_size, seed_send)
    )
    recv_task = asyncio.create_task(receive_blocks(reader, total_bytes, block_size))
    await asyncio.gather(send_task, recv_task)
    elapsed = time.time() - start_time
    mb_sent = total_bytes / (1024 * 1024)
    logging.info(
        "Completed transfer on connection %s in %.2f seconds (%.2f MB/s each direction)",
        writer.get_extra_info("peername"),
        elapsed,
        mb_sent / elapsed if elapsed > 0 else 0,
    )


# =========================
# Worker (Client) Side
# =========================


async def worker(
    worker_id: int, server_addr: tuple, total_bytes: int, block_size: int
) -> None:
    """
    A worker coroutine that connects to the tunnel's server side (SERVER_ADDR)
    and then runs the bidirectional data transmission test.

    The seed for sending data is set to the worker's unique ID.
    """
    logging.info(
        "Worker %d: connecting to %s:%d", worker_id, server_addr[0], server_addr[1]
    )
    try:
        reader, writer = await asyncio.open_connection(*server_addr)
    except Exception as e:
        logging.exception("Worker %d: failed to connect: %s", worker_id, e)
        return

    try:
        await exercise_connection(
            reader, writer, total_bytes, block_size, seed_send=worker_id
        )
    except Exception as e:
        logging.exception("Worker %d encountered an error: %s", worker_id, e)
    finally:
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass
        logging.info("Worker %d: connection closed", worker_id)


async def run_workers(
    num_workers: int, server_addr: tuple, total_bytes: int, block_size: int
) -> None:
    """
    Launch a set of worker coroutines that connect (in parallel) to the server address.
    A slight stagger is added between startups.
    """
    tasks = []
    for i in range(num_workers):
        tasks.append(
            asyncio.create_task(worker(i, server_addr, total_bytes, block_size))
        )
        await asyncio.sleep(0.01)  # small delay to avoid connection burst
    await asyncio.gather(*tasks)


# =========================
# TCP Listener (Server) Side
# =========================


async def handle_connection(
    reader: asyncio.StreamReader, writer: asyncio.StreamWriter
) -> None:
    """
    Coroutine to handle an incoming connection on our TCP listener.
    This connection is coming from the tunnel’s client side.
    It runs the bidirectional exercise; for this side we generate a random seed
    (for sending) based on the current time.
    """
    peer = writer.get_extra_info("peername")
    logging.info("Accepted connection from %s", peer)
    # Generate a seed for sending data on this side (using current time)
    seed_send = int(time.time() * 1000) & 0xFFFFFFFF
    try:
        await exercise_connection(reader, writer, TOTAL_BYTES, BLOCK_SIZE, seed_send)
    except Exception as e:
        logging.exception("Error handling connection from %s: %s", peer, e)
    finally:
        writer.close()
        try:
            await writer.wait_closed()
        except Exception:
            pass
        logging.info("Closed connection from %s", peer)


async def tcp_listener(host: str, port: int) -> None:
    """
    Create a TCP listener on (host, port) and serve forever.
    Each new connection is handled by handle_connection().
    """
    server = await asyncio.start_server(handle_connection, host, port)
    addrs = ", ".join(str(sock.getsockname()) for sock in server.sockets)
    logging.info("TCP listener running on %s", addrs)
    async with server:
        await server.serve_forever()


# =========================
# Main Function
# =========================


async def main() -> None:
    # Configure logging to include the time and log level.
    logging.basicConfig(
        level=logging.INFO, format="%(asctime)s %(levelname)s: %(message)s"
    )

    # Start the TCP listener (the tunnel's client side will connect here).
    listener_task = asyncio.create_task(
        tcp_listener(LISTENER_ADDR[0], LISTENER_ADDR[1])
    )
    # Give the listener a moment to start.
    await asyncio.sleep(1)

    # Launch worker connections (they connect to the tunnel's server side).
    await run_workers(NUM_WORKERS, SERVER_ADDR, TOTAL_BYTES, BLOCK_SIZE)

    # When all workers are finished, we can cancel the listener.
    listener_task.cancel()
    try:
        await listener_task
    except asyncio.CancelledError:
        logging.info("TCP listener has been cancelled. Benchmark complete.")


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        logging.info("Benchmark interrupted by user.")

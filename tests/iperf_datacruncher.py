#!/usr/bin/env python3
import json
import argparse
import os
import matplotlib.pyplot as plt


def extract_throughput(intervals):
    """Extract mid-times and throughput (Mbps) for each interval."""
    times = []
    throughput = []
    for interval in intervals:
        sum_data = interval.get("sum", {})
        start = sum_data.get("start", 0)
        end = sum_data.get("end", 0)
        mid = (start + end) / 2
        times.append(mid)
        # Convert bits_per_second to Mbps
        bps = sum_data.get("bits_per_second", 0)
        throughput.append(bps / 1e6)
    return times, throughput


def extract_rtt(intervals):
    """Extract mid-times and average RTT (ms) for each interval (if available)."""
    times = []
    rtts = []
    for interval in intervals:
        stream_rtts = []
        for stream in interval.get("streams", []):
            if "rtt" in stream:
                stream_rtts.append(stream["rtt"])
        avg_rtt = sum(stream_rtts) / len(stream_rtts) if stream_rtts else None
        sum_data = interval.get("sum", {})
        start = sum_data.get("start", 0)
        end = sum_data.get("end", 0)
        mid = (start + end) / 2
        times.append(mid)
        rtts.append(avg_rtt)
    return times, rtts


def extract_cumulative_bytes(intervals):
    """Extract mid-times and cumulative bytes transferred."""
    times = []
    cum_bytes = []
    total = 0
    for interval in intervals:
        sum_data = interval.get("sum", {})
        total += sum_data.get("bytes", 0)
        start = sum_data.get("start", 0)
        end = sum_data.get("end", 0)
        mid = (start + end) / 2
        times.append(mid)
        cum_bytes.append(total)
    return times, cum_bytes


def plot_cpu_utilization(cpu_data, out_file=None):
    """Plot CPU utilization as a bar chart."""
    labels = list(cpu_data.keys())
    values = [cpu_data[k] for k in labels]
    fig_cpu, ax_cpu = plt.subplots(figsize=(8, 4))
    bars = ax_cpu.bar(labels, values, color="gray")
    ax_cpu.set_xlabel("CPU Metrics")
    ax_cpu.set_ylabel("Utilization (%)")
    ax_cpu.set_title("CPU Utilization")
    for bar, value in zip(bars, values):
        height = bar.get_height()
        ax_cpu.text(
            bar.get_x() + bar.get_width() / 2, height + 1, f"{value:.1f}%", ha="center"
        )
    plt.tight_layout()
    if out_file:
        fig_cpu.savefig(out_file)
        plt.close(fig_cpu)
    else:
        plt.show()


def process_file(json_file, out_dir):
    with open(json_file, "r") as f:
        data = json.load(f)
    intervals = data.get("intervals", [])
    if not intervals:
        print(f"No interval data found in {json_file}. Skipping.")
        return

    # Extract data for plots.
    time_throughput, throughput = extract_throughput(intervals)
    time_rtt, rtt = extract_rtt(intervals)
    time_cum_bytes, cum_bytes = extract_cumulative_bytes(intervals)

    # Create a figure with three subplots.
    fig, axs = plt.subplots(3, 1, figsize=(10, 12), sharex=True)
    fig.suptitle(
        f"iperf3 Performance Metrics\n{os.path.basename(json_file)}", fontsize=16
    )

    # Throughput vs Time.
    axs[0].plot(time_throughput, throughput, marker="o", color="blue")
    axs[0].set_ylabel("Throughput (Mbps)")
    axs[0].set_title("Throughput vs Time")
    axs[0].grid(True)

    # RTT vs Time.
    axs[1].plot(time_rtt, rtt, marker="o", color="orange")
    axs[1].set_ylabel("RTT (ms)")
    axs[1].set_title("RTT vs Time")
    axs[1].grid(True)

    # Cumulative Bytes vs Time.
    axs[2].plot(time_cum_bytes, cum_bytes, marker="o", color="green")
    axs[2].set_ylabel("Cumulative Bytes")
    axs[2].set_xlabel("Time (s)")
    axs[2].set_title("Cumulative Bytes vs Time")
    axs[2].grid(True)

    plt.tight_layout(rect=[0, 0, 1, 0.96])
    base_name = os.path.splitext(os.path.basename(json_file))[0]
    out_file = os.path.join(out_dir, f"{base_name}_plot.png")
    fig.savefig(out_file)
    plt.close(fig)
    print(f"Saved plot for {json_file} to {out_file}")

    # Plot CPU utilization if available.
    if "end" in data and "cpu_utilization_percent" in data["end"]:
        cpu_data = data["end"]["cpu_utilization_percent"]
        cpu_out_file = os.path.join(out_dir, f"{base_name}_cpu.png")
        plot_cpu_utilization(cpu_data, out_file=cpu_out_file)
        print(f"Saved CPU utilization plot for {json_file} to {cpu_out_file}")


def main():
    parser = argparse.ArgumentParser(
        description="Generate iperf3 plots from JSON output files."
    )
    parser.add_argument("json_files", nargs="+", help="Path(s) to iperf3 JSON file(s).")
    parser.add_argument(
        "--out-dir", default="testplots", help="Directory to save plot images."
    )
    args = parser.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)
    for json_file in args.json_files:
        process_file(json_file, args.out_dir)


if __name__ == "__main__":
    main()

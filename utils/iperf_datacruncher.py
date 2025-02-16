#!/usr/bin/env python3
"""
iperf3 Datacruncher Tool
-------------------------
This script processes multiple iperf3 JSON output files (generated with the -J flag) to extract throughput and RTT data.
It then produces two comparison plots:
  - Throughput (Mbps) vs. Time (s)
  - RTT (ms) vs. Time (s)
Data from forward and reverse tests (determined by filenames) are plotted separately.
The generated plots are saved in the specified output directory.

Usage:
  python3 utils/iperf_datacruncher.py <json_files> [--out-dir <output_directory>]
"""

import json
import os
import argparse
import matplotlib.pyplot as plt


def extract_throughput(intervals):
    """
    Given the list of interval dictionaries from an iperf3 JSON output,
    return two lists: mid-times (s) and throughput (Mbps) for each interval.
    If the first datapoint is not at time 0, a point at 0 (throughput 0) is inserted.
    """
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
    if times and times[0] > 0:
        times.insert(0, 0)
        throughput.insert(0, 0)
    return times, throughput


def extract_rtt(intervals):
    """
    Given the list of interval dictionaries from an iperf3 JSON output,
    return two lists: mid-times (s) and average RTT (ms) for each interval.
    If the first datapoint is not at time 0, a point at time 0 is inserted using
    the first measured RTT.
    """
    times = []
    rtts = []
    for interval in intervals:
        # Average RTT across streams (if available)
        stream_rtts = [
            stream["rtt"] for stream in interval.get("streams", []) if "rtt" in stream
        ]
        avg_rtt = sum(stream_rtts) / len(stream_rtts) if stream_rtts else None
        sum_data = interval.get("sum", {})
        start = sum_data.get("start", 0)
        end = sum_data.get("end", 0)
        mid = (start + end) / 2
        times.append(mid)
        rtts.append(avg_rtt)
    if times and times[0] > 0:
        # For RTT, we assume the first measured value applies at time 0.
        times.insert(0, 0)
        rtts.insert(0, rtts[0])
    return times, rtts


def label_from_filename(filepath):
    """
    Derive a label from the filename. If the filename contains 'baseline' or 'tunneled',
    use that; and append 'Forward' or 'Reverse' based on the presence of '_R'.
    """
    base = os.path.basename(filepath)
    if "baseline" in base:
        label = "Baseline"
    elif "tunneled_client_parallel" in base:
        label = "Tunneled (parallel)"
    elif "tunneled_client" in base:
        label = "Tunneled"
    else:
        label = base
    if "_R" in base:
        label += " Reverse"
    else:
        label += " Forward"
    return label


def load_data(json_file):
    """Load a JSON file and extract throughput and RTT data (times and values)."""
    try:
        with open(json_file, "r") as f:
            data = json.load(f)
    except Exception as e:
        print(f"Error loading {json_file}: {e}")
        return None
    intervals = data.get("intervals", [])
    if not intervals:
        print(f"No interval data in {json_file}")
        return None
    t_th, throughput = extract_throughput(intervals)
    t_rtt, rtt = extract_rtt(intervals)
    return t_th, throughput, t_rtt, rtt


def main():
    parser = argparse.ArgumentParser(
        description="Compare iperf3 throughput and RTT across multiple JSON files."
    )
    parser.add_argument("json_files", nargs="+", help="Path(s) to iperf3 JSON file(s).")
    parser.add_argument(
        "--out-dir", default="testplots", help="Directory to save plot images."
    )
    args = parser.parse_args()
    os.makedirs(args.out_dir, exist_ok=True)

    # Group curves by direction for both throughput and RTT.
    forward_throughput = []
    reverse_throughput = []
    forward_rtt = []
    reverse_rtt = []

    for json_file in args.json_files:
        data = load_data(json_file)
        if data is None:
            continue
        t_th, throughput, t_rtt, rtt = data
        label = label_from_filename(json_file)
        if "Reverse" in label:
            reverse_throughput.append((label, t_th, throughput))
            reverse_rtt.append((label, t_rtt, rtt))
        else:
            forward_throughput.append((label, t_th, throughput))
            forward_rtt.append((label, t_rtt, rtt))

    # ----- Throughput Comparison Plot -----
    fig, (ax_fwd, ax_rev) = plt.subplots(1, 2, figsize=(14, 6), sharey=True)
    fig.suptitle("iperf3 Throughput Comparison (Mbps)", fontsize=16)

    if forward_throughput:
        for label, t, th in forward_throughput:
            ax_fwd.plot(t, th, marker="o", label=label)
        ax_fwd.set_title("Forward")
        ax_fwd.set_xlabel("Time (s)")
        ax_fwd.set_ylabel("Throughput (Mbps)")
        ax_fwd.grid(True)
        ax_fwd.legend()
    else:
        ax_fwd.text(0.5, 0.5, "No Forward Data", ha="center", va="center")

    if reverse_throughput:
        for label, t, th in reverse_throughput:
            ax_rev.plot(t, th, marker="o", label=label)
        ax_rev.set_title("Reverse")
        ax_rev.set_xlabel("Time (s)")
        ax_rev.grid(True)
        ax_rev.legend()
    else:
        ax_rev.text(0.5, 0.5, "No Reverse Data", ha="center", va="center")

    plt.tight_layout(rect=[0, 0, 1, 0.95])
    throughput_file = os.path.join(args.out_dir, "comparison_throughput.png")
    fig.savefig(throughput_file)
    plt.close(fig)
    print(f"Saved throughput comparison plot to {throughput_file}")

    # ----- RTT Comparison Plot -----
    fig, (ax_fwd, ax_rev) = plt.subplots(1, 2, figsize=(14, 6), sharey=True)
    fig.suptitle("iperf3 RTT Comparison (ms)", fontsize=16)

    if forward_rtt:
        for label, t, r in forward_rtt:
            ax_fwd.plot(t, r, marker="o", label=label)
        ax_fwd.set_title("Forward")
        ax_fwd.set_xlabel("Time (s)")
        ax_fwd.set_ylabel("RTT (ms)")
        ax_fwd.grid(True)
        ax_fwd.legend()
    else:
        ax_fwd.text(0.5, 0.5, "No Forward Data", ha="center", va="center")

    if reverse_rtt:
        for label, t, r in reverse_rtt:
            ax_rev.plot(t, r, marker="o", label=label)
        ax_rev.set_title("Reverse")
        ax_rev.set_xlabel("Time (s)")
        ax_rev.grid(True)
        ax_rev.legend()
    else:
        ax_rev.text(0.5, 0.5, "No Reverse Data", ha="center", va="center")

    plt.tight_layout(rect=[0, 0, 1, 0.95])
    rtt_file = os.path.join(args.out_dir, "comparison_rtt.png")
    fig.savefig(rtt_file)
    plt.close(fig)
    print(f"Saved RTT comparison plot to {rtt_file}")


if __name__ == "__main__":
    main()

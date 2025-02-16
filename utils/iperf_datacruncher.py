#!/usr/bin/env python3
import json
import os
import argparse
import matplotlib.pyplot as plt

def extract_throughput(intervals):
    """
    Given the list of interval dictionaries from an iperf3 JSON output,
    return two lists: mid-times (s) and throughput (Mbps) for each interval.
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
    return times, throughput


def label_from_filename(filepath):
    """
    Create a label based on the filename.
    If the filename contains "baseline" or "tunneled", use that,
    and append "Forward" or "Reverse" based on the presence of '_R'.
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


def process_json_file(json_file):
    """Load JSON file and extract throughput data (times, Mbps)."""
    try:
        with open(json_file, "r") as f:
            data = json.load(f)
    except Exception as e:
        print(f"Error loading {json_file}: {e}")
        return None, None

    intervals = data.get("intervals", [])
    if not intervals:
        print(f"No interval data found in {json_file}. Skipping.")
        return None, None

    return extract_throughput(intervals)


def main():
    parser = argparse.ArgumentParser(
        description="Compare iperf3 throughput (Forward vs Reverse) across multiple JSON files."
    )
    parser.add_argument("json_files", nargs="+", help="Path(s) to iperf3 JSON file(s).")
    parser.add_argument(
        "--out-dir", default="testplots", help="Directory to save the comparison plot."
    )
    parser.add_argument(
        "--out-file",
        default="comparison_plot.png",
        help="Filename for the output plot image.",
    )
    args = parser.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)

    # Group curves into forward and reverse
    forward_curves = []
    reverse_curves = []

    for json_file in args.json_files:
        times, throughput = process_json_file(json_file)
        if times is None or throughput is None:
            continue
        label = label_from_filename(json_file)
        if "Reverse" in label:
            reverse_curves.append((label, times, throughput))
        else:
            forward_curves.append((label, times, throughput))

    # Create a figure with two subplots: one for forward, one for reverse
    fig, (ax_forward, ax_reverse) = plt.subplots(1, 2, figsize=(14, 6), sharey=True)
    fig.suptitle("Comparison of iperf3 Throughput (Mbps)", fontsize=16)

    # Plot forward curves
    if forward_curves:
        for label, times, throughput in forward_curves:
            ax_forward.plot(times, throughput, marker="o", label=label)
        ax_forward.set_title("Forward Tests")
        ax_forward.set_xlabel("Time (s)")
        ax_forward.set_ylabel("Throughput (Mbps)")
        ax_forward.grid(True)
        ax_forward.legend()
    else:
        ax_forward.text(0.5, 0.5, "No Forward Data", ha="center", va="center")

    # Plot reverse curves
    if reverse_curves:
        for label, times, throughput in reverse_curves:
            ax_reverse.plot(times, throughput, marker="o", label=label)
        ax_reverse.set_title("Reverse Tests")
        ax_reverse.set_xlabel("Time (s)")
        ax_reverse.grid(True)
        ax_reverse.legend()
    else:
        ax_reverse.text(0.5, 0.5, "No Reverse Data", ha="center", va="center")

    plt.tight_layout(rect=[0, 0, 1, 0.95])
    out_path = os.path.join(args.out_dir, args.out_file)
    fig.savefig(out_path)
    plt.close(fig)
    print(f"Saved comparison plot to {out_path}")

if __name__ == "__main__":
    main()

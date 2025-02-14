#!/usr/bin/env python3
"""
Dependencies (Arch Linux):
 pacman -S python-pandas python-matplotlib

Usage:
 python3 tests/cst_datacruncher.py testlogs/prrs_full_e2e_connection_stress_test_*/cst_*

Unit Test:
 python3 -m unittest test_cst_datacruncher.py
"""
import re
import sys
import os
import argparse
from datetime import datetime
import pandas as pd
import matplotlib.pyplot as plt

# Regex to remove ANSI escape codes
ansi_escape = re.compile(r"\x1B\[[0-?]*[ -/]*[@-~]")

# Regex to parse a generic log line
log_line_re = re.compile(
    r"^(?P<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},\d+)\s+(?P<level>\S+):\s+(?P<message>.*)$"
)


def parse_filename(filename):
    """
    Extracts test details from the filename.
    Expected format: cst_(baseline|benchmark)_<concurrency>_<transfersize>.log
    e.g., cst_baseline_1c_1gb.log or cst_benchmark_50c_20mb.log
    """
    base = os.path.basename(filename)
    parts = base.split("_")
    if len(parts) >= 4:
        mode = parts[1]
        concurrency = parts[2]
        transfersize = parts[3].split(".")[0]  # remove extension
        return mode, concurrency, transfersize
    return None, None, None


def parse_transfer_metrics(message):
    """
    Parses the transfer metrics from a log message using substring splitting.
    Expected format (after ANSI codes are removed):
      UP: <up_mb> MB transferred (<up_pct>% complete) | Speed: min: <up_min> MB/s, max: <up_max> MB/s, avg: <up_avg> MB/s
      || DOWN: <down_mb> MB transferred (<down_pct>% complete) | Speed: min: <down_min> MB/s, max: <down_max> MB/s, avg: <down_avg> MB/s
    """
    try:
        # Split into UP and DOWN parts using the "||" delimiter.
        parts = message.split("||")
        if len(parts) < 2:
            return None
        up_part = parts[0].strip()
        down_part = parts[1].strip()

        # --- Parse UP part ---
        # Example up_part:
        # "UP: 218.88 MB transferred (10.94% complete) | Speed: min: 179.18 MB/s, max: 300.28 MB/s, avg: 220.53 MB/s"
        # Get the transfer amount.
        up_mb_str = up_part.split("MB transferred")[0].split("UP:")[1].strip()
        up_mb = float(up_mb_str)
        # Get the percentage (inside the first parentheses)
        pct_section = up_part.split("MB transferred")[1]
        start = pct_section.find("(")
        end = pct_section.find("%")
        up_pct = float(pct_section[start + 1 : end].strip())
        # Get the speed section (after the "|" delimiter)
        up_speed_section = up_part.split("|")[-1]
        if "Speed:" in up_speed_section:
            up_speed_section = up_speed_section.split("Speed:")[1]
        speed_parts = up_speed_section.split(",")
        up_min = float(speed_parts[0].split("min:")[1].strip().split(" ")[0])
        up_max = float(speed_parts[1].split("max:")[1].strip().split(" ")[0])
        up_avg = float(speed_parts[2].split("avg:")[1].strip().split(" ")[0])

        # --- Parse DOWN part ---
        # Example down_part:
        # "DOWN: 33.38 MB transferred (1.67% complete) | Speed: min: 0.00 MB/s, max: 2080.51 MB/s, avg: 1326.88 MB/s"
        down_mb_str = down_part.split("MB transferred")[0].split("DOWN:")[1].strip()
        down_mb = float(down_mb_str)
        pct_section = down_part.split("MB transferred")[1]
        start = pct_section.find("(")
        end = pct_section.find("%")
        down_pct = float(pct_section[start + 1 : end].strip())
        down_speed_section = down_part.split("|")[-1]
        if "Speed:" in down_speed_section:
            down_speed_section = down_speed_section.split("Speed:")[1]
        speed_parts = down_speed_section.split(",")
        down_min = float(speed_parts[0].split("min:")[1].strip().split(" ")[0])
        down_max = float(speed_parts[1].split("max:")[1].strip().split(" ")[0])
        down_avg = float(speed_parts[2].split("avg:")[1].strip().split(" ")[0])

        return (
            up_mb,
            up_pct,
            up_min,
            up_max,
            up_avg,
            down_mb,
            down_pct,
            down_min,
            down_max,
            down_avg,
        )
    except Exception as e:
        print("Error parsing transfer metrics:", e)
        return None


def parse_log_file(filepath):
    data = []
    mode, concurrency, transfersize = parse_filename(filepath)
    with open(filepath, "r", encoding="utf-8") as f:
        for line in f:
            # Remove ANSI codes
            clean_line = ansi_escape.sub("", line).strip()
            if not clean_line:
                continue
            # Parse the generic log line
            m = log_line_re.match(clean_line)
            if not m:
                continue
            log_entry = m.groupdict()
            try:
                # Convert timestamp to datetime
                log_entry["timestamp"] = datetime.strptime(
                    log_entry["timestamp"], "%Y-%m-%d %H:%M:%S,%f"
                )
            except Exception as e:
                print(f"Timestamp parsing error in file {filepath}: {e}")
                continue

            log_entry["file"] = os.path.basename(filepath)
            log_entry["mode"] = mode
            log_entry["concurrency"] = concurrency
            log_entry["transfersize"] = transfersize
            # Default event type is the raw message
            log_entry["event"] = "message"
            # Initialize metric fields as None
            for key in [
                "up_mb",
                "up_pct",
                "up_min",
                "up_max",
                "up_avg",
                "down_mb",
                "down_pct",
                "down_min",
                "down_max",
                "down_avg",
            ]:
                log_entry[key] = None

            # If the message contains transfer metrics, try to parse them.
            if "UP:" in log_entry["message"] and "DOWN:" in log_entry["message"]:
                parsed = parse_transfer_metrics(log_entry["message"])
                if parsed:
                    (
                        log_entry["up_mb"],
                        log_entry["up_pct"],
                        log_entry["up_min"],
                        log_entry["up_max"],
                        log_entry["up_avg"],
                        log_entry["down_mb"],
                        log_entry["down_pct"],
                        log_entry["down_min"],
                        log_entry["down_max"],
                        log_entry["down_avg"],
                    ) = parsed
                    log_entry["event"] = "transfer"
            data.append(log_entry)
    return data


def load_logs(filepaths):
    all_data = []
    for filepath in filepaths:
        print(f"Processing file: {filepath}")
        all_data.extend(parse_log_file(filepath))
    return pd.DataFrame(all_data)


def plot_transfer_timeseries(df):
    # Filter for transfer events only
    df_transfer = df[df["event"] == "transfer"].copy()
    if df_transfer.empty:
        print("No transfer events found to plot.")
        return

    # Plot UP and DOWN average speeds over time for each file
    files = df_transfer["file"].unique()
    plt.figure(figsize=(12, 6))
    for f in files:
        df_file = df_transfer[df_transfer["file"] == f]
        plt.plot(df_file["timestamp"], df_file["up_avg"], marker="o", label=f"{f} UP")
        plt.plot(
            df_file["timestamp"],
            df_file["down_avg"],
            marker="x",
            linestyle="--",
            label=f"{f} DOWN",
        )
    plt.xlabel("Timestamp")
    plt.ylabel("Average Speed (MB/s)")
    plt.title("Transfer Average Speeds Over Time")
    plt.legend(fontsize="small", loc="upper left")
    plt.grid(True)
    plt.tight_layout()
    output_file = "transfer_timeseries.png"
    plt.savefig(output_file)
    print(f"Time series plot saved as {output_file}")
    plt.close()


def plot_speed_boxplot(df):
    # Create a boxplot comparing UP average speeds by test configuration (using concurrency & mode)
    df_transfer = df[df["event"] == "transfer"].copy()
    if df_transfer.empty:
        print("No transfer events found for boxplot.")
        return

    # Create a new column for configuration summary
    df_transfer["config"] = (
        df_transfer["mode"]
        + "_"
        + df_transfer["concurrency"]
        + "_"
        + df_transfer["transfersize"]
    )
    plt.figure(figsize=(12, 6))
    df_transfer.boxplot(column="up_avg", by="config", grid=True, rot=45)
    plt.xlabel("Test Configuration")
    plt.ylabel("UP Avg Speed (MB/s)")
    plt.title("Distribution of UP Average Speeds by Configuration")
    plt.suptitle("")
    plt.tight_layout()
    output_file = "speed_boxplot.png"
    plt.savefig(output_file)
    print(f"Boxplot saved as {output_file}")
    plt.close()


def main():
    parser = argparse.ArgumentParser(
        description="Analyze benchmark log files and plot transfer metrics."
    )
    parser.add_argument("files", nargs="+", help="Paths to log files")
    args = parser.parse_args()

    df = load_logs(args.files)

    if df.empty:
        print("No log data was parsed. Exiting.")
        sys.exit(1)

    print("Sample parsed data:")
    print(df.head())

    # Plot time series for transfer speeds and save as PNG
    plot_transfer_timeseries(df)
    # Plot boxplot for UP average speeds by test configuration and save as PNG
    plot_speed_boxplot(df)


if __name__ == "__main__":
    main()

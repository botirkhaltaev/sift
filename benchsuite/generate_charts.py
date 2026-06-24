#!/usr/bin/env python3
"""Generate benchmark charts from benchsuite CSV output."""

import argparse
import csv
import os
import statistics
import sys

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker
import numpy as np


def load_results(csv_path):
    """Parse CSV into per-benchmark, per-tool mean durations."""
    raw = {}
    with open(csv_path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            bench = row['benchmark']
            name = row['name']
            duration = float(row['duration'])
            key = (bench, name)
            raw.setdefault(key, []).append(duration)

    benchmarks = {}
    for (bench, name), durations in raw.items():
        mean = statistics.mean(durations)
        benchmarks.setdefault(bench, {})[name] = mean
    return benchmarks


def short_name(bench_name):
    """Shorten benchmark name for chart labels."""
    return bench_name.replace('linux_', '').replace('_', ' ')


def generate_time_chart(benchmarks, output_path):
    """Grouped bar chart: sift vs rg mean time per benchmark."""
    # Filter to only rg and sift (primary variants, not ASCII)
    bench_names = list(benchmarks.keys())

    rg_times = []
    sift_times = []
    labels = []

    for bench in bench_names:
        tools = benchmarks[bench]
        rg_t = tools.get('rg')
        sift_t = tools.get('sift')
        if rg_t is None or sift_t is None:
            continue
        rg_times.append(rg_t)
        sift_times.append(sift_t)
        labels.append(short_name(bench))

    x = np.arange(len(labels))
    width = 0.35

    fig, ax = plt.subplots(figsize=(14, 6))
    bars_rg = ax.bar(x - width / 2, rg_times, width, label='ripgrep',
                     color='#e74c3c', edgecolor='white', linewidth=0.5)
    bars_sift = ax.bar(x + width / 2, sift_times, width, label='sift',
                       color='#3498db', edgecolor='white', linewidth=0.5)

    ax.set_ylabel('Mean time (seconds)', fontsize=12)
    ax.set_title('sift vs ripgrep -- Linux kernel corpus (79K files, 1.3 GB)',
                 fontsize=14, fontweight='bold')
    ax.set_xticks(x)
    ax.set_xticklabels(labels, rotation=35, ha='right', fontsize=10)
    ax.legend(fontsize=11)
    ax.grid(axis='y', alpha=0.3)
    ax.set_axisbelow(True)

    # Add time labels on bars
    for bar in bars_rg:
        h = bar.get_height()
        ax.annotate(f'{h:.2f}s', xy=(bar.get_x() + bar.get_width() / 2, h),
                    xytext=(0, 3), textcoords='offset points',
                    ha='center', va='bottom', fontsize=8)
    for bar in bars_sift:
        h = bar.get_height()
        ax.annotate(f'{h:.2f}s', xy=(bar.get_x() + bar.get_width() / 2, h),
                    xytext=(0, 3), textcoords='offset points',
                    ha='center', va='bottom', fontsize=8)

    fig.tight_layout()
    fig.savefig(output_path, dpi=150, bbox_inches='tight')
    plt.close(fig)
    print(f'Saved: {output_path}', file=sys.stderr)


def generate_speedup_chart(benchmarks, output_path):
    """Horizontal bar chart showing Nx speedup (rg_time / sift_time)."""
    entries = []
    for bench, tools in benchmarks.items():
        rg_t = tools.get('rg')
        sift_t = tools.get('sift')
        if rg_t is None or sift_t is None:
            continue
        speedup = rg_t / sift_t
        entries.append((short_name(bench), speedup, rg_t, sift_t))

    # Sort by speedup descending
    entries.sort(key=lambda e: e[1], reverse=True)

    labels = [e[0] for e in entries]
    speedups = [e[1] for e in entries]
    colors = ['#27ae60' if s > 1.0 else '#e74c3c' for s in speedups]

    fig, ax = plt.subplots(figsize=(12, 6))
    y = np.arange(len(labels))
    bars = ax.barh(y, speedups, color=colors, edgecolor='white', linewidth=0.5,
                   height=0.6)

    ax.set_yticks(y)
    ax.set_yticklabels(labels, fontsize=11)
    ax.set_xlabel('Speedup (rg time / sift time)', fontsize=12)
    ax.set_title('sift speedup over ripgrep -- Linux kernel corpus',
                 fontsize=14, fontweight='bold')
    ax.axvline(x=1.0, color='#333', linestyle='--', linewidth=1, alpha=0.7)
    ax.grid(axis='x', alpha=0.3)
    ax.set_axisbelow(True)
    ax.invert_yaxis()

    for bar, speedup in zip(bars, speedups):
        w = bar.get_width()
        label = f'{speedup:.1f}x'
        if speedup < 1.0:
            label = f'{1/speedup:.1f}x slower'
        ax.annotate(label, xy=(w, bar.get_y() + bar.get_height() / 2),
                    xytext=(5, 0), textcoords='offset points',
                    ha='left', va='center', fontsize=10, fontweight='bold')

    fig.tight_layout()
    fig.savefig(output_path, dpi=150, bbox_inches='tight')
    plt.close(fig)
    print(f'Saved: {output_path}', file=sys.stderr)


def main():
    p = argparse.ArgumentParser(description='Generate benchmark charts')
    p.add_argument('csv', help='Path to raw CSV from benchsuite')
    p.add_argument('--outdir', default='.', help='Output directory for charts')
    args = p.parse_args()

    os.makedirs(args.outdir, exist_ok=True)
    benchmarks = load_results(args.csv)

    generate_time_chart(
        benchmarks, os.path.join(args.outdir, 'bench_times.png'))
    generate_speedup_chart(
        benchmarks, os.path.join(args.outdir, 'bench_speedup.png'))


if __name__ == '__main__':
    main()

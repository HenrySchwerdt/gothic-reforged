#!/usr/bin/env bash
# Run every fuzz target for SECONDS (default 600) with the seed corpus and dictionary.
# Usage: crates/formats/fuzz/run_all.sh [SECONDS] [PARALLEL]
set -euo pipefail
cd "$(dirname "$0")"
secs="${1:-600}"
jobs="${2:-2}"
cargo +nightly run -q --manifest-path ../Cargo.toml --example fuzz_seeds -- corpus >/dev/null
cargo +nightly fuzz build -O
mkdir -p logs
targets=$(cargo +nightly fuzz list)
printf '%s\n' $targets | xargs -P "$jobs" -I{} sh -c \
  "cargo +nightly fuzz run -O {} corpus/{} -- -max_total_time=$secs -dict=dict.txt \
     -rss_limit_mb=2048 -malloc_limit_mb=512 -timeout=10 -print_final_stats=1 \
     > logs/{}.log 2>&1 && echo 'ok   {}' || echo 'FAIL {} (see fuzz/logs/{}.log, fuzz/artifacts/{})'"

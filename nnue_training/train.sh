#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${1:-}" == --help ]]; then
  cat <<'HELP'
Usage: bash nnue_training/train.sh [examples.jsonl] [initial.nnue] [new-output-directory]

Run only after data generation has finished.
Defaults: regenerated JSONL, work/nnue-initial.nnue, a new work/train-regenerated-* directory.
Uses source_id-based 80/20 splitting and trains 3 epochs per command, 9 epochs total.
Models, logs, commands and metrics are saved in the printed absolute output directory.
Runs in the foreground; Ctrl-C stops training. Requires cargo, ruby, mkfifo and tee.
HELP
  exit 0
fi
if (( $# > 3 )); then
  echo 'Too many arguments; use --help.' >&2
  exit 2
fi
source_file="${1:-$root/nnue_training/work/yaneuraou-examples-regenerated.jsonl}"
initial_model="${2:-$root/nnue_training/work/nnue-initial.nnue}"
[[ -s "$source_file" ]] || { echo "Missing or empty dataset: $source_file" >&2; exit 1; }
[[ -s "$initial_model" ]] || { echo "Missing or empty model: $initial_model" >&2; exit 1; }
if (( $# == 3 )); then
  mkdir -- "$3"
  output_dir="$(cd -- "$3" && pwd)"
else
  output_dir="$(mktemp -d "$root/nnue_training/work/train-regenerated-$(date +%Y%m%d-%H%M%S)-XXXXXX")"
fi
printf 'Output directory: %s\nLog: %s/run.log\nMetrics: %s/metrics.json\n' "$output_dir" "$output_dir" "$output_dir"
(
  cargo build --manifest-path "$root/Cargo.toml" --target-dir "$root/target" --release --locked -p nnue_training --bin learn
  exec ruby "$root/nnue_training/train.rb" "$source_file" "$initial_model" "$output_dir"
) 2>&1 | tee "$output_dir/run.log"

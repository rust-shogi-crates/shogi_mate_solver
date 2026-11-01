#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == --help ]]; then
  echo 'Usage: bash ops/yaneuraou-learn/benchmark.sh MODEL_DIR DATASET.jsonl OUTPUT_DIR'
  echo 'Evaluates nnue-initial.nnue and all nnue-epoch*.nnue models, with a 20000-position limit.'
  echo 'Existing result files in OUTPUT_DIR are overwritten.'
  exit 0
fi
if (( $# != 3 )); then
  echo 'Usage: bash ops/yaneuraou-learn/benchmark.sh MODEL_DIR DATASET.jsonl OUTPUT_DIR' >&2
  exit 2
fi
model_dir="$(cd -- "$1" && pwd)"
[[ -s "$2" ]] || { echo "Missing or empty dataset: $2" >&2; exit 1; }
dataset="$(cd -- "$(dirname -- "$2")" && pwd)/$(basename -- "$2")"
[[ -s "$model_dir/nnue-initial.nnue" ]] || { echo 'Missing initial model' >&2; exit 1; }
shopt -s nullglob
epoch_models=("$model_dir"/nnue-epoch*.nnue)
(( ${#epoch_models[@]} > 0 )) || { echo 'No epoch models found' >&2; exit 1; }
for model in "${epoch_models[@]}"; do
  [[ -s "$model" ]] || { echo "Empty model: $model" >&2; exit 1; }
done
mkdir -p -- "$3"
output_dir="$(cd -- "$3" && pwd)"
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo build --manifest-path "$root/Cargo.toml" --target-dir "$root/target" --release --locked -p benchmark_harness
for model in "$model_dir/nnue-initial.nnue" "${epoch_models[@]}"; do
  name="$(basename -- "$model" .nnue)"
  echo "評価開始: $name"
  "$root/target/release/benchmark_harness" run \
    --move-ordering=nnue-model \
    --nnue-model="$model" \
    --max-positions=20000 \
    --revision="$name" \
    "$dataset" \
    > "$output_dir/$name.jsonl" \
    2> "$output_dir/$name.log"
  echo "評価終了: $name"
done
printf '結果: %s\n' "$output_dir"

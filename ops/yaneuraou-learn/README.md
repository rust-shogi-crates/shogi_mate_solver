用意する物

- yaneuraou データセット TODO


テストセット
```bash
ruby <<'RUBY'
require 'json'
require 'digest'

input = 'nnue_training/work/yaneuraou-examples-regenerated.jsonl'
output = 'nnue_training/work/compare-20261101/train-search.jsonl'
rng = Random.new(20261101)
samples = { 0 => [], 1 => [] }
counts = { 0 => 0, 1 => 0 }
last_source = nil
is_train = false

File.foreach(input) do |line|
  next if line.strip.empty?
  example = JSON.parse(line)
  source = example.fetch('source_id')

  if source != last_source
    hash = Digest::SHA384.digest("20261031-source-split-v1\0#{source}")
    is_train = hash.unpack1('Q>') % 5 != 0
    last_source = source
  end

  next unless is_train && example.fetch('role') == 'attacker'

  label = example.fetch('label')
  counts[label] += 1
  bucket = samples.fetch(label)

  if bucket.length < 16
    bucket << example
  else
    index = rng.rand(counts[label])
    bucket[index] = example if index < 16
  end
end

unless samples.values.all? { |bucket| bucket.length == 16 }
  abort "抽出対象が不足しています: #{counts}"
end

File.open(output, 'w') do |file|
  samples.values.flatten.each do |example|
    file.puts JSON.generate(
      id: example.fetch('id'),
      sfen: example.fetch('sfen'),
      expected: example.fetch('label') == 1 ? 'mate' : 'nomate'
    )
  end
end

puts "抽出対象: #{counts}"
puts "32局面で上書きしました: #{File.expand_path(output)}"
RUBY
```

repo root を CWD とする。

# テストデータ作成

```bash
```

# 学習
```bash
bash nnue_training/train.sh
```

Output directory: XXXX という形で dirname が出る。

# テスト

```bash
# 実際の学習結果フォルダに置き換える
RUN="./nnue_training/work/train-regenerated-20261101-113833-WZE6CO"

cargo build --release --locked -p benchmark_harness

mkdir -p "$RUN/benchmark"

for MODEL in nnue-initial nnue-epoch03 nnue-epoch06 nnue-epoch09; do
  ./target/release/benchmark_harness run \
    --move-ordering=nnue-model \
    --nnue-model="$RUN/$MODEL.nnue" \
    --max-positions=20000 \
    --revision="$MODEL" \
    benchmark/issue16-ordering.jsonl \
    > "$RUN/benchmark/$MODEL.jsonl" \
    2> "$RUN/benchmark/$MODEL.log"
done
```

表の作成

```bash
cd /Users/kobas-mac/srcview/shogi_mate_solver-worktrees/issue-16-pr7-trained-model-rollout

ruby -rjson <<'RUBY'
dir = 'nnue_training/work/train-regenerated-20261101-113833-WZE6CO/benchmark'
models = %w[nnue-initial nnue-epoch03 nnue-epoch06 nnue-epoch09]

# train-search.jsonl の結果を集計するときは '-same' に変更
suffix = ''

data = models.to_h do |model|
  path = File.join(dir, "#{model}#{suffix}.jsonl")
  records = File.readlines(path)
    .reject { |line| line.strip.empty? }
    .map { |line| JSON.parse(line) }

  abort "#{path}: errorレコードがあります" if records.any? { |r| r['type'] == 'error' }
  results = records.select { |r| r['type'] == 'result' }
  abort "#{path}: 評価結果がありません" if results.empty?
  [model, results]
end

puts '| 探索方式 | モデル | 正解数 | 上限到達 | 合計探索局面数 | 初期比 | 合計時間 (ms) |'
puts '|---|---|---:|---:|---:|---:|---:|'

%w[df_pn eval].each do |solver|
  baseline_rows = data.fetch('nnue-initial')
    .select { |r| r['evaluator'] == solver }
  abort "#{solver}: 初期モデルの結果がありません" if baseline_rows.empty?
  baseline = baseline_rows.sum { |r| r.fetch('positions_inspected') }
  baseline_ids = baseline_rows.map { |r| [r['source'], r['id']] }.sort

  models.each do |model|
    rows = data.fetch(model).select { |r| r['evaluator'] == solver }
    ids = rows.map { |r| [r['source'], r['id']] }.sort
    abort "#{model}/#{solver}: 局面集合が一致しません" unless ids == baseline_ids

    nodes = rows.sum { |r| r.fetch('positions_inspected') }
    ratio =
      if model == 'nnue-initial'
        '基準'
      elsif baseline.zero?
        '—'
      else
        format('%+.1f%%', 100.0 * (nodes.to_f / baseline - 1))
      end

    puts format(
      '| %s | %s | %d/%d | %d | %d | %s | %.2f |',
      solver,
      model.delete_prefix('nnue-'),
      rows.count { |r| r['correct'] == true },
      rows.size,
      rows.count { |r| r['limit_reached'] },
      nodes,
      ratio,
      rows.sum { |r| r.fetch('elapsed_ms') }
    )
  end
end
RUBY
```

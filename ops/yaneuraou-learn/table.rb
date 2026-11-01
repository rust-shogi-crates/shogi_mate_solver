require 'json'

if ARGV == ['--help']
  puts 'Usage: ruby ops/yaneuraou-learn/table.rb RESULTS_DIR'
  exit
end
abort 'Usage: ruby ops/yaneuraou-learn/table.rb RESULTS_DIR' unless ARGV.length == 1
dir = ARGV[0]
pattern = /\Annue-(initial|epoch\d+)\.jsonl\z/
models = Dir.children(dir).filter_map do |name|
  match = pattern.match(name)
  "nnue-#{match[1]}" if match
end.sort_by { |name| name == 'nnue-initial' ? -1 : name.delete_prefix('nnue-epoch').to_i }
abort '初期モデルの結果がありません' unless models.include?('nnue-initial')
abort '比較するepochの結果がありません' unless models.length > 1

data = models.to_h do |model|
  path = File.join(dir, "#{model}.jsonl")
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

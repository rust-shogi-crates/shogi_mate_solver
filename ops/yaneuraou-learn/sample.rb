require 'json'
require 'digest'
require 'fileutils'

if ARGV == ['--help']
  puts 'Usage: ruby ops/yaneuraou-learn/sample.rb INPUT.jsonl OUTPUT.jsonl'
  exit
end
abort 'Usage: ruby ops/yaneuraou-learn/sample.rb INPUT.jsonl OUTPUT.jsonl' unless ARGV.length == 2
input, output = ARGV.map { |path| File.expand_path(path) }
abort '入力と出力には異なるファイルを指定してください' if input == output || (File.exist?(output) && File.identical?(input, output))
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

FileUtils.mkdir_p(File.dirname(output))
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

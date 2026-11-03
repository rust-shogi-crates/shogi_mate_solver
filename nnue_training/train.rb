require 'time'
require 'json'
require 'digest'
require 'fileutils'
require 'shellwords'
require 'rbconfig'
$stdout.sync = true
$stderr.sync = true
ROOT = File.expand_path('..', __dir__)
SOURCE, INITIAL, RUN = ARGV.shift(3).map { |path| File.expand_path(path) }
SEED = '20261031-source-split-v1'

def partition(source_id)
  Digest::SHA384.digest("#{SEED}\0#{source_id}").unpack1('Q>') % 5 == 0 ? 'test' : 'train'
end

def save_json(name, value)
  path = File.join(RUN, name)
  File.write("#{path}.tmp", JSON.pretty_generate(value) + "\n")
  File.rename("#{path}.tmp", path)
end

if ARGV.first == '--feed'
  part, pipe = ARGV[1..2]
  counts = Hash.new(0)
  last_source = nil
  chosen = nil
  File.open(pipe, 'w') do |out|
    File.foreach(SOURCE) do |line|
      next if line.strip.empty?
      example = JSON.parse(line)
      id = example.fetch('source_id')
      if id != last_source
        chosen = partition(id)
        last_source = id
      end
      next unless chosen == part
      out.write(line)
      counts['examples'] += 1
      counts["#{example.fetch('role')}/label=#{example.fetch('label')}"] += 1
    end
  end
  raise "empty #{part} partition" if counts['examples'].zero?
  save_json("#{part}-counts.json", counts)
  exit
end

Dir.chdir(ROOT)
lock = File.open(File.join(RUN, 'run.lock'), 'w')
raise 'already running' unless lock.flock(File::LOCK_EX | File::LOCK_NB)
raise 'run already started; use a new output directory' if File.exist?(File.join(RUN, 'status.json'))
File.write(File.join(RUN, 'pid'), "#{Process.pid}\n")
epochs = []
batches = []
feeders = []
training_pid = nil
Signal.trap('TERM') { raise Interrupt, 'Terminated' }
def save_metrics(epochs, batches)
  times = batches.map { |batch| batch[:seconds] }
  mean = times.empty? ? nil : times.sum / times.length
  stdev = times.length > 1 ? Math.sqrt(times.sum { |t| (t - mean)**2 } / (times.length - 1)) : nil
  save_json('metrics.json', {epochs: epochs, batches: batches, batch_mean_seconds: mean, batch_stdev_seconds: stdev})
end
begin
  FileUtils.cp(INITIAL, File.join(RUN, 'nnue-initial.nnue'))
  FileUtils.cp(File.join(ROOT, 'target/release/learn'), File.join(RUN, 'learn'))
  source_stat = File.stat(SOURCE)
  learning_source = File.read(File.join(ROOT, 'nnue_training/src/bin/learn.rs'))
  rates = learning_source.scan(/const (INPUT|HIDDEN|OUTPUT)_LEARNING_RATE: f64 = ([\d.]+);/).to_h
  save_json('manifest.json', {
    revision: `git rev-parse HEAD`.strip,
    source: SOURCE, source_bytes: source_stat.size, source_mtime: source_stat.mtime.iso8601,
    initial_model: INITIAL, initial_sha384: Digest::SHA384.file(INITIAL).hexdigest,
    seed: SEED, split: 'source_id SHA-384 first 64 bits modulo 5: 0=test, otherwise=train',
    epochs: 9, epochs_per_command: 3, learning_rates: rates.transform_values(&:to_f),
    learner_source_sha384: Digest::SHA384.hexdigest(learning_source),
    learn_sha384: Digest::SHA384.file(File.join(RUN, 'learn')).hexdigest,
    loss: 'floating-point weighted BCE before integer export',
    timing: 'batch seconds include loading, training, loss and export; first epoch interval includes loading',
    stdev: 'sample standard deviation across completed 3-epoch commands'
  })
  %w[train test].each { |part| raise 'mkfifo failed' unless system('mkfifo', File.join(RUN, "#{part}.fifo")) }
  model = File.join(RUN, 'nnue-initial.nnue')
  1.upto(3) do |batch|
    current_stat = File.stat(SOURCE)
    raise 'Input changed during training; finish generation first' unless current_stat.size == source_stat.size && current_stat.mtime == source_stat.mtime
    last_epoch = batch * 3
    save_json('status.json', {state: 'running', batch: batch, epoch_range: [last_epoch - 2, last_epoch], completed_epochs: epochs.length, pid: Process.pid})
    output = File.join(RUN, format('nnue-epoch%02d.nnue', last_epoch))
    log = File.join(RUN, format('epochs%02d-%02d.log', last_epoch - 2, last_epoch))
    args = [File.join(RUN, 'learn'), '--model', model, '--examples', File.join(RUN, 'train.fifo'), '--test-examples', File.join(RUN, 'test.fifo'), '--epochs=3', '--output', output]
    command = "#{Shellwords.join(args)} 2>&1 | tee #{Shellwords.escape(log)}"
    File.open(File.join(RUN, 'commands.sh'), 'a') { |f| f.puts(command) }
    puts "batch #{batch}/3 started #{Time.now.iso8601}\n$ #{command}"
    started = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    last_loss_time = started
    feeders = %w[train test].map { |part| Process.spawn(RbConfig.ruby, __FILE__, SOURCE, INITIAL, RUN, '--feed', part, File.join(RUN, "#{part}.fifo")) }
    IO.popen(['/bin/bash', '-o', 'pipefail', '-c', command], pgroup: true) do |stream|
      training_pid = stream.pid
      stream.each_line do |line|
        print line
        match = line.match(/epoch (\d+)\/3: train_loss=([\d.eE+\-]+) test_loss=([\d.eE+\-]+)/)
        next unless match
        now = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        epoch = (batch - 1) * 3 + Integer(match[1])
        raise 'unexpected epoch order' unless epoch == epochs.length + 1
        epochs << {epoch: epoch, interval_seconds: now - last_loss_time, train_loss: Float(match[2]), test_loss: Float(match[3])}
        last_loss_time = now
        save_metrics(epochs, batches)
        save_json('status.json', {state: 'running', batch: batch, completed_epochs: epochs.length, pid: Process.pid})
        puts "global epoch #{epoch}/9: interval_seconds=#{epochs.last[:interval_seconds].round(3)}"
      end
    rescue StandardError, SignalException
      Process.kill('TERM', -stream.pid) rescue nil
      raise
    end
    training_pid = nil
    raise "batch #{batch} learner failed" unless $?.success?
    elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - started
    feeders.each { |pid| _, status = Process.wait2(pid); raise 'data feeder failed' unless status.success? }
    feeders = []
    current_stat = File.stat(SOURCE)
    raise 'Input changed during training; finish generation first' unless current_stat.size == source_stat.size && current_stat.mtime == source_stat.mtime
    raise 'missing epoch loss lines' unless epochs.length == last_epoch
    digest = Digest::SHA384.file(output).hexdigest
    changed = digest != Digest::SHA384.file(model).hexdigest
    batches << {batch: batch, through_epoch: last_epoch, seconds: elapsed, model: output, sha384: digest, changed_from_input: changed}
    save_metrics(epochs, batches)
    puts "batch #{batch}/3 completed: seconds=#{elapsed.round(3)} model_changed=#{changed} output=#{output}"
    model = output
  end
  save_json('status.json', {state: 'completed', completed_epochs: 9, pid: Process.pid})
rescue StandardError, SignalException => error
  Process.kill('TERM', -training_pid) rescue nil
  feeders.each { |pid| Process.kill('TERM', pid) rescue nil }
  save_json('status.json', {state: 'failed', completed_epochs: epochs.length, error: error.message, pid: Process.pid})
  warn "FAILED: #{error.full_message}"
  exit 1
ensure
  %w[train test].each do |part|
    pipe = File.join(RUN, "#{part}.fifo")
    File.unlink(pipe) if File.pipe?(pipe)
  end
end

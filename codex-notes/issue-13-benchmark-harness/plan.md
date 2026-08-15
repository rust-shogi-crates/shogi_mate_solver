# Plan: issue 13 benchmark harness

## Overview

Add a Rust benchmark harness binary plus a small comparison path that reads benchmark positions from JSONL files, evaluates each position with the two existing local evaluator stages, and emits machine-readable JSONL records. CI will run the harness for `origin/HEAD` and for the current branch, then compare result files for correctness and timing ratios.

The two evaluator stages will be:

- `df_pn`: run `mate_solver::df_pn::search::df_pn` and record its proof/disproof numbers and elapsed time.
- `eval`: run `mate_solver::eval::search::search` after df-pn on the same position and record the returned `Value`, mate predicate, and elapsed time.

This interpretation matches the current codebase structure. The issue says "both evaluators" but does not define them; the only two local solver stages with direct evaluator behavior are df-pn resolution and alpha-beta evaluation.

Each evaluator record will include `positions_inspected`. If a direct node-inspection counter is not already available, add narrowly scoped instrumentation to the search code rather than using timing alone.

The comparison will calculate correctness against the expected answer per input line and compare branch/base runtime with `ratio = current_elapsed_ms / base_elapsed_ms` for each evaluator and aggregate totals. A ratio below `1.0` means the current branch was faster; above `1.0` means slower.

Comparison output will also include detailed descriptive statistics for base elapsed times, current elapsed times, inspected-position counts, and per-position ratios:

- `mean`
- `median`
- `stddev`
- documented percentile fields, initially `p90`, `p95`, and `p99`

## Files to change

- `Cargo.toml`
  - Add a new binary target `benchmark_harness`.
  - Add `serde_json` for robust JSONL parsing and JSONL output.
- `Cargo.lock`
  - Update for `serde_json` if it is not already transitively locked.
- `src/bin/benchmark_harness.rs`
  - New CLI harness.
- `mate_solver/src/df_pn/search.rs`
  - Add minimal optional stats tracking for inspected positions.
- `mate_solver/src/eval/search.rs`
  - Add minimal optional stats tracking for inspected positions.
- `.github/workflows/rust.yml`
  - Add an automatic benchmark comparison job/step for PRs.
  - Upload generated base/current/comparison JSONL files as workflow artifacts.
- `benchmark/issue13-smoke.jsonl`
  - Add a tiny deterministic position set for CI smoke comparison, including expected answers.
- `README.md`
  - Document the harness invocation, accepted JSONL schema, and output records.

## Worktree and branch

- Worktree: current issue worktree only; do not create or use another checkout for implementation.
- Branch: `codex/issue-13-benchmark-harness`
- Base: `origin/HEAD`

The user explicitly requested work only in this existing worktree; no new worktree creation is needed.

## JSONL input schema

Because issue 13 says the schema is not defined yet, the harness will support a minimal object-only schema:

- Preferred object line: `{"id":"mate5","sfen":"3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1","expected":"mate"}`
- `id` is optional. If missing, use `<path>:<line>`.
- Blank lines are ignored.
- Expected answers are optional in ad hoc local runs and required for CI comparison:
  - `expected`: `"mate"` or `"nomate"`.
  - `expected_plies`: optional integer for mate positions.

Invalid JSON, missing `sfen`, invalid `expected`, missing CI-required expected values, or invalid SFEN will produce an error output record that includes the failing raw line text. The harness will continue through the input and exit nonzero after all failing lines have been emitted.

## Output format

The harness prints JSONL to stdout. In CI, stdout will be redirected to files such as `benchmark-base.jsonl`, `benchmark-current.jsonl`, and `benchmark-comparison.jsonl`; those files will be uploaded as workflow artifacts.

First record:

```json
{"type":"metadata","mode":"run","revision":"current","inputs":["benchmark/issue13-smoke.jsonl"]}
```

For a single-revision run, metadata identifies the run mode, the caller-supplied revision label, and input files. It avoids embedding a human diff stat because that is not useful for machine comparison.

Per evaluator result record:

```json
{"type":"result","id":"mate5","source":"positions.jsonl","line":1,"evaluator":"df_pn","elapsed_ms":1.23,"positions_inspected":42,"resolution":"mate","expected":"mate","correct":true,"proof_number":0,"disproof_number":4294967295}
{"type":"result","id":"mate5","source":"positions.jsonl","line":1,"evaluator":"eval","elapsed_ms":2.34,"positions_inspected":71,"resolution":"mate","expected":"mate","correct":true,"value":{"raw":7340031,"plies":6,"pieces":0,"futile":0}}
```

`proof_number` and `disproof_number` are the df-pn result values traditionally called phi and delta in the implementation. The output uses descriptive names instead of raw `phi`/`delta` so the record is understandable without reading the algorithm code.

Error record:

```json
{"type":"error","id":"positions.jsonl:3","source":"positions.jsonl","line":3,"stage":"parse","message":"...","raw_line":"..."}
```

Error records are emitted in the same JSONL stream as metadata and result records. They are not a separate file by default. When CI redirects harness output to JSONL files, error records are included in those files before the process exits nonzero.

Comparison summary record:

```json
{"type":"comparison","evaluator":"df_pn","positions":10,"correct_base":10,"correct_current":10,"elapsed_ms_base":{"total":15.0,"mean":1.5,"median":1.4,"stddev":0.2,"p90":1.8,"p95":1.9,"p99":2.0},"elapsed_ms_current":{"total":12.0,"mean":1.2,"median":1.1,"stddev":0.2,"p90":1.5,"p95":1.6,"p99":1.7},"ratio":{"mean":0.8,"median":0.79,"stddev":0.05,"p90":0.88,"p95":0.9,"p99":0.92}}
```

## Detailed implementation steps

1. Add `serde_json` dependency and the `benchmark_harness` binary target in `Cargo.toml`.
2. Implement manual CLI parsing in `src/bin/benchmark_harness.rs` consistent with existing binaries:
   - Usage for one revision: `benchmark_harness run --revision=current <positions.jsonl> [more.jsonl ...]`
   - Usage for comparison: `benchmark_harness compare --base base.jsonl --current current.jsonl`
   - Optional: `--verbose` to pass verbose logging into solver calls.
3. Implement JSONL reading:
   - Open each file with `BufReader`.
   - Ignore blank lines.
   - Parse each line as `serde_json::Value`.
   - Accept object with string `sfen` and optional string `id`.
   - Accept optional `expected` and `expected_plies` fields.
   - In strict mode, require `expected`.
4. Implement `evaluate_position`:
   - Parse `PartialPosition::from_usi(&format!("sfen {sfen}"))`.
   - For `df_pn`, allocate `DfPnTable::new(1 << 16)`, run `df_pn`, and time with `std::time::Instant`.
   - For `eval`, allocate fresh `DfPnTable` and `EvalTable`, run df-pn first to seed the df-pn table, then time only `eval::search`.
   - Record `resolution` from df-pn sentinel and `Value::is_mate()`.
   - Record `positions_inspected` from explicit stats instrumentation.
5. Implement comparison:
   - Read result JSONL files.
   - Match records by `id` and `evaluator`.
   - Count correct results for base/current.
   - Emit per-evaluator and aggregate comparison records with `ratio = current_elapsed_ms / base_elapsed_ms`.
   - Include mean, median, standard deviation, and percentile stats (`p90`, `p95`, `p99`) for elapsed times, inspected-position counts, and ratios.
   - Exit nonzero if current correctness is lower than base or any current record is incorrect against an expected answer.
6. Add `benchmark/issue13-smoke.jsonl` with a tiny expected-answer dataset from existing unit-test positions.
7. Update `.github/workflows/rust.yml`:
   - Ensure checkout fetches enough history to access the PR base.
   - Build/run the base revision harness into a JSONL file.
   - Build/run the current revision harness into a JSONL file.
   - Run `benchmark_harness compare` on both files.
   - Redirect comparison output into a JSONL file.
   - Upload the base, current, and comparison JSONL files as workflow artifacts.
   - Keep the workflow edit scoped to the benchmark comparison.
8. Implement JSON output records using `serde_json::json!` and one record per line.
9. Update `README.md` with a short usage section and examples.
10. Run formatting and validation.

## Alternatives considered

- Use Criterion benchmarks: rejected because issue asks to read JSONL files and compare current branch against `origin/HEAD`; a CLI harness is easier to use in automation and with arbitrary position files.
- Reuse `mate_solver::search`: rejected for the harness core because it hard-codes verbose output, expands branch data, and sets elapsed to `0.0`. Direct stage calls provide cleaner timing.
- Hand-parse JSONL without dependencies: rejected because the schema is JSONL and robust parsing matters. This plan requires explicit approval for adding `serde_json`.
- Shell out to `mate_solver` for each position: rejected because it would only benchmark the combined binary path and would not expose both evaluator stages directly.
- Have the harness itself check out `origin/HEAD`: rejected because it mixes git workspace mutation into a benchmark binary. CI should orchestrate revisions; the harness should run or compare files.

## Risks

- "Both evaluators" may mean something different from df-pn plus alpha-beta evaluation. If so, the plan should be adjusted before implementation.
- `origin/HEAD` must exist locally. The harness will degrade gracefully if the diff command fails.
- Search runtimes may be noisy; the harness reports elapsed wall time but does not attempt statistical benchmarking.
- Adding `serde_json` changes `Cargo.lock`.
- Building and running two revisions in CI can be slower than the current workflow. The smoke dataset must stay intentionally tiny.
- Exact "positions inspected" may require threading a stats object through recursive search functions. This is a small signature change, but it touches core search code.
- CI edits should stay focused on the benchmark comparison.

## Test strategy

- Formatting is covered by pre-commit hooks; still run the local formatter/check as needed while iterating.
- `cargo fmt --check`
- `cargo test`
- Manual smoke test with `benchmark/issue13-smoke.jsonl` containing:
  - An object line with one known mate SFEN from existing tests.
  - Expected answer fields.
- Temporary invalid-line smoke test to verify error records include `raw_line` and the process exits nonzero after emitting failures.
- Inspect CI YAML textually to verify generated JSONL files are uploaded as artifacts.
- Run:
  - `cargo run --bin benchmark_harness -- run --revision=current benchmark/issue13-smoke.jsonl`
  - `cargo run --bin benchmark_harness -- compare --base /tmp/base.jsonl --current /tmp/current.jsonl`

## Assumptions

- Plan approval includes approval to add `serde_json` as the JSON parser dependency.
- The harness is intended for local branch benchmarking, not for downloading benchmark position datasets.
- The initial JSONL schema can be minimal because the issue explicitly says the schema is undefined.
- CI comparison can use a very small bundled smoke dataset; larger benchmark suites can be supplied later without changing the harness.
- `origin/HEAD` comparison in CI means "run the base revision and current revision separately, then compare JSONL outputs", not "emit a git diff stat".
- Plans and docs should avoid machine-specific worktree paths because those paths are not meaningful on GitHub.

## Open questions

- None after incorporating the added notes.

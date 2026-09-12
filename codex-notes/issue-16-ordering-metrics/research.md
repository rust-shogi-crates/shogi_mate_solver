# Research: PR 2 Ordering Metrics and Benchmark Fixtures

## Relevant Files and Modules

- `benchmark_harness/src/main.rs`
  - Owns benchmark CLI parsing, JSONL input parsing, evaluator execution, JSONL result emission, comparison summaries, and optional HTML report generation.
  - Current subcommands are `run` and `compare`.
  - Current run output includes elapsed time, inspected positions, correctness, resolution, df-pn proof/disproof numbers, and eval `Value`.
- `benchmark/issue13-ci.jsonl`
  - Current CI fixture.
  - Contains one known mate position with `expected_plies: 5` and one known no-mate position.
  - Used directly by CI benchmark workflow.
- `README.md`
  - Documents benchmark input fields, run/compare commands, comparison ratio semantics, HTML output, and CI artifacts.
- `.github/workflows/benchmark.yml`
  - Runs release benchmark comparison on PRs to `main`.
  - Builds current branch fixture output, builds base branch fixture output in a worktree, compares, and uploads JSONL/HTML artifacts.
  - Uses `actions/checkout@v7` and `actions/upload-artifact@v7`; third-party Rust toolchain action is pinned to a commit hash with an inline comment.
- `mate_solver/src/df_pn/search.rs`
  - Exposes `SearchStats { positions_inspected }`.
  - Emits df-pn proof/disproof numbers.
- `mate_solver/src/eval/search.rs`
  - Exposes `SearchStats { positions_inspected }`.
  - Eval benchmark output combines eval positions inspected plus nested df-pn positions inspected during eval.
- `mate_solver/src/move_ordering.rs`
  - PR 1 abstraction centralizes current ordering behavior.
  - PR 2 benchmark metrics should remain compatible with default `Current` ordering.

## Execution Flow and Call Graph

Benchmark run flow:

1. `main`
2. `run`
3. `run_benchmark`
4. Read each JSONL input file line by line.
5. `parse_position_record`
6. `evaluate_position`
7. `evaluate_df_pn`
8. `evaluate_eval`

`evaluate_df_pn`:

- Parses a `PartialPosition` once in `evaluate_position`.
- Creates a fresh `DfPnTable`.
- Creates `dfpnsearch::SearchStats`.
- Calls `df_pn_with_stats`.
- Emits one JSON result object with:
  - `evaluator: "df_pn"`
  - elapsed time
  - `positions_inspected`
  - resolution and correctness
  - `proof_number`
  - `disproof_number`

`evaluate_eval`:

- Creates fresh df-pn and eval tables.
- Seeds df-pn by calling `df_pn_with_stats`.
- Calls `evalsearch::search_with_stats`.
- Emits one JSON result object with:
  - `evaluator: "eval"`
  - elapsed time
  - `positions_inspected` as `eval_stats.positions_inspected + df_pn_stats.positions_inspected`
  - resolution and correctness
  - `expected_plies`
  - serialized eval `Value`

Benchmark compare flow:

1. `compare_outputs`
2. `read_result_records` for base and current JSONL.
3. Match records by `(id, evaluator)`.
4. Accumulate correctness, elapsed time, inspected positions, elapsed ratios, and inspected-position ratios.
5. Emit one comparison object per evaluator and one aggregate `all` object.
6. Optionally write an HTML report with summary, elapsed-time table, inspected-position table, and ratio table.

## Current Benchmark Data and Outputs

Current committed fixture:

- `mate5-2022-05-18-3`
  - expected: mate
  - expected plies: 5
- `nomate-rook-hand-empty-board`
  - expected: nomate

The fixture is intentionally tiny and CI-friendly. It is not representative enough to evaluate ordering quality across different tactical shapes, mate lengths, and no-mate search patterns.

Current output can evaluate:

- correctness regression
- mate/no-mate resolution
- eval mate plies where `expected_plies` exists
- total inspected-position count per evaluator
- elapsed time per evaluator
- df-pn final proof/disproof numbers

Current output cannot directly evaluate:

- root candidate move count
- selected/best move
- number of generated candidate moves
- number of candidates skipped by pruning
- first-child quality or rank of eventual best move
- ordering-specific distribution of child table values
- attacker versus defender ordering behavior separately

## Data Structures and Invariants

- Benchmark input records are JSON objects with:
  - required `sfen`
  - optional `id`
  - optional `expected`, required under `--strict`
  - optional `expected_plies`
- Result records are JSON objects keyed by `type: "result"`.
- Compare logic assumes base/current result records can be matched by `(id, evaluator)`.
- If fixture inputs differ between base and current branches, compare emits missing-result errors.
- The CI workflow currently runs base using the base branch's own `benchmark/issue13-ci.jsonl` path. That matters when PRs add fixture rows: the base output may lack the new ids, causing compare failures unless workflow or compare semantics account for fixture additions.
- `positions_inspected` must remain numeric and present for compare parsing.
- `correct` is optional in parsed result records, but strict fixture runs should produce it.

## Existing Architectural Patterns

- The benchmark harness is a standalone workspace package with minimal dependencies.
- Output is newline-delimited JSON, including metadata, result, comparison, and error records in the same stream.
- The code favors simple structs and explicit JSON construction with `serde_json::json!`.
- Comparison uses `BTreeMap`/`BTreeSet` for deterministic ordering.
- HTML rendering is string-based and uses local escaping helpers.
- CI stores artifacts rather than posting comments or enforcing runtime thresholds beyond correctness.

## Naming Conventions

- Evaluator names are string literals: `"df_pn"`, `"eval"`, and aggregate `"all"` for comparison.
- JSON field names are snake_case.
- Benchmark ids are descriptive kebab-like strings.
- Stats fields include `total`, `mean`, `median`, `stddev`, `p90`, `p95`, `p99`.

## Error Handling Patterns

- Benchmark commands return `Result<(), ()>` and call `process::exit(1)` on failure.
- Parse/evaluate errors are emitted as JSONL `type: "error"` records rather than panicking.
- Compare emits JSONL errors for missing records and malformed result lines.
- HTML write failures become compare errors and fail the command.

## Typing Conventions

- Expected result is a local enum with `Mate` and `NoMate`.
- Records use owned `String` fields.
- Numeric benchmark metrics use `u64` for counts and `f64` for elapsed/statistical values.
- Percentiles are computed from sorted `Vec<f64>` values.

## Potential Pitfalls

- Adding committed fixture rows to `benchmark/issue13-ci.jsonl` can break current CI comparison because the base branch's fixture may not contain those rows.
- Extending result JSON is backward-compatible for compare as long as required existing fields remain present.
- Extending compare to require new fields is risky because older base output may not contain them.
- Measuring move-ordering quality directly may require new solver instrumentation; PR 2 should avoid changing core solver behavior unless clearly bounded.
- Elapsed time is noisy, especially for tiny fixtures; inspected-position counts are more stable for default behavior.
- Larger benchmark fixtures can make CI slow or flaky.
- If `.github/workflows/benchmark.yml` changes, GitHub Actions security rules apply: `actions/*` may use tags, non-`actions/*` actions must stay pinned to exact commit hashes with inline version comments.
- HTML report changes should preserve escaping and avoid injecting raw input values.

## Constraints

- PR 2 scope is benchmark metrics and benchmark fixtures/documentation, not learned ordering implementation.
- Keep committed benchmark data small enough for CI.
- Larger representative sets should be documented as external/generated unless explicitly approved for commit.
- Preserve existing benchmark CLI compatibility where practical.
- Default solver behavior should remain unchanged.
- If benchmark workflow changes are needed, keep action pinning policy intact.

## Unknowns

- Which ordering-quality metric is useful enough for PR 2 without adding invasive solver instrumentation.
- Whether representative issue #16 fixtures should be committed now, generated from existing tests, or documented as an external dataset.
- Whether CI should compare only common ids, run base on current fixture inputs, or avoid adding fixture rows that base cannot match.
- Whether benchmark output should distinguish eval's own inspected positions from nested df-pn inspected positions.
- Whether root move-count and best-move fields are sufficient for early ordering evaluation before PR 3 feature extraction.

# Research: issue 13 benchmark harness

## Relevant files and modules

- `Cargo.toml`: root package `shogi_mate_solver`, workspace root, binary targets `mate_solver` and `to_sfen`, dependency on local `mate_solver` crate.
- `src/bin/mate_solver.rs`: current CLI solver. Reads one SFEN line from stdin, optionally invokes an external USI engine, otherwise solves with local df-pn plus evaluator code. Supports `--verbose`, `--output=json`, `--move-format=...`, and `--engine-path=...`.
- `mate_solver/src/lib.rs`: library-facing search API. Exposes `search(position, timeout_ms) -> Answer`, but currently hard-codes `verbose = true`, returns branch data, and records elapsed as `0.0`.
- `mate_solver/src/df_pn/search.rs`: df-pn implementation. Public root entry point is `df_pn(&mut DfPnTable, &PositionWrapper, verbose) -> (u32, u32)`. `(u32::MAX, 0)` is used as no-mate.
- `mate_solver/src/eval/search.rs`: alpha-beta evaluator. Public `search(&PartialPosition, &mut DfPnTable, &mut EvalTable, verbose) -> Value` returns shortest-mate-style evaluation. `Value::is_mate()` distinguishes successful mate resolution.
- `mate_solver/src/eval/value.rs`: evaluation scalar with plies, pieces, and futile components. Used for comparison and mate/no-mate checks.
- `mate_solver/src/tt.rs`: transposition table implementation and aliases `DfPnTable` / `EvalTable`.
- `README.md`: documents the existing one-position `mate_solver` CLI.
- `.github/workflows/rust.yml`: current CI builds, tests, clippies, formats, documents, and runs `cargo bloat` for the two existing binaries. It does not currently run any benchmark comparison.

## Execution flow and call graph

Existing local solver flow in `src/bin/mate_solver.rs`:

1. Parse CLI options manually from `std::env::args()`.
2. Read a single SFEN string from stdin.
3. Convert it with `PartialPosition::from_usi("sfen ...")`.
4. If `--engine-path` is supplied, spawn the external engine and parse `checkmate ...` output.
5. Otherwise call `solve_myself`.
6. `solve_myself` creates a `DfPnTable` and `EvalTable`, calls `dfpnsearch::df_pn`, treats `(u32::MAX, 0)` as no-mate, calls `evalsearch::search`, and reconstructs a move sequence when `Value::is_mate()`.
7. The CLI prints either `nomate`, text moves, or a JSON array of move strings.

Existing library solver flow in `mate_solver/src/lib.rs`:

1. `search(&PartialPosition, timeout_ms)` ignores timeout except for the signature.
2. It creates a `DfPnTable` and `EvalTable`.
3. It calls `dfpnsearch::df_pn`.
4. It calls `evalsearch::search`.
5. If mate is found, it recursively records branch entries with `find_branches`.
6. It returns `Answer { inner, elapsed }`, with `elapsed` currently fixed at `0.0`.

Potential benchmark harness flow implied by the issue:

1. Determine a diff between `origin/HEAD` and the current branch.
2. Build or run the benchmark harness for both `origin/HEAD` and the current branch.
3. Read positions from one or more JSONL files.
4. For each position, evaluate with both evaluator stages.
5. Emit comparable timing/result records, then compare the two revisions and summarize correctness and runtime ratios.

## Data structures and invariants

- Positions are represented as `shogi_core::PartialPosition` parsed from USI/SFEN text using `shogi_usi_parser::FromUsi`.
- The CLI currently expects raw SFEN without the leading `sfen` keyword on stdin, then prepends `sfen ` before parsing.
- `DfPnTable::new(size)` requires `size` to be an even power of two.
- `EvalTable` stores `(Value, Option<Move>)`.
- `df_pn` no-mate sentinel is `(u32::MAX, 0)`.
- `Value::is_mate()` is the evaluator-level mate success predicate.
- Move output is easiest to compare as USI via `Move::to_usi_owned()`.
- External engine output is modeled as `checkmate <moves>` or `checkmate nomate`; local solver returns `Option<Vec<Move>>`.
- The df-pn implementation currently does not expose an explicit inspected-node counter. The closest existing countable signal is the number of entries stored in `DfPnTable`, but `tt.rs` must be checked before treating table occupancy as inspected positions. If exact inspection counts are required, search contexts or table APIs may need a small instrumentation change.

## Existing architectural patterns

- The root crate owns binaries and user-facing format conversion.
- Core search code lives in the `mate_solver` workspace member.
- CLI argument parsing is manual; there is no `clap` or other command parser.
- Existing JSON output in `mate_solver` is hand-written, not backed by `serde`.
- Tests are colocated inside modules rather than integration tests.
- Existing scripts (`run.sh`, `problems/fetch.sh`, `tt_experiments/run.py`) are thin utilities around binaries.

## Naming conventions

- Binary names are short and command-oriented: `mate_solver`, `to_sfen`.
- Internal solver functions use snake_case and descriptive names such as `solve_myself`, `find_mate_sequence`, `invoke_external_engine`.
- Search tables are named `df_pn` / `eval` or `evals`.
- Current module naming separates algorithm families as `df_pn` and `eval`.

## Error handling patterns

- Current binaries use `unwrap()` and `panic!()` for invalid input, failed parsing, and subprocess errors.
- Library `Answer` can express invalid/unknown through `Resolution`, but the current `search` implementation does not return parse errors because it receives an already parsed `PartialPosition`.
- Existing tests rely on `unwrap()` for fixed test positions.

## Typing conventions

- Public library result types are plain structs/enums, not serialized types.
- CLI options are private enums/structs in the binary.
- Search APIs pass `verbose: bool` explicitly except `mate_solver::search`, which hard-codes it internally.

## Potential pitfalls

- The issue says the JSONL schema is not defined. Any implementation must choose a minimal schema or support a permissive one.
- Adding JSONL parsing likely requires either `serde_json` or ad hoc parsing. `serde_json` is the robust option, but it adds a dependency and should be explicitly approved before implementation under dependency hygiene guardrails.
- The issue asks for "both evaluators"; in the current codebase this most likely means df-pn resolution and the alpha-beta `eval::search` path, not the external engine path. This is an inference from the module names and call graph.
- The user's notes clarify that the harness should run both revisions (`origin/HEAD` and current branch), compare results, include the correct ratio, and be exercised automatically in CI.
- The user's later notes clarify that comparison output should include detailed descriptive statistics: median, configurable or documented percentile(s), mean, standard deviation, and the same statistics for per-position runtime ratios.
- CI should retain the generated benchmark outputs as workflow artifacts so base/current run records and comparison records can be inspected after a job finishes.
- `mate_solver::search` currently forces verbose logging and has branch expansion overhead, so it may not be suitable as the benchmark evaluator unless changed or wrapped carefully.
- The current CLI `--output=json` only outputs a JSON array of moves for one position; it does not expose timings, df-pn result, eval value, parse failures, or per-position records.
- Comparing `origin/HEAD` and current branch inside a single binary is awkward because a running binary cannot swap its own code revision. A practical design is to make the benchmark binary run one revision and emit JSONL, then have CI or a wrapper script run the binary at each revision and compare the two output files.
- Benchmarking search code can be slow or noisy. Tests should use small known positions from existing unit tests.
- Workflow changes for this issue only need to add the benchmark comparison.
- Error records are part of the harness's JSONL output stream. In CI, stdout should be redirected to files, and those files should be uploaded as artifacts.

## Constraints

- Workflow requires this issue work to stay on the current issue worktree and branch `codex/issue-13-benchmark-harness`.
- The benchmark harness should keep the PR to one logical change.
- Do not rebase unless explicitly instructed.
- Do not add third-party dependencies without explicit user approval.
- Current Rust edition is 2024 with `rust-version = "1.85"`.
- Work only in this worktree.
- Keep workflow changes scoped to adding the benchmark comparison.

## Unknowns

- Exact JSONL schema expected by issue 13.
- Exact output format expected from the harness.
- Whether "both evaluators" means `df_pn` plus `eval::search`, local library `mate_solver::search` plus CLI `solve_myself`, or local solver plus external engine.
- Whether the harness should be a Rust binary, a benchmark under Cargo, or a script. A Rust binary is most consistent with existing binaries and direct API access.
- Whether the diff between `origin/HEAD` and current branch should be emitted as metadata, used to select changed positions, or used only for reproducibility reporting.

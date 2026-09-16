# Research: issue 16 NNUE move selection

## Relevant files and modules

- `mate_solver/src/df_pn/search.rs`: df-pn proof/disproof search. It expands attacker `all_checks()` at OR nodes and defender `all_evasions()` at AND nodes, currently using only simple deterministic move ordering before child selection.
- `mate_solver/src/eval/search.rs`: alpha-beta-style mate-line evaluator. It also expands attacker checks and defender evasions, currently using df-pn transposition-table deltas as the main ordering signal.
- `mate_solver/src/position_wrapper.rs`: wraps `shogi_core::PartialPosition`, owns the incremental Zobrist hash, exposes `inner()`, `all_checks()`, `all_evasions()`, and `make_move()`. Any NNUE feature extractor will likely need to read board, hand, side-to-move, and make/unmake or clone/apply moves through this wrapper.
- `mate_solver/src/eval/value.rs`: compact mate evaluation scalar. Smaller values are better for the attacker. It is separate from any static position evaluator.
- `mate_solver/src/tt.rs`: fixed-size transposition table used by both df-pn and eval search. Existing table values are proof/disproof numbers or mate-line `Value`; there is no NNUE cache type.
- `mate_solver/src/lib.rs`: public library API and branch reconstruction. It depends on the same `df_pn` and `eval` search entry points used by the CLI.
- `src/bin/mate_solver.rs`: user-facing CLI. It can solve locally or call an external USI engine, but it has no option for loading evaluator weights.
- `benchmark_harness/src/main.rs`: JSONL benchmark runner/comparator. It already records elapsed time and `positions_inspected` for `df_pn` and `eval`, so it can be used to evaluate move-ordering changes.
- `.github/workflows/benchmark.yml`: runs the benchmark harness on PRs and uploads JSONL/HTML artifacts. Third-party actions must stay pinned to exact commits with inline version comments.
- `benchmark/issue13-ci.jsonl`: tiny deterministic benchmark fixture, useful for smoke tests but too small to validate NNUE quality.
- `mate_solver/ARCHITECTURE.dot`: high-level dependency graph showing `eval` depends on position wrapper, df-pn table, and eval table.

## Issue context

Issue #16 is titled "Better move selection". It says the solver currently tries all possible moves in a fixed order, which wastes time on moves that are not promising from a human perspective. The listed ideas are NNUE learning and other machine-learning approaches such as KP or KPP.

The issue is about move selection/order, not replacing the mate proof algorithm. A scoped implementation should therefore rank generated legal checking/evasion moves while preserving exact df-pn and alpha-beta correctness.

## Execution flow and call graph

Local CLI solving in `src/bin/mate_solver.rs`:

1. Parse one SFEN line from stdin into `PartialPosition`.
2. `solve_myself` creates `DfPnTable` and `EvalTable`.
3. `dfpnsearch::df_pn` resolves immediate mate/no-mate using `PositionWrapper`.
4. If df-pn does not prove no-mate, `evalsearch::search` computes a mate-line `Value`.
5. `find_mate_sequence` repeatedly calls `alpha_beta_me` and `alpha_beta_you` to reconstruct the line.

Library solving in `mate_solver/src/lib.rs` follows the same core sequence:

1. Create `DfPnTable` and `EvalTable`.
2. Run `dfpnsearch::df_pn`.
3. Run `evalsearch::search`.
4. Use recursive `find_branches` to record branch choices.

df-pn expansion path:

1. `df_pn_with_stats` calls `mid_with_stats` on the root as an OR node.
2. `mid_with_stats` increments `SearchStats::positions_inspected`.
3. It fetches current proof/disproof values from `DfPnTable`.
4. It generates moves with `position.all_checks()` for OR nodes or `position.all_evasions()` for AND nodes.
5. It sorts generated moves so normal moves precede drops, and drops prefer lower-value pieces.
6. It clones the position and applies every move once to build `(Move, child_hash)` pairs.
7. The loop repeatedly calls `select_child`, which chooses the child with the smallest current delta from the transposition table.
8. It recurses into the selected child with flipped `NodeKind`.

eval expansion path:

1. `search_with_stats` calls `alpha_beta_me_with_stats` with a fixed root beta of 40 plies.
2. `alpha_beta_me_with_stats` increments eval stats, checks df-pn/eval tables, and optionally calls bounded df-pn to prune no-mate positions.
3. Attacker moves come from `position.all_checks()`.
4. The attacker move list is sorted by the child df-pn table delta, defaulting to 1 for unknown child positions.
5. Each move is cloned/applied and searched by `alpha_beta_you_with_stats`.
6. `alpha_beta_you_with_stats` mirrors this for defender evasions, maximizing the attacker's resulting mate value and also sorting by child df-pn delta.

## Data structures and invariants

- `PositionWrapper` stores both `PartialPosition` and a collision-sensitive `u64` Zobrist hash. `make_move` must keep them in sync.
- `PositionWrapper::inner()` exposes an immutable `PartialPosition`, so static feature extraction can be implemented without changing wrapper internals.
- Generated moves are `shogi_core::Move`, with `Normal { from, to, promote }` and `Drop { piece, to }` variants.
- Attacker moves are checking moves only; defender moves are all legal evasions.
- `df_pn` no-mate sentinel is `(u32::MAX, 0)`. Mate-proof success uses `(0, u32::MAX)` in existing tests.
- `eval::Value::INF` represents no mate. Smaller `Value` is better for the attacker; defender search chooses larger values.
- `DfPnTable::new(size)` requires `size` to be an even power of two.
- Search APIs clone positions rather than maintaining an undo stack. A runtime NNUE evaluator can initially follow this clone/apply model for simplicity, but a performant incremental accumulator would need a more deliberate state API.
- Existing stats count visited search positions, not generated/scored moves.

## Existing architectural patterns

- Core engine code lives in the `mate_solver` workspace crate; binaries and harnesses live outside it.
- Search modules are algorithm-oriented (`df_pn`, `eval`) rather than feature-flag or engine-option oriented.
- Public APIs pass explicit table references and `verbose: bool`; there is no central search options struct for the recursive search functions.
- CLI parsing is manual; no command-line parser dependency is currently used.
- Tests are colocated in modules and use fixed SFEN positions.
- Performance validation uses a separate `benchmark_harness` workspace member and benchmark workflow.
- Dependencies are intentionally small and use constrained feature sets where configured.

## Naming conventions

- Existing modules use lowercase snake_case names such as `df_pn`, `position_wrapper`, and `eval`.
- Search entry points with instrumentation are suffixed `_with_stats`.
- Search context types are named `SearchCtx`; stats types are named `SearchStats`.
- Table variables are named `df_pn`, `dfpn_tbl`, `eval`, or `evals`.
- Move ordering comments describe intent in Japanese; new code comments should be English by global preference.

## Error handling patterns

- Core search code mostly assumes legal internal state and uses `panic!`, `debug_assert!`, and `unreachable!` for impossible conditions.
- CLI code uses `unwrap()` for fixed-format input and subprocess handling.
- Benchmark harness emits JSONL error records and exits nonzero after processing all lines.
- Existing public search API has `Answer`/`Resolution`, but current internal search functions do not return `Result`.

## Typing conventions

- The engine uses concrete structs and free functions rather than traits for search behavior.
- `SearchStats` structs are plain `Copy`/`Default` counters.
- Transposition tables are generic over `Copy` values.
- There is no existing type for static evaluation, model weights, feature IDs, or move scores.
- If NNUE is introduced, likely new types include a model/weights struct, a feature extractor, and an ordering mode/options type.

## Potential pitfalls

- "NNUE" usually implies learned weights and a training pipeline, but the repository currently has no dataset, trainer, weight format, loading path, or model-cache convention.
- Adding a real NNUE implementation will probably require at least one new dependency for model serialization or compressed embedded data unless weights are represented as Rust constants. Global dependency hygiene requires approval before adding essential third-party dependencies.
- Move ordering must not change legal move generation, proof/disproof semantics, mate/no-mate results, or selected shortest mate value.
- In df-pn, the initial list sort only affects tie/default ordering before transposition-table values become informative. `select_child` ultimately chooses by child delta, so an NNUE score may need to be integrated into tie-breaking or initial child priorities rather than as a simple list sort.
- In eval search, sorting by df-pn child delta already uses a strong search-derived signal. NNUE ordering should compose with this rather than discard it blindly.
- Scoring every child requires cloning and applying every move, which df-pn already does to compute child hashes. Eval currently clones/applies inside the sort key, so adding static scoring there may add overhead unless child metadata is computed once.
- `sort_unstable_by_key` is deterministic for keys but not stable for equal keys. If reproducible equal-score ordering matters, include a deterministic tie-breaker.
- A model that ranks attacker checks well may rank defender evasions poorly. The plan should distinguish OR-node and AND-node objectives.
- Benchmark fixture `benchmark/issue13-ci.jsonl` is too small to prove improvement. A meaningful NNUE needs a larger curated benchmark/training set, likely external to this repo or added separately.
- Binary size and runtime may change if weights are embedded. Global workflow requires release binary size reporting for changes that affect compiled binary size.
- Workflows use third-party pinned actions. Any `.github/workflows/*.yml` edit must preserve exact commit pinning for non-`actions/*` actions.

## Constraints

- Worktree for issue 16: dedicated issue worktree on branch `codex/issue-16-nnue`, based on `origin/main`.
- Keep the PR to one logical change for issue 16.
- Do not rebase unless explicitly requested.
- Do not implement production code during research/planning approval phases.
- Do not add third-party dependencies for core behavior without explicit user approval.
- Current root crate uses Rust edition 2024 and `rust-version = "1.85"`; `mate_solver` crate uses edition 2021.
- Existing CI uses `RUSTFLAGS: --deny warnings` and `RUSTDOCFLAGS: --deny warnings`.
- Existing `.pre-commit-config.yaml` runs formatting, clippy, and tests; validators may need to be run after implementation edits.

## Unknowns

- Whether the desired first milestone is a true learned NNUE with training, a runtime hook that can load/use a model once one exists, or a simpler KP/KPP-style learned scorer.
- What training data should be used: solved mate problems, self-generated df-pn labels, external engine labels, or human-authored problem solutions.
- What target should be learned: best first move, proof-number reduction, mate/no-mate child prior, mate length, or attacker/defender separate policies.
- Whether model weights should be embedded into the binary, loaded from a file, or optional behind a CLI/API option.
- Whether NNUE should affect df-pn only, eval only, or both. Issue #16 names fixed move order generally, and both search layers order moves today.
- Whether an implementation PR should include the trainer/dataset, or only the inference/runtime integration plus documented model contract.
- Acceptable dependency choices for model serialization, numeric arrays, or training, if any.
- Performance acceptance threshold: faster mean elapsed time, fewer inspected positions, no regression on p95, or another benchmark criterion.

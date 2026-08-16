# Research: PR 1 Move-Ordering Abstraction

## Relevant Files and Modules

- `mate_solver/src/df_pn/search.rs`
  - Contains df-pn root and recursive search entry points.
  - Generates attacker checks for `NodeKind::Or` and defender evasions for `NodeKind::And`.
  - Applies the only explicit df-pn move ordering currently present.
  - Selects children by proof/disproof table values in `select_child`.
- `mate_solver/src/eval/search.rs`
  - Contains alpha-beta-style mate line evaluation.
  - Generates attacker checks in `alpha_beta_me_with_stats`.
  - Generates defender evasions in `alpha_beta_you_with_stats`.
  - Applies child-ordering based on df-pn table `delta` values in both attacker and defender branches.
- `mate_solver/src/position_wrapper.rs`
  - Wraps `PartialPosition`, tracks zobrist hash, generates checks/evasions, and applies moves.
  - `all_checks()` already sorts promotions first before callers apply additional ordering.
  - `all_evasions()` returns legality-lite move order directly.
- `mate_solver/src/lib.rs`
  - Public library search wrapper used outside the CLI.
  - Calls df-pn and eval search directly, then explores branches using raw `position.all_evasions()` for defender alternatives.
- `src/bin/mate_solver.rs`
  - CLI entry point.
  - Calls df-pn first, then eval search and mate-sequence reconstruction.
  - Has `Opts` for CLI behavior, but no solver-specific config beyond verbosity/output/format/external engine.
- `benchmark_harness/src/main.rs`
  - Calls `df_pn_with_stats` and `search_with_stats` directly for measurement.

## Execution Flow and Call Graph

The CLI path is:

1. `src/bin/mate_solver.rs::main`
2. Parse one SFEN line from stdin.
3. `solve_myself`
4. `dfpnsearch::df_pn`
5. `evalsearch::search`
6. `find_mate_sequence`
7. alternating `evalsearch::alpha_beta_me` / `alpha_beta_you`

The benchmark path is:

1. `benchmark_harness::run_benchmark`
2. `evaluate_position`
3. `evaluate_df_pn` calls `df_pn_with_stats`
4. `evaluate_eval` seeds df-pn with `df_pn_with_stats`, then calls `search_with_stats`

The library path is:

1. `mate_solver::search`
2. `dfpnsearch::df_pn`
3. `evalsearch::search`
4. `find_branches`, which calls eval search and also enumerates defender evasions directly.

## Current Move Ordering

`PositionWrapper::all_checks()` sorts generated checking moves so promotions come first:

- key: `!mv.is_promoting()`
- effect: promoting checks precede non-promoting checks.

`df_pn::mid_with_stats()` then sorts the generated moves by normal/drop category:

- normal moves receive key `0`
- drops receive key `60 - piece_kind`
- comment says drops prioritize lower-value pieces.
- because Rust sort is unstable, equal-key moves are not guaranteed to preserve the original generation order.

df-pn then creates child hashes in that sorted order. Later selection is not a simple iteration order:

- `select_child` chooses the child with minimum `delta`.
- `delta_2` stores the second-smallest delta.
- if a child has `phi == u32::MAX`, the function returns immediately with the current best child.
- for equal `delta`, the first currently encountered child remains best because the comparison is strictly `<`.

`eval::alpha_beta_me_with_stats()` orders attacker checks by child df-pn table `delta`:

- clone position
- apply candidate move
- fetch child hash from `df_pn`
- known child uses stored `delta`; unknown child uses `1`
- lower key is searched first.

`eval::alpha_beta_you_with_stats()` applies the same delta-based ordering to defender evasions.

`mate_solver::find_branches()` does not apply ordering abstraction today:

- attacker side follows the single best move returned by eval search.
- defender side starts from raw `position.all_evasions()`.
- if eval search returned a defender best move, it moves that best move to the front.

## Data Structures and Invariants

- `Move` values are copied and used as orderable candidates.
- `PositionWrapper` is cloned to evaluate candidate child hashes.
- `Key` is a `u64` zobrist hash and must remain consistent with `PositionWrapper::make_move`.
- `DfPnTable` stores `(phi, delta)` values keyed by position hash.
- `EvalTable` stores `(Value, Option<Move>)` values keyed by position hash.
- df-pn correctness depends on proof/disproof number propagation, not on learned ordering. Ordering may affect search work and selected mate line, but should not change mate/no-mate truth.
- Existing public search entry points have no move-ordering config parameter.
- Existing default behavior must remain exactly the current ordering for PR 1.

## Existing Architectural Patterns

- Search modules expose simple public wrappers that construct default stats/context and delegate to `_with_stats` variants.
- Search functions pass many explicit parameters rather than storing a shared search context object.
- Existing tests live in the same module as the search implementation.
- The project uses small dedicated modules under `mate_solver/src/`, re-exported from `mate_solver/src/lib.rs`.
- The current code favors concrete functions and structs over trait-heavy abstractions.

## Naming Conventions

- df-pn functions use names such as `df_pn`, `df_pn_with_stats`, `mid`, `mid_with_stats`.
- eval functions use names such as `search`, `search_with_stats`, `alpha_beta_me`, `alpha_beta_you`.
- Public stats structs are named `SearchStats` inside each search module.
- Japanese comments are present in existing search code; newly added comments should be English by workflow preference.

## Error Handling Patterns

- Solver internals mostly assume valid generated moves and use `unwrap`, `unreachable!`, or direct propagation through table lookups.
- CLI argument parsing panics on unknown move format.
- Search functions generally return sentinel values rather than `Result`.

## Typing Conventions

- `NodeKind` distinguishes attacker OR nodes and defender AND nodes in df-pn.
- Eval search distinguishes attacker and defender through separate functions.
- `Value` represents mate-line quality and is copied.
- Current ordering keys are simple integer values.

## Potential Pitfalls

- Replacing `sort_unstable_by_key` with stable sort could change equal-key ordering, even if the key is identical.
- Moving ordering into a helper must preserve exact key calculations and unstable-sort behavior if default behavior must remain exact.
- Adding config parameters to every public function could create noisy API churn; PR 1 should thread only minimal options.
- Eval ordering currently recomputes child positions while sorting, then recomputes positions again during search. A helper that eagerly builds child positions may accidentally change allocation/runtime behavior.
- df-pn `select_child` relies on child slice order for tie-breaking and early return behavior.
- `PositionWrapper::all_checks()` already has an ordering responsibility, so a new abstraction must account for that pre-existing promotion-first order.
- `mate_solver::find_branches()` uses raw defender evasions for branch presentation, which may or may not be in scope for PR 1 depending on whether the abstraction is only for search ordering.

## Constraints

- No NNUE, model files, scorer interface, training code, or new dependencies should be added in PR 1.
- Default behavior must remain current behavior.
- The abstraction should create a controlled insertion point for future ordering modes.
- Any options/config should be minimal and only threaded where later ordering mode selection needs it.
- The PR should stay a single logical change.

## Unknowns

- Whether future ordering modes should apply to branch presentation in `mate_solver::find_branches` or only to search exploration.
- Whether exact default preservation requires preserving unstable-sort tie behavior, or only preserving the effective ordering for current tests/benchmarks.
- Whether public API compatibility matters for `mate_solver` crate consumers outside this repository.

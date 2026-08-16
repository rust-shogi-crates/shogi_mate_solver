# Plan: PR 1 Move-Ordering Abstraction

## Overview

Add a small internal move-ordering module that centralizes the current search-ordering behavior while preserving the current default path. This PR should make later learned ordering modes possible without adding NNUE, scorers, model files, dependencies, or behavior changes.

The default implementation will keep the existing semantics:

- df-pn still gets checks/evasions from `PositionWrapper`, then applies the current normal/drop key ordering.
- eval search still orders attacker and defender candidates by child df-pn `delta`.
- existing public wrappers continue to work through default options.

## Files to Change

- `mate_solver/src/move_ordering.rs`
- `mate_solver/src/lib.rs`
- `mate_solver/src/df_pn/search.rs`
- `mate_solver/src/eval/search.rs`
- `src/bin/mate_solver.rs`
- `codex-notes/issue-16-move-ordering-hooks/feature_list.json`

## Worktree and Branch

- Branch: `codex/issue-16-move-ordering-hooks`
- Worktree: dedicated issue 16 move-ordering hooks worktree
- Base: `origin/main`

## Detailed Implementation Steps

1. Add `mate_solver/src/move_ordering.rs`.
   - Define a small options/config type, likely:

     ```rust
     #[derive(Clone, Copy, Debug, Default)]
     pub struct MoveOrderingOptions {
         pub mode: MoveOrderingMode,
     }

     #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
     pub enum MoveOrderingMode {
         #[default]
         Current,
     }
     ```

   - Add helper functions for the existing ordering behavior:
     - df-pn child generation/order for a node kind.
     - eval ordering by child df-pn delta.
   - Keep helper signatures concrete and dependency-free.

2. Re-export the module from `mate_solver/src/lib.rs`.
   - Add `pub mod move_ordering;`.

3. Thread options through df-pn with minimal API churn.
   - Keep existing `df_pn`, `df_pn_with_stats`, `mid`, and `mid_with_stats` wrappers behavior-compatible.
   - Add `_with_options` variants only where needed so existing callers remain valid.
   - Inside df-pn expansion, replace the inline move sort with the helper.

4. Thread options through eval search with minimal API churn.
   - Keep existing `search`, `search_with_stats`, `alpha_beta_me`, and `alpha_beta_you` behavior-compatible.
   - Add `_with_options` variants only where needed for later mode selection.
   - Replace inline child-delta sorting with the helper.
   - Ensure df-pn calls made from eval use the same options.

5. Thread default options from the CLI.
   - Extend CLI `Opts` with a `move_ordering` field defaulting to current behavior.
   - Do not add a user-facing ordering mode flag yet unless needed for plumbing; PR 1 should preserve the current CLI surface.
   - Pass options into df-pn/eval calls through the new option-aware APIs.

6. Add focused default-preservation tests.
   - Test df-pn ordering helper against a handcrafted move list that includes normal moves and drops.
   - Test eval ordering helper against a small position/table setup where child `delta` controls order.
   - Keep existing solver tests intact.

7. Run validation.
   - `cargo fmt --check`
   - `cargo test --locked`
   - `cargo clippy --all-targets --locked -- -D warnings`
   - Manual functionality test:
     - run `mate_solver` with a known mate SFEN and inspect the answer.
     - run `mate_solver` with a known no-mate SFEN and inspect `nomate`.
     - run the known mate SFEN with `--verbose` and confirm the CLI still behaves normally.
   - Release benchmark comparison:
     - generate a baseline from `origin/main`.
     - generate current output from the PR branch.
     - compare outputs and confirm correctness does not change.

## Alternatives Considered

- Add a trait-based scorer immediately.
  - Rejected for PR 1 because the roadmap says not to add NNUE/model/scorer behavior yet. A concrete enum/options type is enough for a future extension point.
- Change `PositionWrapper::all_checks()` to stop sorting promotions first.
  - Rejected because that would alter existing behavior and make default preservation harder.
- Build ordered child structs containing move, position, and hash.
  - Rejected for this PR because eval currently recomputes child positions during sorting and search; changing that may affect runtime and allocation behavior beyond the abstraction goal.
- Add a CLI flag for move ordering mode now.
  - Rejected unless implementation reveals it is needed. There is only one mode in PR 1, so a visible flag would create surface area without user value.

## Risks

- Accidentally changing unstable sort tie behavior.
- Adding too many new APIs and making later maintenance harder.
- Forgetting df-pn calls nested inside eval search.
- Accidentally changing CLI output or benchmark JSON behavior.
- Benchmark timings may vary even if correctness and search counts are unchanged.

## Test Strategy

- Unit tests for helper-level ordering behavior.
- Existing df-pn and eval tests for solver correctness.
- Full workspace test and clippy validation.
- Manual `mate_solver` CLI checks for known mate/no-mate behavior.
- Benchmark harness output comparison for expected mate/no-mate and mate plies.

## Assumptions

- PR 1 should preserve the current public behavior and default ordering.
- It is acceptable to add option-aware internal/public variants while keeping existing wrappers.
- Future learned ordering will be added in later PRs, so PR 1 should avoid choosing model architecture or dependencies.
- Branch presentation in `mate_solver::find_branches()` is not the primary search-ordering hook; this PR can leave it unchanged unless option plumbing requires a local default.

## Open Questions

- Should later PRs expose move-ordering mode through the CLI, benchmark harness, or both?
- Should branch presentation eventually use the same move-ordering abstraction, or remain separate from search ordering?

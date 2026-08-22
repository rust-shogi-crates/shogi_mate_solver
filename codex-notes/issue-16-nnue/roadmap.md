# Roadmap: issue 16 NNUE move selection

## Purpose

Issue #16 is large enough to split across multiple PRs. This roadmap is the umbrella decomposition for the issue; it is not an implementation plan and should not be treated as the `plan.md` for a single PR.

Each PR below should get its own branch, worktree, `codex-notes/<task-slug>/plan.md`, and `codex-notes/<task-slug>/feature_list.json`. Implementation should begin only after the specific PR plan is approved.

## Goal

Improve move selection for mate solving by introducing learned or learnable move-ordering signals while preserving exact solver correctness.

The intended first useful outcome is not "NNUE replaces search". The solver should still prove mate/no-mate through df-pn and evaluate mate lines through the existing search. NNUE or related learned scoring should only change the order in which legal candidate moves are explored.

## Success Metrics

- Correctness: all existing tests continue to pass, and benchmark records preserve expected mate/no-mate and expected mate plies.
- Search work: inspected positions decrease or do not regress materially on representative benchmark sets.
- Runtime: release-mode benchmark elapsed time improves or does not regress materially after accounting for scoring overhead.
- Determinism: identical inputs and weights produce identical move ordering and solver output.
- Maintainability: each PR preserves a clear default path that matches current behavior unless a new ordering mode is explicitly enabled.

## Proposed PR Sequence

### PR 1: Move-Ordering Abstraction

- Suggested branch: `codex/issue-16-move-ordering-hooks`
- Suggested worktree: issue-16 move-ordering hooks worktree
- Scope:
  - Add an internal move-ordering abstraction or helper functions that centralize the existing ordering behavior.
  - Preserve current ordering exactly by default.
  - Thread a small options/config value only as far as needed to select ordering mode later.
- Likely files:
  - `mate_solver/src/df_pn/search.rs`
  - `mate_solver/src/eval/search.rs`
  - Possibly `mate_solver/src/move_ordering.rs`
  - `mate_solver/src/lib.rs`
  - `src/bin/mate_solver.rs`
- Validation:
  - `cargo fmt --check`
  - `cargo test --locked`
  - `cargo clippy --all-targets --locked -- -D warnings`
  - Compare benchmark output before/after and confirm no correctness changes.
- Notes:
  - This PR should not add NNUE, model files, or training code.
  - This PR is the safest base for later experimentation.

### PR 2: Ordering Metrics and Benchmark Fixtures

- Suggested branch: `codex/issue-16-ordering-metrics`
- Suggested worktree: issue-16 ordering metrics worktree
- Scope:
  - Extend benchmark output enough to evaluate move-ordering quality.
  - Add or document a representative benchmark set for issue #16 beyond the tiny CI fixture.
  - Keep benchmark data small enough for CI if committed; larger data should be external or generated.
- Likely files:
  - `benchmark_harness/src/main.rs`
  - `benchmark/`
  - `README.md`
  - `.github/workflows/benchmark.yml` if CI behavior changes
- Validation:
  - `cargo run --release --locked -p benchmark_harness -- run --strict --revision=current benchmark/issue13-ci.jsonl`
  - `cargo run --release --locked -p benchmark_harness -- compare --base <base.jsonl> --current <current.jsonl>`
- Notes:
  - If workflows are edited, keep `actions/*` on stable tags and all other actions pinned to exact commit hashes with inline version comments.

### PR 3: Static Feature Extraction

- Suggested branch: `codex/issue-16-feature-extraction`
- Suggested worktree: issue-16 feature extraction worktree
- Scope:
  - Add deterministic feature extraction for positions and candidate moves.
  - Start with features needed by a future learned scorer: side to move, king-relative piece locations, hands, move kind, from/to squares, promotion/drop flags, and attacker/defender node role.
  - Add focused unit tests for feature IDs and stability.
- Likely files:
  - `mate_solver/src/position_wrapper.rs`
  - Possibly `mate_solver/src/features.rs`
  - Possibly `mate_solver/src/move_ordering.rs`
  - `mate_solver/src/lib.rs`
- Validation:
  - Unit tests for stable feature IDs across known SFEN positions.
  - `cargo test --locked`
- Notes:
  - This PR should avoid selecting a neural-network crate or training framework.
  - Keep the feature format independent of a specific model file format.

### PR 4: Baseline Learned-Score Interface

- Suggested branch: `codex/issue-16-learned-score-interface`
- Suggested worktree: issue-16 learned-score interface worktree
- Scope:
  - Add a model-neutral scorer interface that maps extracted features or child positions to integer move-ordering scores.
  - Provide a trivial built-in scorer for tests, such as zero scores or a small hand-written fixture model.
  - Integrate score-based tie-breaking behind an explicit mode, preserving current default behavior.
- Likely files:
  - `mate_solver/src/move_ordering.rs`
  - `mate_solver/src/features.rs`
  - `mate_solver/src/df_pn/search.rs`
  - `mate_solver/src/eval/search.rs`
- Validation:
  - Tests that prove current default ordering is unchanged.
  - Tests that prove a fixture scorer changes order only where enabled.
  - Benchmark smoke test.
- Notes:
  - This creates the runtime integration point before committing to true NNUE internals.

### PR 5: NNUE Inference Runtime

- Suggested branch: `codex/issue-16-nnue-inference`
- Suggested worktree: issue-16 NNUE inference worktree
- Scope:
  - Add a minimal deterministic NNUE-style inference implementation for move ordering.
  - Decide whether weights are embedded Rust constants or loaded from a file.
  - Add a tiny non-quality fixture model for correctness tests.
- Likely files:
  - `mate_solver/src/nnue.rs`
  - `mate_solver/src/features.rs`
  - `mate_solver/src/move_ordering.rs`
  - `mate_solver/Cargo.toml` if dependencies are approved
  - Possibly `src/bin/mate_solver.rs` for model/mode selection
- Validation:
  - Unit tests for inference arithmetic.
  - Deterministic ordering tests with fixture weights.
  - Release binary size measurement before/after.
  - Benchmark smoke test.
- Notes:
  - Adding dependencies for serialization, numeric arrays, or model loading requires explicit approval before implementation.
  - If weights are embedded, report release binary size impact.

### PR 6: Training and Export Pipeline

- Suggested branch: `codex/issue-16-nnue-training`
- Suggested worktree: issue-16 NNUE training worktree
- Scope:
  - Add tooling to generate training examples and export runtime weights.
  - Define labels clearly: best move, child proof/disproof improvement, mate length, or search-work reduction.
  - Keep generated large datasets and trained weights out of the repository unless explicitly approved.
- Likely files:
  - A new training crate or scripts directory
  - `README.md` or `docs/`
  - Possibly `benchmark_harness/` if it emits training examples
- Validation:
  - Small deterministic training/export smoke test.
  - Round-trip test: exported fixture weights load in runtime inference.
- Notes:
  - Training dependencies are likely separate from runtime dependencies and should not be forced into the solver crate.

### PR 7: Trained Model Rollout

- Suggested branch: `codex/issue-16-trained-model-rollout`
- Suggested worktree: issue-16 trained model rollout worktree
- Scope:
  - Add or reference a real trained model.
  - Enable it through an explicit option first; consider default enablement only after benchmark evidence.
  - Document model provenance, training data, and benchmark results.
- Likely files:
  - Runtime model asset or documented external artifact
  - `src/bin/mate_solver.rs`
  - `README.md`
  - `benchmark/` or benchmark result docs
- Validation:
  - Full correctness tests.
  - Release benchmark comparison against the previous default.
  - Release binary size measurement if the model is embedded.
- Notes:
  - This PR should be evidence-driven. If the model does not improve search work or runtime, do not enable it by default.

## Recommended First PR

Start with PR 1, the move-ordering abstraction. It gives later NNUE work a controlled insertion point and lets tests prove that no behavior changes before learned scoring is introduced.

The first PR-specific plan should answer:

- What exact ordering helper type/function will be introduced?
- How will df-pn child selection preserve current proof/disproof semantics?
- How will eval search avoid recomputing child positions more than necessary?
- What tests prove default behavior is unchanged?

## Cross-PR Design Decisions

- Default behavior should remain current behavior until benchmark data justifies changing it.
- Ordering code should distinguish attacker OR nodes and defender AND nodes because their scoring objectives are different.
- Learned scores should be deterministic integers or fixed-point values, not floats in sort keys, unless there is a strong reason.
- Equal-score tie-breaking should be deterministic.
- Runtime inference should be independent from training dependencies.
- Large datasets and generated models should not be committed by default.
- Any model file format should include a version number and enough metadata to reject incompatible weights.

## Open Decisions Before PR 5

- Weight storage: embedded constants vs. loaded external model file.
- Runtime dependency policy: no dependency, serialization-only dependency, or numeric crate dependency.
- Model architecture: true NNUE accumulator, simpler MLP over sparse features, KP/KPP table, or a hybrid.
- Training labels: best move, search-work reduction, proof/disproof target, mate length, or separate attacker/defender policies.
- Benchmark threshold for default enablement.

## Current Artifacts

- Research: `codex-notes/issue-16-nnue/research.md`
- Roadmap: `codex-notes/issue-16-nnue/roadmap.md`

No PR-specific `plan.md` or `feature_list.json` exists yet. Create those only after choosing the first PR-sized milestone.

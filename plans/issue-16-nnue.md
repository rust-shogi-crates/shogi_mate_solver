# Issue 16 NNUE Move Ordering Plan

## Plan status

Canonical issue-level checklist. Its wording does not activate a workflow mode.

This file is the issue-level control plan for the remaining NNUE move-ordering work. It is intended to be merged into `main` and used to spawn smaller PRs. Do not implement multiple remaining PRs from this file in one PR.

Each implementation PR should:

- Start from current `origin/HEAD` after `git fetch --prune`.
- Use its own `codex/issue-16-...` branch and dedicated worktree.
- Treat this file under `plans/` as the canonical implementation plan and update
  its checklist in the implementation PR.
- Mark implementation, functional-test, and self-review items `[x]` once they
  are completed and validated in the PR; do not wait for merge.
- Keep default solver behavior unchanged unless the PR explicitly introduces an opt-in mode.
- Preserve exact mate/no-mate correctness; learned scoring may only change candidate move order.
- Do not create a duplicate PR-local plan or research note when the canonical
  plan is sufficient. Durable guidance belongs under `plans/`.

If the suggested branch or worktree already exists, create a fresh uniquely
named branch and dedicated worktree unless the user explicitly asks to
continue the existing one. Treat sibling worktrees and their untracked files
as unrelated user work; do not inspect, rely on, modify, or delete them unless
explicitly requested.

## Current State

- Done: move-ordering abstraction, merged as PR #18.
- Done: root search stats exposure, merged as PR #19.
- Done: ordering quality benchmark metrics, merged as PR #20.
- Done: deterministic static feature extraction, merged as PR #21.
- Done: NNUE-style inference runtime, merged as PR #23.

The remaining work starts from the merged NNUE runtime. PR 6 is the current implementation PR and adds the first training-example and learning path without committing generated datasets or trained weights.

## Remaining PRs

### [x] PR 4: Baseline Learned-Score Interface

- Suggested branch: `codex/issue-16-learned-score-interface`.
- Purpose: introduce the model-neutral runtime seam between extracted features and move ordering.
- Implementation:
  - [x] Define a small integer scorer interface over candidate features or child positions.
  - [x] Add a trivial fixture scorer for tests, such as all-zero scores or a hand-written deterministic table.
  - [x] Integrate score-based tie-breaking behind an explicit opt-in mode.
  - [x] Keep current default ordering unchanged.
- Functional test:
  - Run `mate_solver` in default mode and fixture-score mode on SFENs where candidate ordering can differ.
  - Inspect verbose output or benchmark metrics to confirm fixture scoring affects only explicitly enabled runs.
- Self review:
  - [x] Add automated tests proving default ordering remains unchanged.
  - [x] Add tests proving fixture scores change order only under the opt-in mode.
  - Confirm the interface does not commit the project to a specific NNUE architecture or file format.

### [x] PR 5: NNUE-Style Inference Runtime

- Suggested branch: `codex/issue-16-pr5-nnue-inference`.
- Purpose: add deterministic runtime inference suitable for move ordering.
- Implementation:
  - [x] Decide whether fixture weights are embedded Rust constants or loaded from a file.
  - [x] Get approval before adding runtime dependencies for serialization, numeric arrays, or model loading.
  - [x] Add minimal fixed-point or integer NNUE-style inference.
  - [x] Add a tiny non-quality fixture model for correctness tests.
- Functional test:
  - Run `mate_solver` with the fixture NNUE mode on selected SFENs.
  - Inspect selected move/order diagnostics.
  - Rerun the same command and confirm identical output.
  - If file loading exists, try invalid and missing model inputs.
- Self review:
  - [x] Add inference arithmetic tests and deterministic ordering tests.
  - Run a release-mode benchmark smoke test.
  - Measure release binary size before and after if a model or dependency is embedded.
  - Confirm model/mode selection is explicit and default behavior remains unchanged.

### [x] PR 6: Training and Learning Pipeline

- Suggested branch: `codex/issue-16-pr6-nnue-training`.
- Purpose: create the tooling path from solved/search data to runtime weights.
- Implementation:
  - [x] Add tooling to generate training examples.
  - [x] Add deterministic mirror/replay augmentation with source, transform, and ply metadata.
  - [x] Define labels per candidate by whether its child position is proven to lead to mate under the selected evaluator; do not use the root-selected move as a proxy.
  - [x] Add a configurable generation timeout that writes partial output instead of running indefinitely.
  - [x] Add learning tooling for the versioned runtime weight format.
  - [x] Require `learn` to take an initial model and add `init` for creating a placeholder model.
  - [x] Keep NNUE and training artifacts in the worktree under an ignored directory.
  - [x] Share versioned model parsing between runtime inference and `nnue_training`.
  - [x] Move search limits into `SearchConfig`, support deterministic position limits, and make native deadlines safe on WASM.
  - [x] Keep generated large datasets and trained weights out of the repository unless explicitly approved.
- Functional test:
  - Run the training command on a tiny local fixture.
  - Inspect generated examples and learned weights.
  - Load the learned fixture through the runtime path and confirm it produces usable output.
- Self review:
  - [x] Add smoke or round-trip tests.
  - Confirm training dependencies are separate from solver runtime dependencies.
  - Confirm generated artifacts are either intentionally committed small fixtures or ignored/external.

### [ ] PR 7: Trained Model Rollout

- Suggested branch: `codex/issue-16-trained-model-rollout`.
- Purpose: connect a real trained model to the solver in an evidence-driven way.
- Implementation:
  - [ ] Add or reference a real trained model.
  - [ ] Expose it through an explicit option first.
  - [ ] Document model provenance, training data, feature format version, and benchmark results.
  - [ ] Consider default enablement only after benchmark evidence supports it.
- Functional test:
  - Run the solver manually with and without the trained-model option on representative SFENs.
  - Inspect answers, runtime summaries, and search summaries.
  - Confirm failures are clear when the model artifact is unavailable or incompatible.
- Self review:
  - Run full automated correctness tests.
  - Compare release benchmarks against the previous default.
  - Measure release binary size if the model is embedded.
  - Keep default enablement evidence-driven; do not enable by default if search work or runtime does not improve.

## Cross-Cutting Rules

- Use deterministic integer or fixed-point scores in sort keys unless there is a strong reason to use floats.
- Include deterministic tie-breakers for equal scores.
- Keep attacker OR-node and defender AND-node scoring objectives distinct.
- Keep runtime inference independent from training dependencies.
- Version any model format and reject incompatible weights clearly.
- Prefer release-mode benchmarks for performance comparisons.
- Do not commit large datasets or generated trained models without explicit approval.

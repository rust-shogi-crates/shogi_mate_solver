# Issue 16 NNUE Move Ordering Plan

## Mode

Workflow mode: plan mode.

This file is the issue-level control plan for the remaining NNUE move-ordering work. It is intended to be merged into `main` and used to spawn smaller PRs. Do not implement multiple remaining PRs from this file in one PR.

Each implementation PR should:

- Start from current `origin/HEAD` after `git fetch --prune`.
- Use its own `codex/issue-16-...` branch and dedicated worktree.
- Create a PR-local `codex-notes/<task-slug>/plan.md` before code edits.
- Keep default solver behavior unchanged unless the PR explicitly introduces an opt-in mode.
- Preserve exact mate/no-mate correctness; learned scoring may only change candidate move order.
- Remove PR-local `codex-notes` right before merge. Durable guidance belongs under `plans/`.

## Current State

- Done: move-ordering abstraction, merged as PR #18.
- Done: root search stats exposure, merged as PR #19.
- Done: ordering quality benchmark metrics, merged as PR #20.
- Done: deterministic static feature extraction, merged as PR #21.

The remaining work starts from the merged feature extractor. PR 4 is the recommended next PR, but it has not started yet. It should not add neural-network inference; it should add the smallest scorer interface that can consume sparse `FeatureId` lists and affect ordering only in an explicit mode.

## Remaining PRs

### [ ] PR 4: Baseline Learned-Score Interface

- Suggested branch: `codex/issue-16-learned-score-interface`.
- Purpose: introduce the model-neutral runtime seam between extracted features and move ordering.
- Implementation:
  - [ ] Define a small integer scorer interface over candidate features or child positions.
  - [ ] Add a trivial fixture scorer for tests, such as all-zero scores or a hand-written deterministic table.
  - [ ] Integrate score-based tie-breaking behind an explicit opt-in mode.
  - [ ] Keep current default ordering unchanged.
- Functional test:
  - Run `mate_solver` in default mode and fixture-score mode on SFENs where candidate ordering can differ.
  - Inspect verbose output or benchmark metrics to confirm fixture scoring affects only explicitly enabled runs.
- Self review:
  - Add automated tests proving default ordering remains unchanged.
  - Add tests proving fixture scores change order only under the opt-in mode.
  - Confirm the interface does not commit the project to a specific NNUE architecture or file format.

### [ ] PR 5: NNUE-Style Inference Runtime

- Suggested branch: `codex/issue-16-nnue-inference`.
- Purpose: add deterministic runtime inference suitable for move ordering.
- Implementation:
  - [ ] Decide whether fixture weights are embedded Rust constants or loaded from a file.
  - [ ] Get approval before adding runtime dependencies for serialization, numeric arrays, or model loading.
  - [ ] Add minimal fixed-point or integer NNUE-style inference.
  - [ ] Add a tiny non-quality fixture model for correctness tests.
- Functional test:
  - Run `mate_solver` with the fixture NNUE mode on selected SFENs.
  - Inspect selected move/order diagnostics.
  - Rerun the same command and confirm identical output.
  - If file loading exists, try invalid and missing model inputs.
- Self review:
  - Add inference arithmetic tests and deterministic ordering tests.
  - Run a release-mode benchmark smoke test.
  - Measure release binary size before and after if a model or dependency is embedded.
  - Confirm model/mode selection is explicit and default behavior remains unchanged.

### [ ] PR 6: Training and Export Pipeline

- Suggested branch: `codex/issue-16-nnue-training`.
- Purpose: create the tooling path from solved/search data to runtime weights.
- Implementation:
  - [ ] Add tooling to generate training examples.
  - [ ] Define labels clearly: best move, child proof/disproof improvement, mate length, search-work reduction, or separate attacker/defender targets.
  - [ ] Add export tooling for the runtime weight format.
  - [ ] Keep generated large datasets and trained weights out of the repository unless explicitly approved.
- Functional test:
  - Run the training/export command on a tiny local fixture.
  - Inspect generated examples and exported weights.
  - Load the exported fixture through the runtime path and confirm it produces usable output.
- Self review:
  - Add smoke or round-trip tests.
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

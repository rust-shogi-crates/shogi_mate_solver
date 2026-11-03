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
  - [x] Define one mate/nomate label per generated position under the selected evaluator; do not include a candidate move in the training example.
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
  - [ ] Add or reference a real trained model using the multi-layer format.
  - [x] Extend runtime inference from the two-unit fixture to `512 -> 32 -> 32 -> 1`.
  - [x] Extend model parsing and learning/export for the multi-layer format.
  - [x] Expose the model's raw logit and a fixed-point sigmoid probability separately.
  - [x] Emit the probability with per-example scores and document its calibration limits.
  - [x] Define and apply an explicit logit scale so small raw scores do not saturate probability output.
  - [x] Evaluate post-move child positions with NNUE position features only.
  - [x] Add a `--max-positions` limit to normal solving and benchmark runs.
  - [x] Add incremental NNUE delta evaluation after child-position evaluation is correct.
  - [x] Expose it through an explicit option first.
  - [x] Expose model text/bytes loading APIs for library and WASM consumers.
  - [x] Add deterministic YaneuraOu SFEN sampling and solver-labeled derived-position generation.
  - [ ] Consider default enablement only after benchmark evidence supports it.
- Functional test:
  - [x] Run the solver manually with and without the trained-model option on representative SFENs.
  - [x] Inspect answers, runtime summaries, and search summaries.
  - [x] Confirm failures are clear when the model artifact is unavailable or incompatible.
- Self review:
  - [x] Run full automated correctness tests.
  - [x] Compare release benchmarks against the previous default.
  - Measure release binary size if the model is embedded.
  - Keep default enablement evidence-driven; do not enable by default if search work or runtime does not improve.

## Follow-up TODOs from training and inference review

### [x] 1. Represent the defending king's absolute position

- Problem: board features contain only offsets from the defending king. Translating every piece produces identical features even when board edges change the mate result.
- Reproduction (White defender to move): `8k/7G1/7K1/9/9/9/9/9/9 w - 1` is mate, while `9/8k/7G1/7K1/9/9/9/9/9 w - 1` is non-mate. Both received score 117 with the tested trained model.
- [x] Implementation: add defending-king square features (IDs 100–180), shared by training and full/incremental inference; require `feature_version 2` and reject legacy models. Existing SFEN datasets remain reusable.
- [x] Functional test: initialize, train, export, and score the two positions above through the release CLI; confirm opposite score signs and rejection of legacy models.
- [x] Self review: regression tests cover full/incremental feature and score equivalence after king moves of both colors, captures, drops, and promotions, plus missing/unsupported/duplicate feature versions. All workspace release tests pass.

### [ ] 3. Evaluate NNUE ordering across existing move categories

- Problem: DF-PN sorts by the existing normal-move/drop-piece category before the NNUE score. A highly rated drop cannot precede a normal move. This is the current tie-breaking design, not a reversed-score bug.
- [ ] Implementation: evaluate an explicit opt-in ordering that lets NNUE compare moves across categories, preserving the default ordering and attacker/defender score direction.
- [ ] Functional test: compare correctness, search nodes, and elapsed time on fixed training-derived and unrelated positions in release mode.
- [ ] Self review: verify deterministic ties and unchanged mate/non-mate answers; use benchmark evidence to decide whether to adopt the ordering.

### [ ] 4. Give distinct generated positions distinct example IDs

- Problem: `prepare_yaneuraou` builds IDs from source, transform, and ply only. Different branches at the same ply can receive the same ID. The reviewed 10,000 training and 1,000 test examples contained 23 repeated-ID occurrences with different SFENs.
- [ ] Implementation: include deterministic branch or position identity in example IDs while preserving `source_id` for dataset splitting.
- [ ] Functional test: generate multiple branches at the same ply and confirm distinct positions have distinct IDs and repeated runs produce stable IDs.
- [ ] Self review: check sampling and evaluation-result matching with the new IDs; regenerate affected datasets before comparing new results.

## King-neighbor feature experiment (2026-11-02)

- Tested eight neighboring squares with four states each: off-board, empty, defender piece, attacker piece. Full and incremental evaluation passed regression tests.
- Compared against feature version 2 using the same audited 10,000 training / 1,000 test examples, initial weights, and 10 epochs. Evaluated each exported model on 32 fixed search positions.
- Final integer-model train/test loss: baseline 0.052268 / 1.334930; neighbors 0.060804 / 1.340870. All 32 search positions remained correct.
- On five unrelated positions, final-model DF-PN nodes increased from 2286 to 2380. Comparing each variant's minimum-test-loss checkpoint reduced nodes (2394 to 2237), but neither comparison improved elapsed time. This small experiment does not establish a speed benefit.
- Adoption deferred; normal code remains feature version 2. Reproduction commands, metrics, models, and the experimental patch are retained locally under `nnue_training/work/king-neighbors-20261102/` (ignored artifacts).

## Defender HalfKP experiment (2026-11-02)

- Replaced relative board features with defending-king square × non-king piece information; paired existing hand-count buckets with the king square. This single-perspective variant does not reproduce YaneuraOu's full encoding or architecture.
- Used the same audited 10,000 training / 1,000 test examples, initialization procedure, and 10 epochs as the king-neighbor experiment. All 58 tests passed, including incremental updates of hand features when the king moves.
- Final integer-model train/test loss: baseline 0.052268 / 1.334930; HalfKP 0.138833 / 2.185923. All epochs solved the fixed 32 search positions correctly.
- On five unrelated positions, final-model DF-PN nodes changed from 2286 to 2253, but eval nodes increased from 8030 to 16884. HalfKP's best-test-loss checkpoint was epoch 1 (50% classification accuracy); its DF-PN/eval nodes were 1769/7762 versus the initial model's 1755/8019. No consistent benefit established.
- Learned feature count increased from 6401 to 61238, and model text size from 7583684 to 65951437 bytes. Adoption deferred; normal code remains version 2. Patch, commands, models, and metrics are retained locally under `nnue_training/work/halfkp-20261102/` (ignored artifacts).

## Color and mirror normalization experiment (2026-11-02)

- [x] Implementation: feature version 5 rotates/swaps colors to a White defender and reflects the board to king files 1–5. Center-file or missing-king cases use board order to resolve mirror ties. Normalize hand colors and side-to-move, retain attacker/defender role, and sort active features.
- [x] Functional test: same 10,000 training / 1,000 test examples and 10 epochs; verified zero symmetry-equivalent positions across splits. Final integer-model test loss improved 1.334930→0.967210 and classification accuracy 74.9%→79.1%; model text shrank 7583684→5296212 bytes.
- [x] Self review: 59 workspace release tests passed, including rotation/color-swap and mirror invariance, missing kings, center-file tie switches, and full/incremental equivalence. Older feature versions are rejected.
- Both variants reached minimum test loss at epoch 4 (0.776197→0.625767). On five unrelated positions, epoch-4 eval nodes fell 11335→9197 and mean time 195.646→149.350 ms across five alternating runs. Final-epoch eval was slower, so no uniform speed improvement is claimed.
- Retain the implementation in the working tree. Existing models require reinitialization and retraining; existing SFEN teachers remain usable after checking symmetry-aware split overlap. Detailed commands, models, logs, metrics, and patch are local under `nnue_training/work/symmetry-20261102/` (ignored artifacts).

## Cross-Cutting Rules

- Use deterministic integer or fixed-point scores in sort keys unless there is a strong reason to use floats.
- Include deterministic tie-breakers for equal scores.
- Keep attacker OR-node and defender AND-node scoring objectives distinct.
- Keep runtime inference independent from training dependencies.
- Version any model format and reject incompatible weights clearly.
- Prefer release-mode benchmarks for performance comparisons.
- Do not commit large datasets or generated trained models without explicit approval.

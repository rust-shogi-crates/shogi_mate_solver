# Issue 16 PR Checklist

- [ ] **PR 1: Move-Ordering Abstraction**
  - [ ] Impl: create the PR branch/worktree and PR-specific `plan.md`/`feature_list.json`, add the internal move-ordering abstraction, preserve current ordering by default, and thread only minimal config needed for later mode selection.
  - [ ] Func test: run `mate_solver` manually with known mate and no-mate SFEN inputs, compare the visible answers and verbose search summary with the pre-change behavior, and confirm the default CLI experience is unchanged.
  - [ ] Self review: run automated checks and benchmarks, verify the diff adds no NNUE, model files, or training code, confirms df-pn/eval search semantics are unchanged, and keeps the PR to one logical change.

- [ ] **PR 2: Ordering Metrics and Benchmark Fixtures**
  - [ ] Impl: create the PR branch/worktree and PR-specific `plan.md`/`feature_list.json`, extend benchmark output for move-ordering quality, and add or document a representative issue #16 benchmark set.
  - [ ] Func test: run `benchmark_harness run` on the tiny CI fixture and the new representative fixture, inspect the JSONL output by hand, then run `benchmark_harness compare` and open/read the report to confirm the new metrics are understandable.
  - [ ] Self review: run automated checks, verify committed benchmark data is CI-sized, larger data is external or generated, and any workflow edits obey the GitHub Actions pinning policy.

- [ ] **PR 3: Static Feature Extraction**
  - [ ] Impl: create the PR branch/worktree and PR-specific `plan.md`/`feature_list.json`, add deterministic feature extraction for positions and candidate moves, and keep the feature format model-file agnostic.
  - [ ] Func test: expose or use a small debug/fixture path to print features for known SFEN positions, inspect the output manually, and confirm equivalent reruns produce identical feature IDs and ordering.
  - [ ] Self review: add and run automated stability tests, verify side-to-move, king-relative piece locations, hands, move kind, from/to squares, promotion/drop flags, and attacker/defender role are covered without adding neural-network or training dependencies.

- [ ] **PR 4: Baseline Learned-Score Interface**
  - [ ] Impl: create the PR branch/worktree and PR-specific `plan.md`/`feature_list.json`, add a model-neutral integer scorer interface, provide a trivial fixture scorer, and gate score tie-breaking behind an explicit mode.
  - [ ] Func test: run `mate_solver` in default mode and fixture-score mode on SFENs where ordering can differ, inspect verbose output or benchmark metrics, and confirm the fixture scorer affects only explicitly enabled runs.
  - [ ] Self review: add and run automated ordering tests, verify current default behavior remains the default, tie-breaking is deterministic, and the interface does not commit the project to NNUE internals yet.

- [ ] **PR 5: NNUE Inference Runtime**
  - [ ] Impl: create the PR branch/worktree and PR-specific `plan.md`/`feature_list.json`, decide embedded versus loaded weights, get approval before adding any runtime dependency, and add minimal deterministic NNUE-style inference with tiny fixture weights.
  - [ ] Func test: run `mate_solver` with the fixture NNUE mode on selected SFENs, inspect the selected move/order diagnostics, rerun the same command to confirm identical output, and try an invalid/missing model input if file loading exists.
  - [ ] Self review: add and run automated inference/order tests, run a benchmark smoke test, measure release binary size before/after, verify runtime dependencies are justified and minimal, and confirm model/mode selection is explicit.

- [ ] **PR 6: Training and Export Pipeline**
  - [ ] Impl: create the PR branch/worktree and PR-specific `plan.md`/`feature_list.json`, add tooling to generate training examples and export runtime weights, and define labels clearly.
  - [ ] Func test: run the training/export command on a tiny local fixture, inspect the generated examples and exported weights, then load the exported fixture through the runtime path and confirm it produces usable output.
  - [ ] Self review: add and run automated smoke/round-trip tests, verify generated large datasets and trained weights are not committed without approval, and keep training dependencies separate from solver runtime dependencies.

- [ ] **PR 7: Trained Model Rollout**
  - [ ] Impl: create the PR branch/worktree and PR-specific `plan.md`/`feature_list.json`, add or reference a real trained model, expose it through an explicit option first, and document provenance, training data, and benchmark results.
  - [ ] Func test: run the solver manually with and without the trained-model option on representative SFENs, inspect answers and runtime/search summaries, and confirm failures are clear when the model artifact is unavailable or incompatible.
  - [ ] Self review: run full automated correctness tests, compare release benchmarks against the previous default, measure release binary size if embedded, verify default enablement is evidence-driven, and confirm the model artifact strategy is documented.

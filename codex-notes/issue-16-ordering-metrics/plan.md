# Plan: PR 2 Ordering Metrics and Benchmark Fixtures

## Overview

Extend the benchmark harness so it is useful for evaluating move-ordering changes, while keeping solver behavior unchanged. The core addition is benchmark output that records whether the current ordered first root candidate is the exact chosen/proving move, plus the chosen/proving move's rank in the ordered root list. The fixture strategy should stay CI-safe: keep `benchmark/issue13-ci.jsonl` small and document how to run a larger representative set without committing large generated data.

The main CI hazard is fixture drift between base and current branches. PR 2 should make comparison tolerate new current-only benchmark IDs by comparing common records while still failing on correctness regressions for records that exist in current output.

Newly emitted run metrics:

  - `root_candidate_moves`: context metric: number of legal root candidates considered by the evaluator before search.
  - For `df_pn`, this is root checking moves for the attacker/root OR node.
  - For `eval`, this is root checking moves for the attacker entry point.
  - These are not "the moves that appear first" after ordering. They are the full generated legal candidate set at the root before the search loop starts. In current mode, ordering can reorder this set but does not change its size.
- `root_chosen_move`: ordering-quality metric: the exact root move chosen/proven by the exact solver path, serialized in USI where available.
  - For `eval`, this is the `best` move returned by root `alpha_beta_me`.
  - For `df_pn`, this is the first ordered root child that is proven mate after the df-pn root search, when the root is mate.
  - For no-mate records, this is absent/null because there is no proving mate move.
- `root_chosen_move_rank`: ordering-quality metric: zero-based rank of `root_chosen_move` in the ordered root candidate list.
  - `0` means the current ordering put the exact useful move first.
  - Larger values mean the ordering placed other candidates before the exact useful move.
- `root_first_candidate_chosen`: ordering-quality metric: boolean shortcut for `root_chosen_move_rank == 0`.
- `eval_positions_inspected`: eval search work only, emitted for `eval` records.
- `df_pn_positions_inspected`: nested df-pn work during eval search, emitted for `eval` records.
- Existing `positions_inspected` remains the compatibility total.

How these help evaluate ordering:

- `root_chosen_move_rank`, `root_chosen_move_rank / root_candidate_moves`, and `root_first_candidate_chosen` directly evaluate root ordering quality. A better ordering should increase first-candidate hits and lower chosen-move rank/fraction.
- `root_candidate_moves` is only context. A first-candidate hit with one candidate is not meaningful; a first-candidate hit with many candidates is meaningful.
- Comparing `positions_inspected` alongside chosen-move rank gives a stable search-work signal for whether a new ordering reduced exploration.
- Splitting `eval_positions_inspected` and `df_pn_positions_inspected` shows whether a change affected eval search itself or the df-pn probes eval uses to reject no-mate branches.
- Branch labels from `find_branches`/`mate_solver::search` are useful for the later NNUE objective `Position -> probability of mate`, because each recorded branch position implies mate/no-mate continuation information. PR 2 should document this and, if cheap, expose branch-label counts or examples without turning the benchmark harness into a training-data exporter.
- The metrics are intentionally root-level PR 2 metrics. Deeper per-node ordering quality and full training-example export should wait until later PRs add feature/scorer/training instrumentation.

## Files to Change

- `benchmark_harness/src/main.rs`
- `benchmark/issue16-ordering.jsonl`
- `README.md`
- `.github/workflows/benchmark.yml`
- `codex-notes/issue-16-ordering-metrics/feature_list.json`

## Worktree and Branch

- Branch: `codex/issue-16-ordering-metrics`
- Worktree: dedicated issue 16 ordering metrics worktree
- Base: `origin/main`

## Detailed Implementation Steps

1. Extend benchmark result records with ordering-relevant fields.
   - For df-pn, emit `root_candidate_moves` from the initial position:
     - checking move count for attacker/root OR node after generation and before search selection.
     - `root_chosen_move`, `root_chosen_move_rank`, and `root_first_candidate_chosen` when the root is mate and a proving child can be identified from the df-pn table.
   - For eval, emit:
     - `root_candidate_moves` from the generated initial attacker checks before eval ordering/search.
     - `root_chosen_move`, `root_chosen_move_rank`, and `root_first_candidate_chosen` from root `alpha_beta_me`.
     - `eval_positions_inspected`.
     - `df_pn_positions_inspected`.
     - existing combined `positions_inspected` remains unchanged.
   - Preserve existing JSON fields so older readers still work.

2. Extend compare parsing and summaries carefully.
   - Keep required fields unchanged.
   - Parse optional new numeric fields when present.
  - Add comparison summaries for new metrics only when both base/current records contain them:
     - chosen root move rank.
     - chosen root move rank divided by root candidate moves.
     - first-candidate chosen hit rate.
     - eval-only inspected positions.
     - nested df-pn inspected positions.
   - Keep root candidate moves as denominator/context for derived metrics, not as a standalone base/current quality comparison.
   - Avoid making older base outputs invalid.

3. Make compare robust to current-only fixture additions.
   - Compare records that exist in both base and current.
   - Still fail if any current result has `correct: false`.
   - Emit separate structured JSONL warning records with `type: "warning"` for base-only/current-only records.
   - Include unmatched-record counts in comparison metadata or aggregate comparison output when practical.
   - Ensure CI can pass when a PR adds new benchmark fixture rows that are correct in current output.

4. Add a small issue 16 representative fixture.
   - Add `benchmark/issue16-ordering.jsonl` with a few committed positions that cover:
     - short mate.
     - longer mate from existing tests.
     - no-mate.
     - at least one position with multiple root candidates.
     - at least one mate position where the chosen/proving root move is not trivially the only candidate.
   - Keep it small enough for release-mode CI smoke use.
   - Prefer SFENs already present in tests/repo history over introducing large external data.

5. Document benchmark usage.
   - Update `README.md` to describe:
     - new result fields.
     - first-candidate hit/rank interpretation.
     - why `root_candidate_moves` is context, not itself an ordering-quality metric.
     - how `find_branches` output can inform future `Position -> probability of mate` data generation.
     - issue 16 fixture.
     - compare behavior for added/removed records.
     - how to run both CI fixture and issue 16 fixture manually.

6. Update CI workflow.
   - Run both `benchmark/issue13-ci.jsonl` and `benchmark/issue16-ordering.jsonl` in benchmark CI.
   - Preserve action pinning policy while editing `.github/workflows/benchmark.yml`.

7. Validate.
   - Run strict release benchmark on `benchmark/issue13-ci.jsonl`.
   - Run strict release benchmark on `benchmark/issue16-ordering.jsonl`.
   - Run compare with base/current outputs.
   - Run a simulated current-only added-record comparison.
   - Run formatting, tests, clippy.

## Alternatives Considered

- Add deep solver instrumentation for per-node ordering quality.
  - Rejected for PR 2 because it risks changing core search APIs and belongs after feature/scorer work is clearer.
- Commit a large representative benchmark corpus.
  - Rejected unless explicitly approved; larger data can make CI slow and repository history noisy.
- Change CI to run base solver against current branch fixtures.
  - Possible, but riskier because it mixes base code with current benchmark files. Compare-level tolerance is less invasive.
- Only document a larger fixture without adding metrics.
  - Rejected because PR 2’s purpose includes extending benchmark output enough to evaluate ordering quality.

## Risks

- Compare semantics may hide real missing-result problems if current-only/base-only handling is too permissive.
- `root_chosen_move_rank` is root-only. It can miss deeper ordering improvements or regressions.
- df-pn proving-child identification from the final table may be ambiguous if multiple children are proven; the metric should use the first proven child in the current ordered list and document that tie behavior.
- Branch labels from `find_branches` may be useful but should not turn PR 2 into a training-data pipeline.
- Additional fixture positions can make CI slower.
- Existing benchmark artifacts or scripts may assume only current fields, so new fields should be additive.
- Workflow edits require strict GitHub Actions pinning discipline.
- Running both benchmark fixtures in CI increases runtime; keep `issue16-ordering.jsonl` small enough for release-mode smoke coverage.

## Test Strategy

- Unit tests for compare behavior with current-only and base-only records if practical.
- Strict release benchmark runs:
  - `cargo run --release --locked -p benchmark_harness -- run --strict --revision=current benchmark/issue13-ci.jsonl`
  - `cargo run --release --locked -p benchmark_harness -- run --strict --revision=current benchmark/issue16-ordering.jsonl`
- Compare smoke tests with generated base/current JSONL.
- Full checks:
  - `cargo fmt --check`
  - `cargo test --locked`
  - `cargo clippy --all-targets --locked -- -D warnings`

## Assumptions

- PR 2 should not change solver search behavior.
- Additive JSON result fields are acceptable.
- Keeping `positions_inspected` as the combined existing value preserves compatibility.
  - Note: correct.
- The issue 16 fixture can reuse positions already present in tests.
- CI fixture expansion should not require large data.

## Open Questions

- Should CI run only `issue13-ci.jsonl`, or both `issue13-ci.jsonl` and the new `issue16-ordering.jsonl`?
  - Decision for PR 2: run both.
- Should root candidate count include generated candidates before or after ordering?
  - Decision for PR 2: count generated candidates at the root before search selection/iteration. This count is independent of which move the ordering places first.
- How should df-pn handle multiple proven root children?
  - Decision for PR 2: report the first proven child in current ordered root order, because that is the candidate an ordering policy most wants to put early.

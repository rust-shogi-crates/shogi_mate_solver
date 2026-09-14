# Research: PR 3 Static Feature Extraction

## Relevant Files and Modules

- `mate_solver/src/position_wrapper.rs`
  - Wraps `shogi_core::PartialPosition`, owns the cached zobrist hash, and exposes generated checking/evasion moves.
  - Provides `inner()` access to the underlying `PartialPosition`, which is enough for board, hand, side-to-move, and king-square feature extraction.
- `mate_solver/src/move_ordering.rs`
  - Contains the PR1 move-ordering abstraction.
  - Current mode has no learned features; PR3 should add feature extraction without changing ordering behavior.
- `mate_solver/src/df_pn/search.rs`
  - Distinguishes attacker `NodeKind::Or` and defender `NodeKind::And`.
  - Generates candidate checks/evasions, then applies current ordering.
  - The node role is available at the generation call site, but PR3 does not need to integrate feature scoring into search yet.
- `mate_solver/src/eval/search.rs`
  - Separates attacker and defender search through `alpha_beta_me*` and `alpha_beta_you*`.
  - Uses the same move-ordering hooks as df-pn.
  - Future scoring will likely call feature extraction from these ordering hooks.
- `mate_solver/src/lib.rs`
  - Re-exports solver modules.
  - A new `features` module should be exported here if later PRs need it from the benchmark harness or scorer interface.
- `benchmark_harness/src/main.rs`
  - PR2 added root ordering metrics.
  - PR3 does not need to change benchmark behavior unless a debug/fixture feature dump path is chosen there.

## Dependency APIs

- `shogi_core::Square`
  - `Square::all()` iterates every board square in ascending internal index order.
  - `array_index()`, `file()`, and `rank()` provide stable numeric square data.
  - `relative_file(color)` and `relative_rank(color)` provide perspective-normalized coordinates.
- `shogi_core::PieceKind`
  - `PieceKind::all()` returns all 14 piece kinds in discriminant order.
  - `array_index()` provides stable compact indices.
  - `promote()` and `unpromote()` distinguish promoted/unpromoted kinds.
- `shogi_core::Hand`
  - `Hand::all_hand_pieces()` returns the seven hand-legal unpromoted piece kinds.
  - `count(piece_kind)` returns the count for a hand piece.
- `shogi_core::Move`
  - Variants expose normal moves, drops, source squares, destination squares, promotion flags, and dropped piece.
  - `from()`, `to()`, `is_promoting()`, and `is_drop()` are available helpers.
- `shogi_core::PartialPosition`
  - `side_to_move()`, `piece_at(square)`, `hand_of_a_player(color)`, and `king_position(color)` are sufficient for the roadmap feature set.

## Feature Surface Implied by Roadmap

PR3 should provide deterministic extraction for:

- Side to move.
- Board pieces, including owner, piece kind, and king-relative location.
- Hands for both sides, restricted to hand-legal unpromoted piece kinds.
- Candidate move facts: normal/drop, from square, to square, promotion flag, dropped piece kind.
- Search role: attacker/OR versus defender/AND.

The roadmap says feature format should stay independent of model file format, and PR3 should not choose a neural-network crate or training framework.

## Candidate Shape

A small `mate_solver/src/features.rs` module fits the current codebase:

- `NodeRole` enum for attacker/defender feature context.
- `FeatureId` as a deterministic integer wrapper or alias.
- `FeatureSet` or `Vec<FeatureId>` output for sparse features.
- Functions such as:
  - `position_features(position: &PositionWrapper) -> Vec<FeatureId>`
  - `move_features(position: &PositionWrapper, mv: Move, role: NodeRole) -> Vec<FeatureId>`
  - `candidate_features(position: &PositionWrapper, mv: Move, role: NodeRole) -> Vec<FeatureId>`

Keeping this function-based mirrors existing modules and avoids a premature trait or model interface.

## Determinism Considerations

- Iterate board squares with `Square::all()` so feature ordering is deterministic.
- Iterate colors with `Color::all()` and hand pieces with `Hand::all_hand_pieces()`.
- Sort or emit feature IDs in a documented deterministic order.
- Use integer feature IDs only; do not use floats.
- Avoid hashing feature names for IDs, because hash choices could become implicit ABI.
- Reserve separate ID ranges for side, board, hand, move, and role features so later scoring can rely on stable IDs.

## King-Relative Coordinates

`PartialPosition::king_position(color)` is available and should anchor the relative board features.

Open design point: which king to anchor to.

- For mate move ordering, the defender king is the most useful anchor because checking moves target that king.
- The defender is `position.inner().side_to_move().flip()` for attacker/OR nodes and `position.inner().side_to_move()` for defender/AND nodes after the attacker has given check.
- A role-aware API can make this explicit instead of hiding it in a position-only extractor.

## Test Strategy

Focused unit tests in `features.rs` should cover:

- Stable feature IDs for a fixed known SFEN.
- Repeated extraction from the same SFEN returns identical IDs and ordering.
- Side-to-move changes affect side features.
- Board piece features include owner, kind, and king-relative coordinates.
- Hand features count multiple pieces deterministically.
- Normal moves include from/to/promote facts.
- Drop moves include dropped kind/to/drop facts and no from-square feature.
- Attacker and defender role features differ.

Existing test SFENs from df-pn/eval tests are suitable small fixtures.

## Functional Test Path

Roadmap asks for a small debug/fixture path to print features for known SFEN positions. Options:

- Add a `#[cfg(test)]` helper used by tests only.
- Add a small internal test that renders feature IDs to a string and asserts exact output.
- Add a CLI flag to `mate_solver` to print features.

The least invasive PR3 path is test-only rendering. A CLI flag would expose user-facing behavior and may be better deferred until a later scorer/training PR needs it.

## Constraints

- No NNUE, model files, training code, or new dependencies.
- Default search and CLI behavior should remain unchanged.
- Keep extraction model-file agnostic.
- Keep the PR to static deterministic features and stability tests.
- Do not wire features into ordering decisions yet; that belongs to PR4 or later.

## Potential Pitfalls

- Feature IDs become a durable contract once training data exists, so ranges and tests should be explicit.
- If feature extraction depends on `Debug` or USI strings, later format changes could silently break training data compatibility.
- A pure position extractor cannot know attacker/defender role, which the roadmap explicitly wants covered.
- King-relative features need a clear fallback if a malformed `PartialPosition` lacks a king. Existing partial positions may be arbitrary; extraction should either skip king-relative board features or emit absolute coordinates when the anchor is missing.
- Emitting one hand feature per held piece can duplicate IDs; emitting count-valued features is more compact but less NNUE-like. PR3 should document whichever representation it chooses.

## Unknowns

- Whether duplicate sparse features are acceptable for hand counts, or whether count should be encoded as separate count-bucket IDs.
- Whether board features should be anchored to the defender king only, or include both defender-relative and side-to-move-relative coordinates.
- Whether PR3 should add any public API, or keep `features` crate-public until PR4 defines the scorer interface.

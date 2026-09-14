# Plan: PR 3 Static Feature Extraction

## Overview

Add deterministic, model-agnostic feature extraction for positions and candidate moves. This PR creates the data surface that later scorer/NNUE PRs can consume, but it does not add a scorer, neural-network dependency, model file, training code, CLI mode, or ordering behavior change.

The extractor will produce stable integer feature IDs from `shogi_core` enum and square APIs, with focused tests that lock the ID scheme and output ordering for fixed SFEN fixtures.

## Files to Change

- `mate_solver/src/features.rs`
- `mate_solver/src/lib.rs`
- `codex-notes/issue-16-feature-extraction/feature_list.json`

## Worktree and Branch

- Branch: `codex/issue-16-feature-extraction`
- Worktree: dedicated issue 16 feature extraction worktree
- Base: `origin/main`

## Detailed Implementation Steps

1. Add `mate_solver/src/features.rs`.
   - Define `FeatureId` as a small deterministic integer wrapper, likely:

     ```rust
     #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
     pub struct FeatureId(pub u32);
     ```

   - Define a role enum for search context:

     ```rust
     #[derive(Clone, Copy, Debug, Eq, PartialEq)]
     pub enum FeatureRole {
         Attacker,
         Defender,
     }
     ```

   - Keep the API concrete and dependency-free.

2. Define explicit feature ID ranges.
   - Reserve separate ranges for:
     - role and side-to-move features.
     - board piece features.
     - hand features.
     - candidate move kind and destination/source facts.
     - promotion/drop facts.
   - Use `Color::array_index()`, `PieceKind::array_index()`, `Square::array_index()`, `Square::relative_file()`, and `Square::relative_rank()` rather than strings or hashes.

3. Implement deterministic extraction functions.
   - Add position-level extraction for side to move, board pieces, and hands.
   - Add candidate move extraction for:
     - normal/drop move kind.
     - from square for normal moves.
     - to square for all moves.
     - promotion flag for normal moves.
     - dropped piece kind for drop moves.
     - attacker/defender role.
   - Anchor king-relative board coordinates to the defender king when available.
   - For malformed partial positions without a relevant king, emit absolute square features and keep output deterministic.

4. Re-export the module.
   - Add `pub mod features;` in `mate_solver/src/lib.rs`.
   - Do not thread features into move ordering yet.

5. Add focused unit tests in `features.rs`.
   - Assert exact feature IDs for a known SFEN fixture.
   - Assert repeated extraction from the same SFEN returns identical IDs in identical order.
   - Assert side-to-move changes affect side features.
   - Assert board feature extraction covers piece owner, kind, and coordinates.
   - Assert hand features cover multiple pieces deterministically.
   - Assert normal move features include from, to, and promotion facts.
   - Assert drop move features include dropped piece kind and destination, without a from-square fact.
   - Assert attacker and defender roles produce distinct role features.

6. Run validation.
   - `cargo fmt --check`
   - `cargo test --locked -p mate_solver features`
   - `cargo test --locked`
   - `cargo clippy --all-targets --locked -- -D warnings`
   - Manual functionality test:
     - run a small test/debug rendering path for known SFEN fixtures.
     - inspect feature ID output by hand.
     - rerun the same command/test and confirm output is identical.

## Alternatives Considered

- Add a CLI feature-dump mode now.
  - Rejected for the initial plan because PR3 can satisfy deterministic fixture inspection with tests only, while a CLI flag would create user-facing behavior before the scorer/training workflow needs it.
- Use string feature names and map them later.
  - Rejected because the roadmap needs stable feature IDs; strings add formatting surface and delayed ABI decisions.
- Use hashed feature IDs.
  - Rejected because the hash function and seed would become hidden format contracts.
- Add a scorer trait now.
  - Rejected because PR4 owns the learned-score interface.

## Risks

- Feature IDs may become durable training-data ABI, so ranges need to be explicit and tests should lock representative IDs.
- A position-only extractor cannot know attacker/defender semantics; the move feature API must accept the role explicitly.
- `PartialPosition` can be malformed, including missing kings. The extractor must avoid panics on missing anchors.
- Hand counts can be encoded as duplicate sparse features or count-bucket features. The implementation should choose one and keep it documented.
- Broad tests that only check "some features exist" would not protect stability; tests need exact IDs for representative cases.

## Test Strategy

- Unit tests for feature ID arithmetic and ordering.
- Unit tests for representative SFEN fixtures covering board, hands, normal moves, drop moves, side to move, and role.
- Full workspace tests to confirm no search or CLI behavior regressed.
- Clippy and format checks through the normal Rust validation path.
- Manual inspection of deterministic feature output through a small test/debug fixture path.

## Assumptions

- PR3 should expose the module publicly enough for PR4 and the benchmark harness to consume later.
- Feature extraction should not alter current move ordering or benchmark output.
- Dependency-free sparse integer IDs are the best fit until PR4 defines the scorer interface.
- Defender-king-relative coordinates are the useful default for mate move ordering, with deterministic fallback when no defender king exists.

## Open Questions

- Should hand counts be represented as repeated identical sparse IDs or as count-bucket IDs?
- Should board features include both absolute and defender-king-relative coordinates, or only the role-aware coordinate form plus fallback?

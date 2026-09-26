//! Deterministic sparse feature IDs for move-ordering scorers.
//!
//! The ID ranges are part of the feature format contract:
//! roles start at 0, side-to-move at 10, king-relative board features at
//! 1_000, absolute board fallback features at 10_000, and hands at 20_000.
//! Hand counts use one count-bucket feature per color/piece kind, capped at
//! bucket 19 for malformed positions with unusually large hands.

use shogi_core::{Color, Hand, PieceKind, Square};

use crate::position_wrapper::PositionWrapper;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FeatureId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FeatureRole {
    Attacker,
    Defender,
}

const ROLE_BASE: u32 = 0;
const SIDE_TO_MOVE_BASE: u32 = 10;
const BOARD_RELATIVE_BASE: u32 = 1_000;
const BOARD_ABSOLUTE_BASE: u32 = 10_000;
const HAND_BASE: u32 = 20_000;

const COLORS: u32 = Color::NUM as u32;
const PIECE_KINDS: u32 = PieceKind::NUM as u32;
const SQUARES: u32 = Square::NUM as u32;
const RELATIVE_COORDS: u32 = 17;
const HAND_PIECES: u32 = Hand::NUM_HAND_PIECES as u32;
const HAND_COUNT_BUCKETS: u32 = 20;

impl FeatureRole {
    fn index(self) -> u32 {
        match self {
            FeatureRole::Attacker => 0,
            FeatureRole::Defender => 1,
        }
    }

    fn defender_color(self, side_to_move: Color) -> Color {
        match self {
            FeatureRole::Attacker => side_to_move.flip(),
            FeatureRole::Defender => side_to_move,
        }
    }
}

pub fn position_features(position: &PositionWrapper, role: FeatureRole) -> Vec<FeatureId> {
    let inner = position.inner();
    let mut features = Vec::new();

    features.push(role_feature(role));
    features.push(side_to_move_feature(inner.side_to_move()));

    let defender = role.defender_color(inner.side_to_move());
    let king = inner.king_position(defender);
    for square in Square::all() {
        if let Some(piece) = inner.piece_at(square) {
            let (piece_kind, color) = piece.to_parts();
            features.push(match king {
                Some(king) => board_relative_feature(color, piece_kind, square, king),
                None => board_absolute_feature(color, piece_kind, square),
            });
        }
    }

    for color in Color::all() {
        let hand = inner.hand_of_a_player(color);
        for (piece_index, piece_kind) in Hand::all_hand_pieces().enumerate() {
            let count = hand.count(piece_kind).unwrap_or(0);
            if count > 0 {
                features.push(hand_feature(color, piece_index as u32, count));
            }
        }
    }

    features
}

fn role_feature(role: FeatureRole) -> FeatureId {
    FeatureId(ROLE_BASE + role.index())
}

fn side_to_move_feature(color: Color) -> FeatureId {
    FeatureId(SIDE_TO_MOVE_BASE + color.array_index() as u32)
}

fn board_relative_feature(
    color: Color,
    piece_kind: PieceKind,
    square: Square,
    king: Square,
) -> FeatureId {
    let color_piece = color_piece_index(color, piece_kind);
    let file_delta = square.file() as i16 - king.file() as i16 + 8;
    let rank_delta = square.rank() as i16 - king.rank() as i16 + 8;
    debug_assert!((0..RELATIVE_COORDS as i16).contains(&file_delta));
    debug_assert!((0..RELATIVE_COORDS as i16).contains(&rank_delta));

    FeatureId(
        BOARD_RELATIVE_BASE
            + ((color_piece * RELATIVE_COORDS + file_delta as u32) * RELATIVE_COORDS
                + rank_delta as u32),
    )
}

fn board_absolute_feature(color: Color, piece_kind: PieceKind, square: Square) -> FeatureId {
    FeatureId(
        BOARD_ABSOLUTE_BASE
            + (color_piece_index(color, piece_kind) * SQUARES + square.array_index() as u32),
    )
}

fn hand_feature(color: Color, piece_index: u32, count: u8) -> FeatureId {
    debug_assert!(piece_index < HAND_PIECES);
    let count_bucket = u32::from(count).min(HAND_COUNT_BUCKETS - 1);
    FeatureId(
        HAND_BASE
            + ((color.array_index() as u32 * HAND_PIECES + piece_index) * HAND_COUNT_BUCKETS
                + count_bucket),
    )
}

fn color_piece_index(color: Color, piece_kind: PieceKind) -> u32 {
    let index = color.array_index() as u32 * PIECE_KINDS + piece_kind.array_index() as u32;
    debug_assert!(index < COLORS * PIECE_KINDS);
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_core::PartialPosition;
    use shogi_usi_parser::FromUsi;

    fn wrapped(sfen: &str) -> PositionWrapper {
        PositionWrapper::new(PartialPosition::from_usi(&format!("sfen {sfen}")).unwrap())
    }

    #[test]
    fn extracts_stable_position_features_for_known_sfen() {
        let position = wrapped("4k4/9/9/9/9/9/9/9/4K4 b 2GS 1");

        let features = position_features(&position, FeatureRole::Attacker);

        assert_eq!(
            features,
            vec![
                FeatureId(0),
                FeatureId(10),
                FeatureId(7213),
                FeatureId(3175),
                FeatureId(20061),
                FeatureId(20082),
            ]
        );
    }

    #[test]
    fn repeated_extraction_preserves_ids_and_order() {
        let position = wrapped("4k4/9/9/9/9/9/9/9/4K4 b 2GS 1");

        assert_eq!(
            position_features(&position, FeatureRole::Attacker),
            position_features(&position, FeatureRole::Attacker),
        );
    }

    #[test]
    fn side_to_move_changes_side_feature() {
        let black = wrapped("4k4/9/9/9/9/9/9/9/4K4 b - 1");
        let white = wrapped("4k4/9/9/9/9/9/9/9/4K4 w - 1");

        assert!(position_features(&black, FeatureRole::Attacker).contains(&FeatureId(10)));
        assert!(position_features(&white, FeatureRole::Attacker).contains(&FeatureId(11)));
    }

    #[test]
    fn missing_defender_king_uses_absolute_board_features() {
        let position = wrapped("9/9/9/9/9/9/9/9/4K4 b - 1");

        let features = position_features(&position, FeatureRole::Attacker);

        assert_eq!(
            features,
            vec![FeatureId(0), FeatureId(10), FeatureId(10611)]
        );
    }
}

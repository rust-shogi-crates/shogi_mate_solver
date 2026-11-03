//! Deterministic sparse feature IDs for move-ordering scorers.
//!
//! The ID ranges are part of the feature format contract:
//! roles start at 0, side-to-move at 10, defending-king squares at 100,
//! king-relative board features at 1_000, absolute board fallback features
//! at 10_000, and hands at 20_000.
//! Hand counts use one count-bucket feature per color/piece kind, capped at
//! bucket 19 for malformed positions with unusually large hands.
//! Version 5 normalizes colors and squares to a White defender, folds the
//! king onto files 1..=5, and resolves center-file ties by board order.

use shogi_core::{Color, Hand, Move, PieceKind, Square};

use crate::position_wrapper::PositionWrapper;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FeatureId(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FeatureRole {
    Attacker,
    Defender,
}

pub const FEATURE_VERSION: u32 = 5;

const KING_SQUARE_BASE: u32 = 100;
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
    pub fn flip(self) -> Self {
        match self {
            Self::Attacker => Self::Defender,
            Self::Defender => Self::Attacker,
        }
    }

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

// Normalize the defender to White, then place its king on files 1..=5.
// On file 5 (or without a king), use board lexicographic order to break
// the mirror tie so every horizontally mirrored position shares features.
#[derive(Clone, Copy)]
struct Frame {
    rotate: bool,
    mirror: bool,
}

impl Frame {
    fn new(position: &shogi_core::PartialPosition, defender: Color) -> Self {
        let mut frame = Self {
            rotate: defender == Color::Black,
            mirror: false,
        };
        if let Some(king) = position.king_position(defender) {
            let file = frame.square(king).file();
            if file != 5 {
                frame.mirror = file > 5;
                return frame;
            }
        }
        let reflected = Self {
            mirror: true,
            ..frame
        };
        for square in Square::all() {
            let code = |f: Self| {
                position.piece_at(f.square(square)).map_or(0, |piece| {
                    let (kind, color) = piece.to_parts();
                    1 + f.color(color).array_index() as u32 * PIECE_KINDS + kind as u32
                })
            };
            match code(frame).cmp(&code(reflected)) {
                std::cmp::Ordering::Greater => {
                    frame.mirror = true;
                    break;
                }
                std::cmp::Ordering::Less => break,
                std::cmp::Ordering::Equal => {}
            }
        }
        frame
    }

    fn color(self, color: Color) -> Color {
        if self.rotate {
            color.flip()
        } else {
            color
        }
    }

    fn square(self, square: Square) -> Square {
        let mut file = square.file();
        let mut rank = square.rank();
        if self.rotate {
            file = 10 - file;
            rank = 10 - rank;
        }
        if self.mirror {
            file = 10 - file;
        }
        Square::new(file, rank).unwrap()
    }
}

pub fn position_features(position: &PositionWrapper, role: FeatureRole) -> Vec<FeatureId> {
    let inner = position.inner();
    let mut features = Vec::new();

    features.push(role_feature(role));
    let defender = role.defender_color(inner.side_to_move());
    let frame = Frame::new(inner, defender);
    features.push(side_to_move_feature(frame.color(inner.side_to_move())));

    let king = inner.king_position(defender);
    if let Some(king) = king {
        features.push(king_square_feature(frame.square(king)));
    }
    for square in Square::all() {
        if let Some(piece) = inner.piece_at(square) {
            let (piece_kind, color) = piece.to_parts();
            features.push(match king {
                Some(king) => board_relative_feature(
                    frame.color(color),
                    piece_kind,
                    frame.square(square),
                    frame.square(king),
                ),
                None => {
                    board_absolute_feature(frame.color(color), piece_kind, frame.square(square))
                }
            });
        }
    }

    for color in Color::all() {
        let hand = inner.hand_of_a_player(color);
        for (piece_index, piece_kind) in Hand::all_hand_pieces().enumerate() {
            let count = hand.count(piece_kind).unwrap_or(0);
            if count > 0 {
                features.push(hand_feature(frame.color(color), piece_index as u32, count));
            }
        }
    }

    features.sort_unstable();
    features
}

/// Returns the feature IDs removed from and added to `position` by `mv`.
///
/// `role` describes the parent side to move; the child has the opposite role.
/// Board features are compared square-by-square in O(81), including changes
/// to the defending king's square.
pub fn position_feature_delta(
    position: &PositionWrapper,
    mv: Move,
    role: FeatureRole,
) -> (Vec<FeatureId>, Vec<FeatureId>) {
    let mut child = position.clone();
    child.make_move(mv);
    let parent_inner = position.inner();
    let child_inner = child.inner();
    let parent_king = role.defender_color(parent_inner.side_to_move());
    let child_king = role.flip().defender_color(child_inner.side_to_move());
    let parent_frame = Frame::new(parent_inner, parent_king);
    let child_frame = Frame::new(child_inner, child_king);
    let mut removed = Vec::new();
    let mut added = Vec::new();

    push_changed(
        &mut removed,
        &mut added,
        Some(role_feature(role)),
        Some(role_feature(role.flip())),
    );
    push_changed(
        &mut removed,
        &mut added,
        Some(side_to_move_feature(
            parent_frame.color(parent_inner.side_to_move()),
        )),
        Some(side_to_move_feature(
            child_frame.color(child_inner.side_to_move()),
        )),
    );

    push_changed(
        &mut removed,
        &mut added,
        parent_inner
            .king_position(parent_king)
            .map(|k| king_square_feature(parent_frame.square(k))),
        child_inner
            .king_position(child_king)
            .map(|k| king_square_feature(child_frame.square(k))),
    );
    for square in Square::all() {
        push_changed(
            &mut removed,
            &mut added,
            board_feature(parent_inner, square, parent_king, parent_frame),
            board_feature(child_inner, square, child_king, child_frame),
        );
    }

    for color in Color::all() {
        let parent_hand = parent_inner.hand_of_a_player(color);
        let child_hand = child_inner.hand_of_a_player(color);
        for (piece_index, piece_kind) in Hand::all_hand_pieces().enumerate() {
            let parent_count = parent_hand.count(piece_kind).unwrap_or(0);
            let child_count = child_hand.count(piece_kind).unwrap_or(0);
            push_changed(
                &mut removed,
                &mut added,
                (parent_count > 0).then(|| {
                    hand_feature(parent_frame.color(color), piece_index as u32, parent_count)
                }),
                (child_count > 0).then(|| {
                    hand_feature(child_frame.color(color), piece_index as u32, child_count)
                }),
            );
        }
    }

    (removed, added)
}

fn board_feature(
    position: &shogi_core::PartialPosition,
    square: Square,
    defender_king: Color,
    frame: Frame,
) -> Option<FeatureId> {
    let piece = position.piece_at(square)?;
    let (piece_kind, color) = piece.to_parts();
    Some(match position.king_position(defender_king) {
        Some(king) => board_relative_feature(
            frame.color(color),
            piece_kind,
            frame.square(square),
            frame.square(king),
        ),
        None => board_absolute_feature(frame.color(color), piece_kind, frame.square(square)),
    })
}

fn push_changed(
    removed: &mut Vec<FeatureId>,
    added: &mut Vec<FeatureId>,
    parent: Option<FeatureId>,
    child: Option<FeatureId>,
) {
    if parent != child {
        if let Some(feature) = parent {
            removed.push(feature);
        }
        if let Some(feature) = child {
            added.push(feature);
        }
    }
}

fn king_square_feature(square: Square) -> FeatureId {
    FeatureId(KING_SQUARE_BASE + square.array_index() as u32)
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
    use shogi_core::{Color, Move, PartialPosition, Piece, PieceKind, Square};
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
                king_square_feature(Square::SQ_5A),
                FeatureId(3175),
                FeatureId(7213),
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
    fn side_to_move_is_relative_to_defender() {
        let black = wrapped("4k4/9/9/9/9/9/9/9/4K4 b - 1");
        let white = wrapped("4k4/9/9/9/9/9/9/9/4K4 w - 1");

        assert!(position_features(&black, FeatureRole::Attacker).contains(&FeatureId(10)));
        assert_eq!(
            position_features(&black, FeatureRole::Attacker),
            position_features(&white, FeatureRole::Attacker)
        );
    }

    #[test]
    fn direct_move_delta_matches_position_feature_delta() {
        assert_direct_delta_matches(
            "4k4/9/9/9/9/9/9/9/4K4 b G 1",
            Move::Drop {
                piece: Piece::new(PieceKind::Gold, Color::Black),
                to: Square::SQ_5E,
            },
            FeatureRole::Attacker,
        );
        assert_direct_delta_matches(
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            Move::Normal {
                from: Square::SQ_7G,
                to: Square::SQ_7F,
                promote: false,
            },
            FeatureRole::Attacker,
        );
        assert_direct_delta_matches(
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            Move::Normal {
                from: Square::SQ_7G,
                to: Square::SQ_7F,
                promote: false,
            },
            FeatureRole::Defender,
        );
        assert_direct_delta_matches(
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL b - 1",
            Move::Normal {
                from: Square::SQ_5I,
                to: Square::SQ_5H,
                promote: false,
            },
            FeatureRole::Defender,
        );
    }

    #[test]
    fn symmetric_positions_share_features_for_both_roles() {
        let groups = [
            [
                "7k1/5+p3/6G2/9/9/9/9/1B7/4K4 b 2GSr 1",
                "1k7/3+p5/2G6/9/9/9/9/7B1/4K4 b 2GSr 1",
                "4k4/7b1/9/9/9/9/2g6/3+P5/1K7 w 2gsR 1",
                "4k4/1b7/9/9/9/9/6g2/5+P3/7K1 w 2gsR 1",
            ],
            [
                "4k4/5G3/9/9/9/9/9/9/4K4 w 2GSr 1",
                "4k4/3G5/9/9/9/9/9/9/4K4 w 2GSr 1",
                "4k4/9/9/9/9/9/9/3g5/4K4 b 2gsR 1",
                "4k4/9/9/9/9/9/9/5g3/4K4 b 2gsR 1",
            ],
            [
                "9/5G3/9/9/9/9/9/9/4K4 b 2GSr 1",
                "9/3G5/9/9/9/9/9/9/4K4 b 2GSr 1",
                "4k4/9/9/9/9/9/9/3g5/9 w 2gsR 1",
                "4k4/9/9/9/9/9/9/5g3/9 w 2gsR 1",
            ],
        ];
        for group in groups {
            for role in [FeatureRole::Attacker, FeatureRole::Defender] {
                let expected = position_features(&wrapped(group[0]), role);
                for sfen in &group[1..] {
                    assert_eq!(position_features(&wrapped(sfen), role), expected, "{sfen}");
                }
            }
        }
    }

    #[test]
    fn delta_handles_canonical_frame_switches() {
        let sfen = "4k4/5G3/9/9/9/9/9/9/4K4 b - 1";
        let mv = Move::Normal {
            from: Square::SQ_4B,
            to: Square::SQ_5B,
            promote: false,
        };
        let parent = wrapped(sfen);
        let mut child = parent.clone();
        child.make_move(mv);
        assert_ne!(
            Frame::new(parent.inner(), Color::White).mirror,
            Frame::new(child.inner(), Color::White).mirror
        );
        assert_direct_delta_matches(sfen, mv, FeatureRole::Attacker);
        assert_direct_delta_matches(
            "5k3/9/9/9/9/9/9/9/4K4 w 2GSr 1",
            Move::Normal {
                from: Square::SQ_4A,
                to: Square::SQ_5A,
                promote: false,
            },
            FeatureRole::Defender,
        );
    }

    #[test]
    fn translated_positions_have_different_features() {
        let edge = wrapped("8k/7G1/7K1/9/9/9/9/9/9 w - 1");
        let inland = wrapped("9/8k/7G1/7K1/9/9/9/9/9 w - 1");
        assert_ne!(
            position_features(&edge, FeatureRole::Defender),
            position_features(&inland, FeatureRole::Defender)
        );
    }

    #[test]
    fn delta_handles_white_king_and_promoting_capture() {
        assert_direct_delta_matches(
            "4k4/9/9/9/9/9/9/9/4K4 w - 1",
            Move::Normal {
                from: Square::SQ_5A,
                to: Square::SQ_4B,
                promote: false,
            },
            FeatureRole::Defender,
        );
        assert_direct_delta_matches(
            "4k4/4s4/4P4/9/9/9/9/9/4K4 b - 1",
            Move::Normal {
                from: Square::SQ_5C,
                to: Square::SQ_5B,
                promote: true,
            },
            FeatureRole::Attacker,
        );
    }

    fn assert_direct_delta_matches(sfen: &str, mv: Move, role: FeatureRole) {
        let position = wrapped(sfen);
        let mut child = position.clone();
        child.make_move(mv);

        let mut actual = position_features(&position, role);
        let (removed, added) = position_feature_delta(&position, mv, role);
        // Give every relevant feature a distinct contribution so score equality
        // also exercises king-square updates, not only feature bookkeeping.
        let mut model = crate::nnue::parse::DeepModel::empty();
        for feature in actual.iter().chain(added.iter()) {
            let mut weights = vec![0; crate::nnue::parse::DEEP_INPUTS];
            weights[0] = feature.0 as i32;
            model.input_weights.insert(feature.0, weights);
        }
        let scorer = crate::nnue::NnueScorer::from_model(&model.to_text()).unwrap();
        assert_eq!(
            scorer.score_accumulator_delta(&scorer.accumulator(&actual), &removed, &added),
            scorer.score(&position_features(&child, role.flip()))
        );
        for feature in removed {
            let index = actual
                .iter()
                .position(|candidate| *candidate == feature)
                .unwrap();
            actual.remove(index);
        }
        actual.extend(added);
        let mut expected = position_features(&child, role.flip());
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected);
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

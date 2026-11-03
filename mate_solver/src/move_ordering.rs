use shogi_core::Move;

use crate::{
    features::{position_feature_delta, position_features, FeatureId, FeatureRole},
    nnue::{NnueAccumulator, NnueScorer},
    position_wrapper::PositionWrapper,
    tt::DfPnTable,
};

pub trait FeatureScorer {
    fn score(&self, features: &[FeatureId]) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixtureScorer {
    entries: &'static [(FeatureId, i32)],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MoveOrderingScorer {
    Fixture(FixtureScorer),
    Nnue(NnueScorer),
}

impl Default for MoveOrderingScorer {
    fn default() -> Self {
        Self::Fixture(FixtureScorer::default())
    }
}

impl FeatureScorer for MoveOrderingScorer {
    fn score(&self, features: &[FeatureId]) -> i32 {
        match self {
            Self::Fixture(scorer) => scorer.score(features),
            Self::Nnue(scorer) => scorer.score(features),
        }
    }
}

impl FeatureScorer for NnueScorer {
    fn score(&self, features: &[FeatureId]) -> i32 {
        NnueScorer::score(self, features)
    }
}

impl FixtureScorer {
    pub const fn new(entries: &'static [(FeatureId, i32)]) -> Self {
        Self { entries }
    }
}

impl Default for FixtureScorer {
    fn default() -> Self {
        // FeatureId(10_691) is the preferred child-position feature in the fixture test.
        Self::new(&[(FeatureId(10_691), 100)])
    }
}

impl FeatureScorer for FixtureScorer {
    fn score(&self, features: &[FeatureId]) -> i32 {
        self.entries
            .iter()
            .map(|&(feature, value)| {
                value * features.iter().filter(|&&id| id == feature).count() as i32
            })
            .sum()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MoveOrderingOptions {
    pub mode: MoveOrderingMode,
    pub scorer: MoveOrderingScorer,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum MoveOrderingMode {
    #[default]
    Current,
    FixtureScore,
    NnueFixture,
    NnueModel,
    /// Order DF-PN candidates solely by NNUE score, across move categories.
    NnueModelGlobal,
}

pub fn order_df_pn_moves(
    moves: &mut [Move],
    position: &PositionWrapper,
    role: FeatureRole,
    options: &MoveOrderingOptions,
) {
    if options.mode == MoveOrderingMode::Current {
        moves.sort_unstable_by_key(|&mv| df_pn_primary_key(mv));
        return;
    }

    let base_accumulator = match &options.scorer {
        MoveOrderingScorer::Fixture(_) => None,
        MoveOrderingScorer::Nnue(scorer) => {
            Some(scorer.accumulator(&position_features(position, role)))
        }
    };
    let mut ordered: Vec<_> = moves
        .iter()
        .copied()
        .enumerate()
        .map(|(index, mv)| {
            let primary = if options.mode == MoveOrderingMode::NnueModelGlobal {
                0
            } else {
                df_pn_primary_key(mv)
            };
            let score = score_child_position(
                &options.scorer,
                base_accumulator.as_ref(),
                position,
                mv,
                role,
            );
            (primary, score_order_key(score, role), index, mv)
        })
        .collect();
    ordered.sort_unstable_by_key(|&(primary, score, index, _)| (primary, score, index));
    for (destination, (_, _, _, mv)) in moves.iter_mut().zip(ordered) {
        *destination = mv;
    }
}

pub fn order_eval_moves_with_role(
    moves: &mut [Move],
    position: &PositionWrapper,
    df_pn: &DfPnTable,
    role: FeatureRole,
    options: &MoveOrderingOptions,
) {
    match options.mode {
        MoveOrderingMode::Current => moves.sort_unstable_by_key(|&mv| {
            let mut cp = position.clone();
            cp.make_move(mv);
            if let Some((_, delta)) = df_pn.fetch(cp.zobrist_hash()) {
                delta
            } else {
                1
            }
        }),
        MoveOrderingMode::FixtureScore
        | MoveOrderingMode::NnueFixture
        | MoveOrderingMode::NnueModel
        | MoveOrderingMode::NnueModelGlobal => {
            let base_accumulator = match &options.scorer {
                MoveOrderingScorer::Fixture(_) => None,
                MoveOrderingScorer::Nnue(scorer) => {
                    Some(scorer.accumulator(&position_features(position, role)))
                }
            };
            let mut ordered: Vec<_> = moves
                .iter()
                .copied()
                .enumerate()
                .map(|(index, mv)| {
                    let mut cp = position.clone();
                    cp.make_move(mv);
                    let primary = df_pn
                        .fetch(cp.zobrist_hash())
                        .map(|(_, delta)| delta)
                        .unwrap_or(1);
                    let score = score_child_position(
                        &options.scorer,
                        base_accumulator.as_ref(),
                        position,
                        mv,
                        role,
                    );
                    (primary, score_order_key(score, role), index, mv)
                })
                .collect();
            ordered.sort_unstable_by_key(|&(primary, score, index, _)| (primary, score, index));
            for (destination, (_, _, _, mv)) in moves.iter_mut().zip(ordered) {
                *destination = mv;
            }
        }
    }
}

fn score_child_position(
    scorer: &MoveOrderingScorer,
    base_accumulator: Option<&NnueAccumulator>,
    position: &PositionWrapper,
    mv: Move,
    role: FeatureRole,
) -> i32 {
    match scorer {
        MoveOrderingScorer::Fixture(scorer) => {
            let mut child = position.clone();
            child.make_move(mv);
            scorer.score(&position_features(&child, role.flip()))
        }
        MoveOrderingScorer::Nnue(scorer) => {
            let (removed, added) = position_feature_delta(position, mv, role);
            scorer.score_accumulator_delta(
                base_accumulator.expect("NNUE base accumulator"),
                &removed,
                &added,
            )
        }
    }
}

// Scores estimate attacker success, regardless of whose turn it is.
fn score_order_key(score: i32, role: FeatureRole) -> i64 {
    match role {
        FeatureRole::Attacker => -i64::from(score),
        FeatureRole::Defender => i64::from(score),
    }
}

fn df_pn_primary_key(mv: Move) -> u8 {
    match mv {
        Move::Normal { .. } => 0,
        Move::Drop { piece, .. } => 60 - piece.piece_kind() as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_core::{Color, PartialPosition, Piece, PieceKind, Square};
    use shogi_usi_parser::FromUsi;

    #[test]
    fn trained_ordering_uses_child_role_and_parent_objective_for_both_colors() {
        for (side, piece) in [("b", "R"), ("w", "r")] {
            let position = PositionWrapper::new(
                PartialPosition::from_usi(&format!(
                    "sfen 4k4/9/9/9/4{piece}4/9/9/9/4K4 {side} - 1"
                ))
                .unwrap(),
            );
            let moves = [
                Move::Normal {
                    from: Square::SQ_5E,
                    to: Square::SQ_5D,
                    promote: false,
                },
                Move::Normal {
                    from: Square::SQ_5E,
                    to: Square::SQ_4E,
                    promote: false,
                },
            ];
            for role in [FeatureRole::Attacker, FeatureRole::Defender] {
                let mut first_child = position.clone();
                first_child.make_move(moves[0]);
                let mut second_child = position.clone();
                second_child.make_move(moves[1]);
                let first_features = position_features(&first_child, role.flip());
                let second_features = position_features(&second_child, role.flip());
                let signal = first_features
                    .iter()
                    .find(|feature| !second_features.contains(feature))
                    .unwrap();
                let mut model = crate::nnue::parse::DeepModel::empty();
                model
                    .input_weights
                    .insert(signal.0, vec![1; crate::nnue::parse::DEEP_INPUTS]);
                model.input_weights.insert(
                    match role.flip() {
                        FeatureRole::Attacker => 0,
                        FeatureRole::Defender => 1,
                    },
                    vec![2; crate::nnue::parse::DEEP_INPUTS],
                );
                let scorer = NnueScorer::from_model(&model.to_text()).unwrap();
                let expected_score = scorer.score(&first_features);
                assert!(expected_score > scorer.score(&second_features));
                let accumulator = scorer.accumulator(&position_features(&position, role));
                let options = MoveOrderingOptions {
                    mode: MoveOrderingMode::NnueModel,
                    scorer: MoveOrderingScorer::Nnue(scorer),
                };
                assert_eq!(
                    score_child_position(
                        &options.scorer,
                        Some(&accumulator),
                        &position,
                        moves[0],
                        role
                    ),
                    expected_score
                );
                let expected = match role {
                    FeatureRole::Attacker => moves,
                    FeatureRole::Defender => [moves[1], moves[0]],
                };
                let mut ordered = moves;
                order_df_pn_moves(&mut ordered, &position, role, &options);
                assert_eq!(ordered, expected);
                ordered = moves;
                order_eval_moves_with_role(
                    &mut ordered,
                    &position,
                    &DfPnTable::new(16),
                    role,
                    &options,
                );
                assert_eq!(ordered, expected);
            }
        }
        assert!(
            score_order_key(i32::MIN, FeatureRole::Attacker)
                > score_order_key(i32::MAX, FeatureRole::Attacker)
        );
    }

    #[test]
    fn global_nnue_orders_across_categories_for_both_colors_and_roles() {
        for (side, rook, hand, color) in [
            ("b", "R", "GS", Color::Black),
            ("w", "r", "gs", Color::White),
        ] {
            let position = PositionWrapper::new(
                PartialPosition::from_usi(&format!(
                    "sfen 4k4/9/9/9/4{rook}4/9/9/9/4K4 {side} {hand} 1"
                ))
                .unwrap(),
            );
            let moves = [
                Move::Normal {
                    from: Square::SQ_5E,
                    to: Square::SQ_5D,
                    promote: false,
                },
                Move::Drop {
                    piece: Piece::new(PieceKind::Gold, color),
                    to: Square::SQ_4D,
                },
                Move::Drop {
                    piece: Piece::new(PieceKind::Silver, color),
                    to: Square::SQ_6D,
                },
            ];
            for role in [FeatureRole::Attacker, FeatureRole::Defender] {
                let features: Vec<_> = moves
                    .iter()
                    .map(|&mv| {
                        let mut child = position.clone();
                        child.make_move(mv);
                        position_features(&child, role.flip())
                    })
                    .collect();
                let signal = features[1]
                    .iter()
                    .find(|f| !features[0].contains(f) && !features[2].contains(f))
                    .unwrap();
                let mut model = crate::nnue::parse::DeepModel::empty();
                let sign = if role == FeatureRole::Attacker { 1 } else { -1 };
                model
                    .input_weights
                    .insert(signal.0, vec![sign; crate::nnue::parse::DEEP_INPUTS]);
                let mut options = MoveOrderingOptions {
                    mode: MoveOrderingMode::NnueModelGlobal,
                    scorer: MoveOrderingScorer::Nnue(
                        NnueScorer::from_model(&model.to_text()).unwrap(),
                    ),
                };
                let mut ordered = moves;
                order_df_pn_moves(&mut ordered, &position, role, &options);
                assert_eq!(ordered, [moves[1], moves[0], moves[2]]);
                options.mode = MoveOrderingMode::NnueModel;
                order_df_pn_moves(&mut ordered, &position, role, &options);
                assert_eq!(
                    ordered[0], moves[0],
                    "existing mode retains category precedence"
                );
                options.mode = MoveOrderingMode::NnueModelGlobal;
                options.scorer = MoveOrderingScorer::Nnue(
                    NnueScorer::from_model(&crate::nnue::parse::DeepModel::empty().to_text())
                        .unwrap(),
                );
                let mut tied = [moves[2], moves[1], moves[0]];
                order_df_pn_moves(&mut tied, &position, role, &options);
                assert_eq!(
                    tied,
                    [moves[2], moves[1], moves[0]],
                    "equal scores preserve input order across categories"
                );
            }
        }
    }

    #[test]
    fn current_df_pn_order_keeps_normal_moves_before_drops() {
        let normal = Move::Normal {
            from: Square::SQ_2B,
            to: Square::SQ_2A,
            promote: false,
        };
        let drop = Move::Drop {
            piece: Piece::new(PieceKind::Pawn, Color::Black),
            to: Square::SQ_5E,
        };
        let mut moves = [drop, normal];
        let position = PositionWrapper::new(
            PartialPosition::from_usi("sfen 9/9/9/9/9/9/9/9/9 b P 1").unwrap(),
        );

        order_df_pn_moves(
            &mut moves,
            &position,
            FeatureRole::Attacker,
            &MoveOrderingOptions::default(),
        );

        assert_eq!(moves, [normal, drop]);
    }

    #[test]
    fn current_eval_order_uses_child_delta() {
        let position = PositionWrapper::new(
            PartialPosition::from_usi("sfen 9/9/9/9/9/9/9/9/9 b GS 1").unwrap(),
        );
        let first = Move::Drop {
            piece: Piece::new(PieceKind::Gold, Color::Black),
            to: Square::SQ_5E,
        };
        let second = Move::Drop {
            piece: Piece::new(PieceKind::Silver, Color::Black),
            to: Square::SQ_4E,
        };
        let mut first_position = position.clone();
        first_position.make_move(first);
        let mut second_position = position.clone();
        second_position.make_move(second);

        let mut df_pn = DfPnTable::new(16);
        df_pn.insert(first_position.zobrist_hash(), (1, 8));
        df_pn.insert(second_position.zobrist_hash(), (1, 2));
        let mut moves = [first, second];

        order_eval_moves_with_role(
            &mut moves,
            &position,
            &df_pn,
            FeatureRole::Attacker,
            &MoveOrderingOptions::default(),
        );

        assert_eq!(moves, [second, first]);
    }

    #[test]
    fn fixture_score_breaks_ties_without_changing_primary_order() {
        let position = PositionWrapper::new(
            PartialPosition::from_usi("sfen 9/9/9/9/9/9/9/9/4P4 b GS 1").unwrap(),
        );
        let preferred = Move::Normal {
            from: Square::SQ_5I,
            to: Square::SQ_5H,
            promote: true,
        };
        let other = Move::Normal {
            from: Square::SQ_5I,
            to: Square::SQ_4H,
            promote: false,
        };
        let mut moves = [other, preferred];
        let options = MoveOrderingOptions {
            mode: MoveOrderingMode::FixtureScore,
            scorer: MoveOrderingScorer::Fixture(FixtureScorer::default()),
        };

        order_df_pn_moves(&mut moves, &position, FeatureRole::Attacker, &options);

        assert_eq!(moves, [preferred, other]);
    }

    #[test]
    fn nnue_fixture_preserves_input_order_for_equal_child_scores() {
        let position = PositionWrapper::new(
            PartialPosition::from_usi("sfen 9/9/9/9/9/9/9/9/4P4 b GS 1").unwrap(),
        );
        let preferred = Move::Normal {
            from: Square::SQ_5I,
            to: Square::SQ_5H,
            promote: true,
        };
        let other = Move::Normal {
            from: Square::SQ_5I,
            to: Square::SQ_4H,
            promote: false,
        };
        let mut moves = [other, preferred];
        let options = MoveOrderingOptions {
            mode: MoveOrderingMode::NnueFixture,
            scorer: MoveOrderingScorer::Nnue(NnueScorer::default()),
        };

        order_df_pn_moves(&mut moves, &position, FeatureRole::Attacker, &options);

        let scorer = match &options.scorer {
            MoveOrderingScorer::Nnue(scorer) => scorer,
            _ => unreachable!(),
        };
        let mut first = position.clone();
        first.make_move(preferred);
        let mut second = position.clone();
        second.make_move(other);
        assert_eq!(
            scorer.score(&position_features(&first, FeatureRole::Defender)),
            scorer.score(&position_features(&second, FeatureRole::Defender))
        );
        assert_eq!(moves, [other, preferred]);
    }

    #[test]
    fn fixture_scorer_can_distinguish_roles() {
        let scorer = FixtureScorer::new(&[(FeatureId(0), 7), (FeatureId(1), -3)]);

        assert_eq!(scorer.score(&[FeatureId(0)]), 7);
        assert_eq!(scorer.score(&[FeatureId(1)]), -3);
    }
}

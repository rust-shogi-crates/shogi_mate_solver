use shogi_core::Move;

use crate::{
    features::{candidate_features, FeatureId, FeatureRole},
    nnue::NnueScorer,
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
        // FeatureId(30_300) is MOVE_PROMOTE, so the fixture favors promotions.
        Self::new(&[(FeatureId(30_300), 100)])
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

    let mut ordered: Vec<_> = moves
        .iter()
        .copied()
        .enumerate()
        .map(|(index, mv)| {
            let primary = df_pn_primary_key(mv);
            let score = options
                .scorer
                .score(&candidate_features(position, mv, role));
            (primary, -score, index, mv)
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
        MoveOrderingMode::FixtureScore | MoveOrderingMode::NnueFixture => {
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
                    let score = options
                        .scorer
                        .score(&candidate_features(position, mv, role));
                    (primary, -score, index, mv)
                })
                .collect();
            ordered.sort_unstable_by_key(|&(primary, score, index, _)| (primary, score, index));
            for (destination, (_, _, _, mv)) in moves.iter_mut().zip(ordered) {
                *destination = mv;
            }
        }
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
            PartialPosition::from_usi("sfen 9/9/9/9/9/9/9/9/9 b GS 1").unwrap(),
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
    fn nnue_fixture_score_breaks_ties_without_changing_primary_order() {
        let position = PositionWrapper::new(
            PartialPosition::from_usi("sfen 9/9/9/9/9/9/9/9/9 b GS 1").unwrap(),
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

        assert_eq!(moves, [preferred, other]);
    }

    #[test]
    fn fixture_scorer_can_distinguish_roles() {
        let scorer = FixtureScorer::new(&[(FeatureId(0), 7), (FeatureId(1), -3)]);

        assert_eq!(scorer.score(&[FeatureId(0)]), 7);
        assert_eq!(scorer.score(&[FeatureId(1)]), -3);
    }
}

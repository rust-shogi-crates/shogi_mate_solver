use shogi_core::Move;

use crate::{position_wrapper::PositionWrapper, tt::DfPnTable};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MoveOrderingOptions {
    pub mode: MoveOrderingMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MoveOrderingMode {
    #[default]
    Current,
}

pub fn order_df_pn_moves(moves: &mut [Move], options: &MoveOrderingOptions) {
    match options.mode {
        MoveOrderingMode::Current => moves.sort_unstable_by_key(|&mv| match mv {
            Move::Normal { .. } => 0,
            Move::Drop { piece, .. } => 60 - piece.piece_kind() as u8,
        }),
    }
}

pub fn order_eval_moves(
    moves: &mut [Move],
    position: &PositionWrapper,
    df_pn: &DfPnTable,
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

        order_df_pn_moves(&mut moves, &MoveOrderingOptions::default());

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

        order_eval_moves(
            &mut moves,
            &position,
            &df_pn,
            &MoveOrderingOptions::default(),
        );

        assert_eq!(moves, [second, first]);
    }
}

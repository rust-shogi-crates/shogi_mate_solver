use once_cell::sync::Lazy;
use shogi_core::{Color, Hand, Move, PartialPosition, PieceKind, Square};

type Key = u64;

pub struct PositionWrapper {
    inner: PartialPosition,
    hash: Key,
}
impl PositionWrapper {
    pub fn new(position: PartialPosition) -> Self {
        let hash = Self::compute_hash(&position);
        Self {
            inner: position,
            hash,
        }
    }

    /// 攻め方の王手の一覧。
    pub fn all_checks(&self) -> Vec<Move> {
        shogi_legality_lite::all_checks_partial(&self.inner)
    }

    /// 玉方の手の一覧。
    pub fn all_evasions(&self) -> Vec<Move> {
        shogi_legality_lite::all_legal_moves_partial(&self.inner)
    }

    /// 局面のハッシュ値。この値は衝突してはならない。
    pub fn zobrist_hash(&self) -> u64 {
        self.hash
    }

    /// 手を指す。ハッシュ値も更新する。
    pub fn make_move(&mut self, mv: Move) {
        if self.inner.make_move(mv).is_some() {
            // TODO: differential update
            let hash = Self::compute_hash(&self.inner);
            self.hash = hash;
        }
    }

    /// 局面のハッシュ値を計算する。
    fn compute_hash(position: &PartialPosition) -> Key {
        let mut x = 0;
        for i in 0..81 {
            // Safety: i+1 は 1..=81 に含まれる
            let square = unsafe { Square::from_u8_unchecked(i + 1) };
            let piece = position.piece_at(square);
            if let Some(piece) = piece {
                let (piece_kind, color) = piece.to_parts();
                x ^= TABLE.board[square.array_index()][color.array_index()]
                    [piece_kind.array_index()];
            }
        }
        if position.side_to_move() == Color::White {
            x ^= COLOR_HASH;
        }
        for color in Color::all() {
            let hand = position.hand_of_a_player(color);
            for piece_kind in Hand::all_hand_pieces() {
                let num = unsafe { hand.count(piece_kind).unwrap_unchecked() };
                for i in 0..num {
                    x ^= TABLE.hands[color.array_index()][piece_kind.array_index()][i as usize];
                }
            }
        }
        x
    }
}

struct ZobristTable {
    board: [[[u64; PieceKind::NUM]; Color::NUM]; Square::NUM],
    hands: [[[u64; 18]; Hand::NUM_HAND_PIECES]; 2],
}

static TABLE: Lazy<ZobristTable> = Lazy::new(|| {
    use rand::{Rng, SeedableRng};

    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(0xe964);
    let mut board = [[[0; PieceKind::NUM]; Color::NUM]; Square::NUM];
    let mut hands = [[[0; 18]; Hand::NUM_HAND_PIECES]; Color::NUM];
    for v in &mut board {
        for v in v {
            for v in v {
                *v = rng.gen();
            }
        }
    }
    for v in &mut hands {
        for v in v {
            for v in v {
                *v = rng.gen();
            }
        }
    }
    ZobristTable { board, hands }
});

const COLOR_HASH: Key = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_core::Piece;

    #[test]
    fn empty_hash() {
        let position = PartialPosition::empty();
        let position = PositionWrapper::new(position);
        assert_eq!(position.zobrist_hash(), 0);
    }

    #[test]
    fn startpos_hash() {
        let position = PartialPosition::startpos();
        let mut position = PositionWrapper::new(position);
        let init_hash = position.zobrist_hash();
        let moves = [
            Move::Normal {
                from: Square::SQ_5I,
                to: Square::SQ_5H,
                promote: false,
            },
            Move::Normal {
                from: Square::SQ_5A,
                to: Square::SQ_5B,
                promote: false,
            },
            Move::Normal {
                from: Square::SQ_5H,
                to: Square::SQ_5I,
                promote: false,
            },
            Move::Normal {
                from: Square::SQ_5B,
                to: Square::SQ_5A,
                promote: false,
            },
        ];
        for mv in moves {
            position.make_move(mv);
        }
        assert_eq!(position.zobrist_hash(), init_hash);

        // 駒取り・駒打ちを含む同一局面。2 手目と 8 手目の直後が同一。
        let moves = [
            Move::Normal {
                from: Square::SQ_7G,
                to: Square::SQ_7F,
                promote: false,
            },
            Move::Normal {
                from: Square::SQ_3C,
                to: Square::SQ_3D,
                promote: false,
            },
            Move::Normal {
                from: Square::SQ_8H,
                to: Square::SQ_2B,
                promote: true,
            },
            Move::Normal {
                from: Square::SQ_3A,
                to: Square::SQ_2B,
                promote: false,
            },
            Move::Drop {
                piece: Piece::B_B,
                to: Square::SQ_7G,
            },
            Move::Normal {
                from: Square::SQ_2B,
                to: Square::SQ_3A,
                promote: false,
            },
            Move::Normal {
                from: Square::SQ_7G,
                to: Square::SQ_8H,
                promote: false,
            },
            Move::Drop {
                piece: Piece::W_B,
                to: Square::SQ_2B,
            },
        ];
        let mut hashes = vec![init_hash; 9];
        for i in 0..8 {
            position.make_move(moves[i]);
            hashes[i + 1] = position.zobrist_hash();
        }
        assert_eq!(hashes[2], hashes[8]);
    }
}

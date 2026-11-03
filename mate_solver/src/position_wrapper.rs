use once_cell::sync::Lazy;
use shogi_core::{Color, Hand, Move, PartialPosition, Piece, PieceKind, Square};

pub type Key = u64;

#[derive(Clone, Debug)]
pub struct PositionWrapper {
    inner: PartialPosition,
    hash: Key,
}
impl PositionWrapper {
    #[inline]
    pub fn new(position: PartialPosition) -> Self {
        let hash = Self::compute_hash(&position);
        Self {
            inner: position,
            hash,
        }
    }

    /// 攻め方の王手の一覧。
    pub fn all_checks(&self) -> Vec<Move> {
        // Build checks without all_checks_partial. Its drop-pawn legality path
        // can recursively re-enter is_mate through all_valid_moves on large
        // hand-heavy positions. Normal moves and checking drops can be tested
        // independently without that recursive path.
        let side = self.inner.side_to_move();
        let mut moves = Vec::new();
        for from in Square::all() {
            if !self.inner.player_bitboard(side).contains(from) {
                continue;
            }
            for to in shogi_legality_lite::normal_from_candidates(&self.inner, from) {
                for promote in [false, true] {
                    let mv = Move::Normal { from, to, promote };
                    if !shogi_legality_lite::is_legal_partial_lite(&self.inner, mv) {
                        continue;
                    }
                    let mut next = self.inner.clone();
                    next.make_move(mv);
                    if shogi_legality_lite::is_in_check_partial_lite(&next) {
                        moves.push(mv);
                    }
                }
            }
        }
        let hand = self.inner.hand_of_a_player(side);
        for piece_kind in Hand::all_hand_pieces() {
            if unsafe { hand.count(piece_kind).unwrap_unchecked() } == 0 {
                continue;
            }
            for to in shogi_legality_lite::all_drop_checks_partial(&self.inner, piece_kind) {
                moves.push(Move::Drop {
                    piece: Piece::new(piece_kind, side),
                    to,
                });
            }
        }
        moves.sort_by_key(|mv| !mv.is_promoting()); // promoting moves come first
        moves
    }

    /// 玉方の手の一覧。
    #[inline(always)]
    pub fn all_evasions(&self) -> Vec<Move> {
        shogi_legality_lite::all_legal_moves_partial(&self.inner)
    }

    /// 局面のハッシュ値。この値は衝突してはならない。
    #[inline(always)]
    pub fn zobrist_hash(&self) -> u64 {
        self.hash
    }

    #[inline(always)]
    pub fn inner(&self) -> &PartialPosition {
        &self.inner
    }

    /// 手を指す。ハッシュ値も更新する。
    pub fn make_move(&mut self, mv: Move) {
        let mut diff = 0;
        if let Move::Normal { to, .. } = mv {
            let original = self.inner.piece_at(to);
            if let Some(original) = original {
                let (piece_kind, color) = original.to_parts();
                diff = TABLE.board[to.array_index()][color.array_index()][piece_kind.array_index()];
                let unpromoted = if let Some(x) = piece_kind.unpromote() {
                    x
                } else {
                    piece_kind
                };
                let num = self
                    .inner
                    .hand(Piece::new(unpromoted, color.flip()))
                    .unwrap();
                diff ^=
                    TABLE.hands[1 - color.array_index()][unpromoted.array_index()][num as usize];
            }
        }
        if self.inner.make_move(mv).is_some() {
            match mv {
                Move::Normal { from, to, promote } => {
                    let piece = self.inner.piece_at(to);
                    if let Some(piece) = piece {
                        let (piece_kind, color) = piece.to_parts();
                        let before = if promote {
                            piece_kind.unpromote().unwrap()
                        } else {
                            piece_kind
                        };
                        diff ^= TABLE.board[to.array_index()][color.array_index()]
                            [piece_kind.array_index()];
                        diff ^= TABLE.board[from.array_index()][color.array_index()]
                            [before.array_index()];
                    }
                }
                Move::Drop { piece, to } => {
                    let (piece_kind, color) = piece.to_parts();
                    // 盤上の差分
                    diff ^= TABLE.board[to.array_index()][color.array_index()]
                        [piece_kind.array_index()];
                    // 持ち駒の差分
                    let now = self.inner.hand(piece).unwrap();
                    diff ^=
                        TABLE.hands[color.array_index()][piece_kind.array_index()][now as usize];
                }
            }
            diff ^= COLOR_HASH;
            self.hash ^= diff;
        } else {
            unreachable!();
        }
    }

    /// 攻め方 (先手) の持ち駒を相手の駒台に移動させる。ハッシュ値も更新する。
    pub fn give_piece(&mut self, piece_kind: PieceKind) {
        let s = self.inner.hand_of_a_player_mut(Color::Black);
        *s = s.removed(piece_kind).unwrap();
        self.hash ^= TABLE.hands[Color::Black.array_index()][piece_kind.array_index()]
            [s.count(piece_kind).unwrap() as usize];
        let s = self.inner.hand_of_a_player_mut(Color::White);
        self.hash ^= TABLE.hands[Color::White.array_index()][piece_kind.array_index()]
            [s.count(piece_kind).unwrap() as usize];
        *s = s.added(piece_kind).unwrap();
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
    hands: [[[u64; 18]; Hand::NUM_HAND_PIECES]; Color::NUM],
}

static TABLE: Lazy<ZobristTable> = Lazy::new(|| {
    use rand::{Rng, SeedableRng};

    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(0xe964);
    let mut board = [[[0; PieceKind::NUM]; Color::NUM]; Square::NUM];
    let mut hands = [[[0; 18]; Hand::NUM_HAND_PIECES]; Color::NUM];
    for v in &mut board {
        for v in v {
            for v in v {
                *v = rng.random();
            }
        }
    }
    for v in &mut hands {
        for v in v {
            for v in v {
                *v = rng.random();
            }
        }
    }
    ZobristTable { board, hands }
});

const COLOR_HASH: Key = 1;

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut hashes = [init_hash; 9];
        for i in 0..8 {
            position.make_move(moves[i]);
            hashes[i + 1] = position.zobrist_hash();
            assert_eq!(
                position.zobrist_hash(),
                PositionWrapper::compute_hash(&position.inner)
            );
        }
        assert_eq!(hashes[2], hashes[8]);
    }

    #[test]
    fn give_piece_works() {
        let piece_kind = PieceKind::Pawn;
        let mut position = PartialPosition::startpos();
        position
            .hand_of_a_player_mut(Color::Black)
            .Hand_add(piece_kind);
        let mut position = PositionWrapper::new(position);
        position.give_piece(piece_kind);
        assert_eq!(
            position.hash,
            PositionWrapper::compute_hash(&position.inner),
        );
    }
}

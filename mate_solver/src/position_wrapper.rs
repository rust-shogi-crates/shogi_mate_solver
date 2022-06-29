use shogi_core::Move;
pub trait PositionLike {
    /// 攻め方の王手の一覧。
    fn all_my_checks(&self) -> Vec<Move>;

    /// 玉方の手の一覧。
    fn all_evasions(&self) -> Vec<Move>;

    /// 局面のハッシュ値。この値は衝突してはならない。
    fn zobrist_hash(&self) -> u64;

    /// 手を指す。
    fn make_move(&mut self, mv: Move);
}

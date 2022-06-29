use std::fmt::Debug;

/// 評価値。小さいほど攻め方に有利。
///
/// 32 ビットで、12 (手数) + 8 (攻め方の持ち駒の個数) + 12 (無駄な合駒の個数を反転したもの) というレイアウト。
#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct Value(pub u32);

impl Value {
    pub fn new(plies: u32, pieces: u32, futile: u32) -> Self {
        Self(plies << 20 | pieces << 12 | (0xfff - futile))
    }
    pub fn plies_added(self, a: i32) -> Self {
        Self(self.0.wrapping_add((a as u32) << 20))
    }
    pub fn pieces_added(self, a: i32) -> Self {
        Self(self.0.wrapping_add((a as u32) << 12))
    }
    pub fn futile_added(self, a: i32) -> Self {
        Self(self.0.wrapping_sub(a as u32))
    }
}

impl Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let plies = self.0 >> 20;
        let pieces = (self.0 >> 12) & 0xff;
        let futile_inverted = self.0 & 0xfff;
        write!(
            f,
            "MC={}, P={}, F={}",
            plies,
            pieces,
            0xfff - futile_inverted
        )
    }
}

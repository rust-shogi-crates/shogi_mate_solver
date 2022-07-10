use std::fmt::Debug;

/// 評価値。小さいほど攻め方に有利。
///
/// 32 ビットで、12 (手数) + 8 (攻め方の持ち駒の個数を反転したもの) + 12 (無駄な合駒の個数を反転したもの) というレイアウト。
#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone)]
pub struct Value(pub u32);

impl Value {
    // いかなる詰みよりも良い評価値。
    pub const ZERO: Self = Self(0);
    // 不詰。
    pub const INF: Self = Self(0xffff_ffff);
    const PLY_MASK: u32 = 0xfff0_0000;
    pub fn new(plies: u32, pieces: u32, futile: u32) -> Self {
        Self(plies << 20 | (0xff - pieces) << 12 | (0xfff - futile))
    }

    /// 詰みかどうかを返す。
    pub fn is_mate(self) -> bool {
        self.0 < Self::PLY_MASK
    }

    #[inline(always)]
    pub fn plies(self) -> u32 {
        self.0 >> 20
    }
    #[inline(always)]
    pub fn pieces(self) -> u32 {
        0xff - ((self.0 >> 12) & 0xff)
    }
    #[inline(always)]
    pub fn futile(self) -> u32 {
        0xfff - (self.0 & 0xfff)
    }

    pub fn plies_added_unchecked(self, a: i32) -> Self {
        if self.0 >= Self::PLY_MASK {
            Self::INF
        } else {
            Self(self.0.wrapping_add((a as u32) << 20))
        }
    }
    pub fn pieces_added_unchecked(self, a: i32) -> Self {
        if self.0 >= Self::PLY_MASK {
            Self::INF
        } else {
            Self(self.0.wrapping_sub((a as u32) << 12))
        }
    }
    pub fn futile_added_unchecked(self, a: i32) -> Self {
        if self.0 >= Self::PLY_MASK {
            Self::INF
        } else {
            Self(self.0.wrapping_sub(a as u32))
        }
    }
}

impl Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let plies = self.plies();
        let pieces = self.pieces();
        let futile = self.futile();
        write!(f, "MC={}, P={}, F={}", plies, pieces, futile)
    }
}

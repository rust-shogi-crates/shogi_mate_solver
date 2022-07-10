// 長井, 今井: df-pnアルゴリズムの詰将棋を解くプログラムへの応用.

use shogi_core::{Move, Square};

use crate::{
    position_wrapper::{Key, PositionWrapper},
    tt::DfPnTable,
};

#[derive(Clone, Copy)]
enum NodeKind {
    Or,
    And,
}

impl NodeKind {
    pub fn flip(self) -> Self {
        match self {
            NodeKind::Or => NodeKind::And,
            NodeKind::And => NodeKind::Or,
        }
    }
}

// ルートでの反復深化
pub fn df_pn(dfpn_tbl: &mut DfPnTable, position: &PositionWrapper) -> (u32, u32) {
    let (phi_now, delta_now) = mid(
        dfpn_tbl,
        position,
        (u32::MAX - 1, u32::MAX - 1),
        NodeKind::Or,
    );
    // ループを見つけてしまった
    if phi_now != u32::MAX && delta_now != u32::MAX {
        return mid(dfpn_tbl, position, (u32::MAX, u32::MAX), NodeKind::Or);
    }
    (phi_now, delta_now)
}

// ノードの展開
// (新しい phi(現在の局面), 新しい delta(現在の局面)) を返す。
fn mid(
    dfpn_tbl: &mut DfPnTable,
    position: &PositionWrapper,
    (mut phi_now, mut delta_now): (u32, u32),
    node_kind: NodeKind,
) -> (u32, u32) {
    let (phi, delta) = look_up_hash(dfpn_tbl, position.zobrist_hash());
    if phi_now <= phi || delta_now <= delta {
        return (phi, delta);
    }
    let moves = match node_kind {
        NodeKind::Or => position.all_checks(),
        NodeKind::And => position.all_evasions(),
    };
    if moves.is_empty() {
        put_in_hash(dfpn_tbl, position.zobrist_hash(), (u32::MAX, 0));
        return (u32::MAX, 0);
    }
    let mut children = vec![];
    for mv in moves {
        let mut cp = position.clone();
        cp.make_move(mv);
        children.push((mv, cp.zobrist_hash()));
    }
    // 3. ハッシュによるサイクル回避
    put_in_hash(dfpn_tbl, position.zobrist_hash(), (phi_now, delta_now));
    // 4. 多重反復深化
    loop {
        let delta_min = delta_min(dfpn_tbl, &children);
        let phi_sum = phi_sum(dfpn_tbl, &children);
        // φ か δ がそのしきい値以上なら探索終了
        if phi_now <= delta_min || delta_now <= phi_sum {
            phi_now = delta_min;
            delta_now = phi_sum;
            put_in_hash(dfpn_tbl, position.zobrist_hash(), (phi_now, delta_now));
            return (phi_now, delta_now);
        }
        let ((mv, _n_c, phi_c, delta_c), delta_2) = select_child(dfpn_tbl, &children);

        let phi_n_c = if phi_c == u32::MAX - 1 {
            u32::MAX
        } else if phi_now >= u32::MAX - 1 {
            u32::MAX - 1
        } else {
            delta_now + phi_c - phi_sum
        };
        let delta_n_c = if delta_c == u32::MAX - 1 {
            u32::MAX
        } else {
            core::cmp::min(phi_now, delta_2.saturating_add(1))
        };
        let mut next = position.clone();
        next.make_move(mv);
        mid(dfpn_tbl, &next, (phi_n_c, delta_n_c), node_kind.flip());
    }
}

// 子ノードの選択
// ((子ノードへ向かうための手, 子ノードのハッシュ, phi_c, delta_c), delta_2) を返す。
fn select_child(dfpn_tbl: &DfPnTable, children: &[(Move, Key)]) -> ((Move, Key, u32, u32), u32) {
    debug_assert!(!children.is_empty());
    let mut n_best = (
        Move::Normal {
            from: Square::SQ_1A,
            to: Square::SQ_1A,
            promote: false,
        },
        0,
    );
    let mut phi_c = u32::MAX;
    let mut delta_c = u32::MAX;
    let mut delta_2 = u32::MAX;
    for &(mv, hash) in children {
        let (phi, delta) = look_up_hash(dfpn_tbl, hash);
        if delta < delta_c {
            n_best = (mv, hash);
            delta_2 = delta_c;
            phi_c = phi;
            delta_c = delta;
        } else if delta < delta_2 {
            delta_2 = delta;
        }
    }
    ((n_best.0, n_best.1, phi_c, delta_c), delta_2)
}

// ハッシュを引く (本当は優越関係が使える)
fn look_up_hash(dfpn_tbl: &DfPnTable, position: Key) -> (u32, u32) {
    if let Some(x) = dfpn_tbl.fetch(position) {
        return x;
    }
    (1, 1)
}

// ハッシュに記録
fn put_in_hash(dfpn_tbl: &mut DfPnTable, position: Key, (phi, delta): (u32, u32)) {
    dfpn_tbl.insert(position, (phi, delta));
}

// n の子ノード の δ の最小を計算
fn delta_min(dfpn_tbl: &DfPnTable, children: &[(Move, Key)]) -> u32 {
    let mut mi = u32::MAX;
    for &child in children {
        let (_, delta) = look_up_hash(dfpn_tbl, child.1);
        mi = core::cmp::min(mi, delta);
    }
    mi
}

// nの子ノードのφの和を計算
fn phi_sum(dfpn_tbl: &DfPnTable, children: &[(Move, Key)]) -> u32 {
    let mut sum: u32 = 0;
    for &child in children {
        let (phi, _) = look_up_hash(dfpn_tbl, child.1);
        sum = sum.saturating_add(phi);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_core::PartialPosition;

    #[test]
    fn solve_mate_problem_works_0() {
        use shogi_usi_parser::FromUsi;

        // From https://github.com/koba-e964/shogi-mate-problems/blob/d58d61336dd82096856bc3ac0ba372e5cd722bc8/2022-05-18/mate5.psn#L3
        let position =
            PartialPosition::from_usi("sfen 3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1")
                .unwrap();
        let wrapped = PositionWrapper::new(position.clone());

        let mut dfpn_tbl = DfPnTable::new(1 << 15);
        let result = df_pn(&mut dfpn_tbl, &wrapped);
        // 詰み
        assert_eq!(result, (0, u32::MAX));

        // 初手 ▲51角成 だと △同玉 で詰まない。
        let moves = [
            Move::Normal {
                from: Square::SQ_2D,
                to: Square::SQ_5A,
                promote: true,
            },
            Move::Normal {
                from: Square::SQ_4A,
                to: Square::SQ_5A,
                promote: false,
            },
        ];
        let mut tmp = wrapped.clone();
        for mv in moves {
            tmp.make_move(mv);
        }
        let result = df_pn(&mut dfpn_tbl, &tmp);
        // 不詰
        assert_eq!(result, (u32::MAX, 0));

        // 初手 ▲52銀成 だと △同玉 で詰まない。
        let moves = [
            Move::Normal {
                from: Square::SQ_5C,
                to: Square::SQ_5B,
                promote: true,
            },
            Move::Normal {
                from: Square::SQ_4A,
                to: Square::SQ_5B,
                promote: false,
            },
        ];
        let mut tmp = wrapped.clone();
        for mv in moves {
            tmp.make_move(mv);
        }
        let result = df_pn(&mut dfpn_tbl, &tmp);
        // 不詰
        assert_eq!(result, (u32::MAX, 0));
    }
}

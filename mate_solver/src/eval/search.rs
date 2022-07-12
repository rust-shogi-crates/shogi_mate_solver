use shogi_core::{Hand, Move, PartialPosition, Piece, ToUsi};
use std::collections::BTreeSet;

use crate::{
    position_wrapper::{Key, PositionWrapper},
    tt::{DfPnTable, EvalTable},
};

use super::Value;

const LOG_THRESHOLD: usize = 3;

#[derive(Clone, Default)]
pub struct SearchCtx {
    seq: Vec<Move>,
}

impl SearchCtx {
    pub fn push(&mut self, mv: Move) {
        self.seq.push(mv);
    }
    pub fn pop(&mut self) {
        self.seq.pop();
    }
}

impl core::fmt::Debug for SearchCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        for &mv in &self.seq {
            write!(f, " {}", mv.to_usi_owned())?;
        }
        write!(f, "]")
    }
}

fn one_less(x: Value) -> Value {
    if x.plies() >= 1 {
        x.plies_added_unchecked(-1)
    } else {
        Value::ZERO
    }
}

// alpha-beta 法で探索する。
pub fn search(position: &PartialPosition, df_pn: &DfPnTable, evals: &mut EvalTable) -> Value {
    alpha_beta_me(
        &PositionWrapper::new(position.clone()),
        df_pn,
        evals,
        Value::ZERO,
        Value::new(12, 0, 0),
        &mut BTreeSet::new(),
        &mut Default::default(),
    )
    .0
}

// alpha-beta 法で攻め方の手を探索する。
pub fn alpha_beta_me(
    position: &PositionWrapper,
    df_pn: &DfPnTable,
    evals: &mut EvalTable,
    alpha: Value,
    mut beta: Value,
    seen: &mut BTreeSet<Key>,
    ctx: &mut SearchCtx,
) -> (Value, Option<Move>) {
    if beta.plies() == 0 {
        // 0 手で詰ますことはできない。攻め方にとって最悪の評価値を返す。
        return (Value::INF, None);
    }
    if let Some((pn, dn)) = df_pn.fetch(position.zobrist_hash()) {
        if (pn, dn) == (u32::MAX, 0) {
            // もう詰まないことが分かっている。攻め方にとって最悪の評価値を返す。
            return (Value::INF, None);
        }
    }
    if ctx.seq.len() <= LOG_THRESHOLD {
        eprintln!(
            "start: {:?} {:016x} {:?} {:?}",
            ctx,
            position.zobrist_hash(),
            alpha,
            beta,
        );
    }
    if let Some((memo_value, memo_move)) = evals.fetch(position.zobrist_hash()) {
        return (core::cmp::min(memo_value, beta), memo_move);
    }

    let mut all = position.all_checks();
    if all.is_empty() {
        evals.insert(position.zobrist_hash(), (Value::INF, None));
        return (Value::INF, None);
    }

    if seen.contains(&position.zobrist_hash()) {
        return (Value::INF, None);
    }
    seen.insert(position.zobrist_hash());

    // 詰みがありそうな局面から探索する。
    all.sort_unstable_by_key(|&mv| {
        let mut cp = position.clone();
        cp.make_move(mv);
        if let Some((_, delta)) = df_pn.fetch(cp.zobrist_hash()) {
            delta
        } else {
            1
        }
    });

    let mut best = None;
    for mv in all {
        let new_alpha = one_less(alpha);
        let new_beta = one_less(beta);
        let mut next = position.clone();
        next.make_move(mv);
        ctx.push(mv);
        let eval = alpha_beta_you(&next, df_pn, evals, new_alpha, new_beta, seen, ctx).0;
        ctx.pop();
        let eval = eval.plies_added_unchecked(1);
        if eval < beta {
            best = Some(mv);
            beta = eval
        }
        if alpha >= beta {
            seen.remove(&position.zobrist_hash());
            return (beta, best);
        }
    }
    if best.is_none() {
        evals.insert(position.zobrist_hash(), (Value::INF, best));
        seen.remove(&position.zobrist_hash());
        return (Value::INF, None);
    }
    evals.insert(position.zobrist_hash(), (beta, best));
    seen.remove(&position.zobrist_hash());
    if ctx.seq.len() <= LOG_THRESHOLD {
        eprintln!(
            "end  : {:?} {:016x} {:?} {}",
            ctx,
            position.zobrist_hash(),
            beta,
            best.map(|mv| mv.to_usi_owned())
                .unwrap_or_else(|| "none".to_owned()),
        );
    }
    (beta, best)
}

// alpha-beta 法で玉方の手を探索する。
pub fn alpha_beta_you(
    position: &PositionWrapper,
    df_pn: &DfPnTable,
    evals: &mut EvalTable,
    mut alpha: Value,
    beta: Value,
    seen: &mut BTreeSet<Key>,
    ctx: &mut SearchCtx,
) -> (Value, Option<Move>) {
    if let Some((dn, pn)) = df_pn.fetch(position.zobrist_hash()) {
        if (pn, dn) == (u32::MAX, 0) {
            // もう詰まないことが分かっている。攻め方にとって最悪の評価値を返す。
            return (Value::INF, None);
        }
    }
    if ctx.seq.len() <= LOG_THRESHOLD {
        eprintln!(
            "start: {:?} {:016x} {:?} {:?}",
            ctx,
            position.zobrist_hash(),
            alpha,
            beta,
        );
    }
    if alpha >= beta {
        return (alpha, None);
    }
    if let Some((memo_value, memo_move)) = evals.fetch(position.zobrist_hash()) {
        return (core::cmp::max(memo_value, alpha), memo_move);
    }
    let mut all = position.all_evasions();
    if all.is_empty() {
        let mut pieces = 0;
        for piece_kind in Hand::all_hand_pieces() {
            let inner = position.inner();
            pieces += inner
                .hand(Piece::new(piece_kind, inner.side_to_move().flip()))
                .unwrap();
        }
        let value = Value::new(0, pieces as u32, 0);
        evals.insert(position.zobrist_hash(), (value, None));
        if ctx.seq.len() <= LOG_THRESHOLD {
            eprintln!(
                "mate : {:?} {:016x} {:?}",
                ctx,
                position.zobrist_hash(),
                value,
            );
        }
        return (value, None);
    }

    if seen.contains(&position.zobrist_hash()) {
        return (Value::INF, None);
    }
    seen.insert(position.zobrist_hash());

    // 逃れがありそうな局面から探索する。
    all.sort_unstable_by_key(|&mv| {
        let mut cp = position.clone();
        cp.make_move(mv);
        if let Some((_, delta)) = df_pn.fetch(cp.zobrist_hash()) {
            delta
        } else {
            1
        }
    });

    let mut best = None;
    for &mv in &all {
        let new_alpha = one_less(alpha);
        let new_beta = one_less(beta);

        let mut next = position.clone();
        next.make_move(mv);
        ctx.push(mv);
        let eval = alpha_beta_me(&next, df_pn, evals, new_alpha, new_beta, seen, ctx).0;
        ctx.pop();
        let eval = eval.plies_added_unchecked(1);
        if eval > alpha {
            best = Some(mv);
            alpha = eval;
        }
        if alpha >= beta {
            seen.remove(&position.zobrist_hash());
            return (alpha, best);
        }
    }
    evals.insert(position.zobrist_hash(), (alpha, best));
    seen.remove(&position.zobrist_hash());
    if ctx.seq.len() <= LOG_THRESHOLD {
        eprintln!(
            "end  : {:?} {:016x} {:?} {}",
            ctx,
            position.zobrist_hash(),
            alpha,
            best.map(|mv| mv.to_usi_owned())
                .unwrap_or_else(|| "none".to_owned()),
        );
    }
    (alpha, best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_core::{Square, ToUsi};

    fn find_mate_sequence(
        df_pn: &DfPnTable,
        evals: &mut EvalTable,
        position: &PartialPosition,
        opt: Value,
    ) -> Vec<Move> {
        let mut turn = 0;
        let mut beta = opt.plies_added_unchecked(1);
        let mut position = PositionWrapper::new(position.clone());
        let mut result = Vec::new();
        let mut ctx = SearchCtx::default();
        loop {
            let (_value, mv) = if turn % 2 == 0 {
                alpha_beta_me(
                    &position,
                    df_pn,
                    evals,
                    Value::ZERO,
                    beta,
                    &mut BTreeSet::new(),
                    &mut ctx,
                )
            } else {
                alpha_beta_you(
                    &position,
                    df_pn,
                    evals,
                    Value::ZERO,
                    beta,
                    &mut BTreeSet::new(),
                    &mut ctx,
                )
            };
            if let Some(mv) = mv {
                result.push(mv);
                position.make_move(mv);
            } else {
                return result;
            }
            turn += 1;
            beta = beta.plies_added_unchecked(-1);
        }
    }

    #[test]
    fn solve_mate_problem_works_0() {
        use shogi_usi_parser::FromUsi;

        // From https://github.com/koba-e964/shogi-mate-problems/blob/d58d61336dd82096856bc3ac0ba372e5cd722bc8/2022-05-18/mate5.psn#L3
        let mut position =
            PartialPosition::from_usi("sfen 3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1")
                .unwrap();

        let mut df_pn = DfPnTable::new(1 << 15);
        let mut eval = EvalTable::new(1 << 20);

        let _mate_result =
            crate::df_pn::search::df_pn(&mut df_pn, &PositionWrapper::new(position.clone()));

        let result = search(&position, &df_pn, &mut eval);
        eprintln!("result = {:?}", result);
        let sequence = find_mate_sequence(&df_pn, &mut eval, &position, result);
        for &mv in &sequence {
            eprintln!("{}", mv.to_usi_owned());
            position.make_move(mv).unwrap();
        }
        assert_eq!(sequence.len(), 5);
    }

    #[test]
    fn solve_mate_problem_works_1() {
        use shogi_usi_parser::FromUsi;

        // From https://github.com/koba-e964/shogi-mate-problems/blob/d58d61336dd82096856bc3ac0ba372e5cd722bc8/2022-05-18/mate9.psn#L3
        let mut position =
            PartialPosition::from_usi("sfen 5kgnl/9/4+B1pp1/8p/9/9/9/9/9 b 2S2rb3g2s3n3l15p 1")
                .unwrap();

        let moves = [
            Move::Drop {
                piece: Piece::B_S,
                to: Square::SQ_5B,
            }, // 52銀
            Move::Normal {
                from: Square::SQ_4A,
                to: Square::SQ_3B,
                promote: false,
            }, // 32玉
        ];

        let mut df_pn = DfPnTable::new(1 << 15);
        let mut evals = EvalTable::new(1 << 15);

        let _mate_result =
            crate::df_pn::search::df_pn(&mut df_pn, &PositionWrapper::new(position.clone()));

        let result = search(&position, &df_pn, &mut evals);
        eprintln!("result = {:?}", result);
        let sequence = find_mate_sequence(&df_pn, &mut evals, &position, result);
        {
            let mut tmp = position.clone();
            for &mv in &sequence {
                eprintln!("{}", mv.to_usi_owned());
                tmp.make_move(mv).unwrap();
            }
        }
        assert_eq!(sequence.len(), 9);

        for &mv in &moves {
            position.make_move(mv).unwrap();
        }
        let result = search(&position, &df_pn, &mut evals);
        assert_eq!(result.plies(), 7);
    }

    #[test]
    fn solve_mate_problem_works_3() {
        use shogi_usi_parser::FromUsi;

        let mut position = PartialPosition::from_usi("sfen 7kl/9/6G1p/9/9/9/9/9/9 b S 1").unwrap();

        let mut df_pn = DfPnTable::new(1 << 15);
        let mut eval = EvalTable::new(1 << 15);

        let _mate_result =
            crate::df_pn::search::df_pn(&mut df_pn, &PositionWrapper::new(position.clone()));

        let result = search(&position, &df_pn, &mut eval);
        eprintln!("result = {:?}", result);
        let sequence = find_mate_sequence(&df_pn, &mut eval, &position, result);
        let expected = [
            "S*3b", // 32銀
            "2a1b", // 12玉
            "3c2c", // 23金
        ];
        for (index, &mv) in sequence.iter().enumerate() {
            assert_eq!(mv.to_usi_owned(), expected[index]);
            position.make_move(mv).unwrap();
        }
        assert_eq!(sequence.len(), 3);
    }
}

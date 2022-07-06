use shogi_core::{Hand, Move, PartialPosition, Piece, ToUsi};
use std::collections::BTreeSet;

use crate::{
    position_wrapper::{Key, PositionWrapper},
    tt::{DfPnTable, EvalTable},
};

use super::Value;

// alpha-beta 法で探索する。
pub fn search(position: &PartialPosition, df_pn: &DfPnTable, evals: &mut EvalTable) -> Value {
    alpha_beta_me(
        &PositionWrapper::new(position.clone()),
        df_pn,
        evals,
        Value::ZERO,
        Value::new(12, 0, 0),
        &mut BTreeSet::new(),
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
) -> (Value, Option<Move>) {
    if beta.plies() == 0 {
        // 0 手で詰ますことはできない。攻め方にとって最悪の評価値を返す。
        return (beta, None);
    }
    if let Some((memo_value, memo_move)) = evals.fetch(position.zobrist_hash()) {
        return (core::cmp::min(memo_value, beta), memo_move);
    }
    let new_alpha = if alpha.plies() >= 1 {
        alpha.plies_added_unchecked(-1)
    } else {
        Value::ZERO
    };
    let new_beta = if beta.plies() >= 1 {
        beta.plies_added_unchecked(-1)
    } else {
        Value::ZERO
    };
    let last = position.inner().last_move();
    eprintln!(
        "{} {:?} {:?}",
        if let Some(last) = last {
            last.to_usi_owned()
        } else {
            "".to_string()
        },
        alpha,
        beta
    );
    let all = position.all_checks();
    if all.is_empty() {
        evals.insert(position.zobrist_hash(), (Value::INF, None));
        return (Value::INF, None);
    }

    if seen.contains(&position.zobrist_hash()) {
        return (Value::INF, None);
    }
    seen.insert(position.zobrist_hash());

    let mut best = None;
    for mv in all {
        let mut next = position.clone();
        next.make_move(mv);
        let eval = alpha_beta_you(&next, df_pn, evals, new_alpha, new_beta, seen).0;
        let eval = eval.plies_added_unchecked(1);
        if eval <= beta {
            best = Some(mv);
            beta = eval
        }
        if alpha >= beta {
            seen.remove(&position.zobrist_hash());
            return (beta, best);
        }
    }
    evals.insert(position.zobrist_hash(), (beta, best));
    seen.remove(&position.zobrist_hash());
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
) -> (Value, Option<Move>) {
    if alpha >= beta {
        return (alpha, None);
    }
    if let Some((memo_value, memo_move)) = evals.fetch(position.zobrist_hash()) {
        return (core::cmp::max(memo_value, alpha), memo_move);
    }
    let new_alpha = if alpha.plies() >= 1 {
        alpha.plies_added_unchecked(-1)
    } else {
        Value::ZERO
    };
    let new_beta = if beta.plies() >= 1 {
        beta.plies_added_unchecked(-1)
    } else {
        Value::ZERO
    };
    let all = position.all_evasions();
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
        return (value, None);
    }
    let last = position.inner().last_move();
    eprintln!(
        "{} {:?} {:?}",
        if let Some(last) = last {
            last.to_usi_owned()
        } else {
            "".to_string()
        },
        alpha,
        beta
    );

    if seen.contains(&position.zobrist_hash()) {
        return (Value::INF, None);
    }
    seen.insert(position.zobrist_hash());

    let mut best = None;
    for &mv in &all {
        let mut next = position.clone();
        next.make_move(mv);
        let eval = alpha_beta_me(&next, df_pn, evals, new_alpha, new_beta, seen).0;
        let eval = eval.plies_added_unchecked(1);
        if eval >= alpha {
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
    (alpha, best)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        loop {
            let (_value, mv) = if turn % 2 == 0 {
                alpha_beta_me(
                    &position,
                    df_pn,
                    evals,
                    Value::ZERO,
                    beta,
                    &mut BTreeSet::new(),
                )
            } else {
                alpha_beta_you(
                    &position,
                    df_pn,
                    evals,
                    Value::ZERO,
                    beta,
                    &mut BTreeSet::new(),
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

        let df_pn = DfPnTable::new(1 << 15);
        let mut eval = EvalTable::new(1 << 20);
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

        let df_pn = DfPnTable::new(1 << 15);
        let mut eval = EvalTable::new(1 << 15);
        let result = search(&position, &df_pn, &mut eval);
        eprintln!("result = {:?}", result);
        let sequence = find_mate_sequence(&df_pn, &mut eval, &position, result);
        for &mv in &sequence {
            eprintln!("{}", mv.to_usi_owned());
            position.make_move(mv).unwrap();
        }
        assert_eq!(sequence.len(), 9);
    }

    #[test]
    fn solve_mate_problem_works_3() {
        use shogi_usi_parser::FromUsi;

        // From https://github.com/koba-e964/shogi-mate-problems/blob/d58d61336dd82096856bc3ac0ba372e5cd722bc8/2022-05-18/mate9.psn#L3
        let mut position = PartialPosition::from_usi("sfen 7kl/9/6G1p/9/9/9/9/9/9 b S 1").unwrap();

        let df_pn = DfPnTable::new(1 << 15);
        let mut eval = EvalTable::new(1 << 15);
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

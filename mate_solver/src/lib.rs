use std::collections::BTreeSet;

use df_pn::search as dfpnsearch;
use eval::{search as evalsearch, Value};
use shogi_core::{Move, PartialPosition};
use tt::{DfPnTable, EvalTable};

pub mod df_pn;
pub mod eval;
pub mod position_wrapper;
pub mod tt;

#[derive(Clone, Debug)]
pub struct Answer {
    pub inner: Result<OkType, ErrType>,
    pub elapsed: f64,
}

#[derive(Clone, Debug)]
pub struct OkType {
    pub resolution: Resolution,
    pub branches: Branches,
}

#[derive(Clone, Debug)]
pub struct ErrType {
    pub resolution: Resolution,
    pub reason: String,
}

#[derive(Clone, Copy, Debug)]
pub enum Resolution {
    Mate,
    NoMate,
    Unknown,
    Invalid,
}

pub type Branches = Vec<BranchEntry>;

#[derive(Clone, Debug)]
pub struct BranchEntry {
    pub moves: Vec<Move>,
    pub possible_next_moves: Vec<Move>,
    pub eval: Option<Eval>,
}

#[derive(Clone, Copy, Debug)]
pub struct Eval {
    pub num_moves: i32,
    pub pieces: i32,
    pub futile: i32,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub verbose: bool,
}

fn find_mate_sequence(
    df_pn: &mut DfPnTable,
    evals: &mut EvalTable,
    position: &PartialPosition,
    opt: Value,
    opts: &Options,
) -> Vec<Move> {
    let mut turn = 0;
    let mut beta = opt.plies_added_unchecked(1);
    let mut position = position_wrapper::PositionWrapper::new(position.clone());
    let mut result = Vec::new();
    loop {
        let mut ctx = evalsearch::SearchCtx::default();
        let (_value, mv) = if turn % 2 == 0 {
            evalsearch::alpha_beta_me(
                &position,
                df_pn,
                evals,
                Value::ZERO,
                beta,
                &mut BTreeSet::new(),
                &mut ctx,
                opts.verbose,
            )
        } else {
            evalsearch::alpha_beta_you(
                &position,
                df_pn,
                evals,
                Value::ZERO,
                beta,
                &mut BTreeSet::new(),
                &mut ctx,
                opts.verbose,
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

pub fn search(position: &PartialPosition, _timeout_ms: u64) -> Answer {
    // TODO: use wasm-timer
    let verbose = true;
    let size = 1 << 16;

    let mut df_pn = DfPnTable::new(size);

    let mut eval = EvalTable::new(size);
    let mate_result = dfpnsearch::df_pn(
        &mut df_pn,
        &position_wrapper::PositionWrapper::new(position.clone()),
        verbose,
    );
    // 不詰。
    if mate_result == (u32::MAX, 0) {
        return Answer {
            inner: Ok(OkType {
                resolution: Resolution::NoMate,
                branches: vec![],
            }),
            elapsed: 0.0,
        };
    }
    let result = evalsearch::search(position, &mut df_pn, &mut eval, verbose);
    if verbose {
        eprintln!("! result = {:?}", result);
    }
    if !result.is_mate() {
        return Answer {
            inner: Ok(OkType {
                resolution: Resolution::NoMate,
                branches: vec![],
            }),
            elapsed: 0.0,
        };
    }
    let sequence = find_mate_sequence(
        &mut df_pn,
        &mut eval,
        position,
        result,
        &Options { verbose },
    );
    let elapsed = 0.0;
    let mut branches = vec![];
    for i in 0..sequence.len() + 1 {
        let moves = sequence[0..i].to_vec();
        // TODO: enumerate all moves
        let possible_next_moves = if i == sequence.len() {
            vec![]
        } else {
            vec![sequence[i]]
        };
        branches.push(BranchEntry {
            moves,
            possible_next_moves,
            eval: Some(Eval {
                num_moves: (sequence.len() - i) as i32,
                pieces: 0,
                futile: 0,
            }),
        });
    }
    Answer {
        inner: Ok(OkType {
            resolution: Resolution::Mate,
            branches,
        }),
        elapsed,
    }
}

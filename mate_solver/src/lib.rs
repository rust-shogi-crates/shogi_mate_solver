use std::collections::{BTreeSet, HashMap};

use df_pn::search as dfpnsearch;
use eval::{search as evalsearch, Value};
use position_wrapper::PositionWrapper;
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

impl From<Value> for Eval {
    fn from(value: Value) -> Self {
        Self {
            num_moves: value.plies() as i32,
            pieces: value.pieces() as i32,
            futile: value.futile() as i32,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub verbose: bool,
}

// Returns true if the branch is worth recording.
fn find_branches(
    df_pn: &mut DfPnTable,
    evals: &mut EvalTable,
    position: &PositionWrapper,
    opt: Value,
    opts: &Options,
    memo: &mut HashMap<Vec<Move>, BranchEntry>,
    current: Vec<Move>,
) -> bool {
    let turn = current.len();
    if turn > opt.plies() as usize {
        return false;
    }
    if turn % 2 == 1 && dfpnsearch::df_pn(df_pn, position, opts.verbose) != (u32::MAX, 0) {
        return false;
    }
    let beta = opt.plies_added_unchecked(turn as i32);
    let mut ctx = evalsearch::SearchCtx::default();
    let (value, mv) = if turn % 2 == 0 {
        evalsearch::alpha_beta_me(
            position,
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
            position,
            df_pn,
            evals,
            Value::ZERO,
            beta,
            &mut BTreeSet::new(),
            &mut ctx,
            opts.verbose,
        )
    };
    let all_moves = if turn % 2 == 0 {
        let mv = if let Some(mv) = mv { mv } else { return false };
        vec![mv]
    } else {
        let mut tmp = position.all_evasions();
        if let Some(idx) = tmp.iter().position(|&cmv| Some(cmv) == mv) {
            tmp.remove(idx);
        }
        if let Some(mv) = mv {
            tmp.insert(0, mv);
        }
        tmp
    };
    let mut possible_next_moves = vec![];
    for mv in all_moves {
        let mut next = current.clone();
        next.push(mv);
        let mut next_position = position.clone();
        next_position.make_move(mv);
        if find_branches(df_pn, evals, &next_position, opt, opts, memo, next) {
            possible_next_moves.push(mv);
        }
    }
    let eval = Eval::from(value);
    let branch_entry = BranchEntry {
        moves: current.clone(),
        possible_next_moves,
        eval: Some(eval),
    };
    memo.insert(current, branch_entry);
    true
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
    let mut branches_hashmap = HashMap::new();
    find_branches(
        &mut df_pn,
        &mut eval,
        &position_wrapper::PositionWrapper::new(position.clone()),
        result,
        &Options { verbose },
        &mut branches_hashmap,
        vec![],
    );
    let elapsed = 0.0;
    let mut branches = vec![];
    for (_moves, branch_entry) in branches_hashmap.iter() {
        branches.push(branch_entry.clone());
    }
    Answer {
        inner: Ok(OkType {
            resolution: Resolution::Mate,
            branches,
        }),
        elapsed,
    }
}

use std::{
    collections::{BTreeSet, HashMap},
    time::Instant,
};

use df_pn::search as dfpnsearch;
use eval::{search as evalsearch, Value};
use position_wrapper::PositionWrapper;
use shogi_core::{Move, PartialPosition};
use tt::{DfPnTable, EvalTable};

pub mod df_pn;
pub mod eval;
pub mod features;
pub mod move_ordering;
pub mod nnue;
pub mod position_wrapper;
pub mod tt;

/// Optional limits and other controls for a search invocation.
///
/// On `wasm32`, a deadline is intentionally ignored because the standard
/// library cannot provide a reliable monotonic clock on every WASM host.
/// `max_positions` remains available there as a deterministic limit.
#[derive(Clone, Copy, Debug, Default)]
pub struct SearchConfig {
    deadline: Option<Instant>,
    max_positions: Option<u64>,
}

impl SearchConfig {
    pub fn with_deadline(deadline: Instant) -> Self {
        Self {
            deadline: Some(deadline),
            max_positions: None,
        }
    }

    pub fn with_max_positions(max_positions: u64) -> Self {
        Self {
            deadline: None,
            max_positions: Some(max_positions),
        }
    }

    pub fn with_deadline_and_max_positions(deadline: Instant, max_positions: u64) -> Self {
        Self {
            deadline: Some(deadline),
            max_positions: Some(max_positions),
        }
    }

    pub fn remaining_positions(self, positions_inspected: u64) -> Self {
        Self {
            deadline: self.deadline,
            max_positions: self
                .max_positions
                .map(|max_positions| max_positions.saturating_sub(positions_inspected)),
        }
    }

    pub(crate) fn limit_reached(self, positions_inspected: u64) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            // `std::time::Instant::now()` is not available on all WASM hosts.
            // Ignore an optional native deadline instead of producing a panic
            // or a false timeout and returning a bogus search result.
            let _ = self.deadline;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                return true;
            }
        }
        self.max_positions
            .is_some_and(|max| positions_inspected >= max)
    }
}

#[derive(Clone, Debug)]
pub struct Answer {
    pub inner: Result<OkType, ErrType>,
    pub stats: SearchStats,
    pub elapsed: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SearchStats {
    pub df_pn: DfPnStats,
    pub eval: EvalStats,
}

impl SearchStats {
    fn from_internal(df_pn: dfpnsearch::SearchStats, eval: evalsearch::SearchStats) -> SearchStats {
        SearchStats {
            df_pn: DfPnStats {
                positions_inspected: df_pn.positions_inspected,
            },
            eval: EvalStats {
                positions_inspected: eval.positions_inspected,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DfPnStats {
    pub positions_inspected: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EvalStats {
    pub positions_inspected: u64,
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
    let (value, mv) = if turn.is_multiple_of(2) {
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
    let all_moves = if turn.is_multiple_of(2) {
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
    let mut df_pn_stats = dfpnsearch::SearchStats::default();
    let mut eval_stats = evalsearch::SearchStats::default();
    let mate_result = dfpnsearch::df_pn_with_stats(
        &mut df_pn,
        &position_wrapper::PositionWrapper::new(position.clone()),
        verbose,
        &mut df_pn_stats,
    );
    // 不詰。
    if mate_result == (u32::MAX, 0) {
        return Answer {
            inner: Ok(OkType {
                resolution: Resolution::NoMate,
                branches: vec![],
            }),
            stats: SearchStats::from_internal(df_pn_stats, eval_stats),
            elapsed: 0.0,
        };
    }
    let result = evalsearch::search_with_stats(
        position,
        &mut df_pn,
        &mut eval,
        verbose,
        &mut eval_stats,
        &mut df_pn_stats,
    );
    if verbose {
        eprintln!("! result = {:?}", result);
    }
    if !result.is_mate() {
        return Answer {
            inner: Ok(OkType {
                resolution: Resolution::NoMate,
                branches: vec![],
            }),
            stats: SearchStats::from_internal(df_pn_stats, eval_stats),
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
    for branch_entry in branches_hashmap.values() {
        branches.push(branch_entry.clone());
    }
    Answer {
        inner: Ok(OkType {
            resolution: Resolution::Mate,
            branches,
        }),
        stats: SearchStats::from_internal(df_pn_stats, eval_stats),
        elapsed,
    }
}

use std::{
    collections::BTreeSet,
    env, fs,
    io::Write,
    io::{BufRead, BufReader, stdin},
    process::{self, Command, Stdio},
};

use mate_solver::SearchConfig;
use mate_solver::df_pn::search as dfpnsearch;
use mate_solver::eval::Value;
use mate_solver::eval::search as evalsearch;
use mate_solver::move_ordering::{
    FixtureScorer, MoveOrderingMode, MoveOrderingOptions, MoveOrderingScorer,
};
use mate_solver::nnue::NnueScorer;
use mate_solver::position_wrapper::PositionWrapper;
use mate_solver::tt::{DfPnTable, EvalTable};
use shogi_core::{Move, PartialPosition, Position, ToUsi};
use shogi_usi_parser::FromUsi;

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum Output {
    Text,
    Json,
}

enum MoveFormat {
    Usi,
    Kif,
    Csa,
    Official,
    Traditional,
}

struct Opts {
    verbose: bool,
    stats: bool,
    output: Output,
    move_format: MoveFormat,
    engine_path: Option<String>,
    move_ordering: MoveOrderingOptions,
    max_positions: Option<u64>,
    sfen: Option<String>,
}

#[derive(Default)]
struct PositionStats {
    df_pn: u64,
    eval: u64,
}

impl PositionStats {
    fn total(&self) -> u64 {
        self.df_pn.saturating_add(self.eval)
    }
}

fn parse_args() -> Result<Opts, String> {
    parse_args_from(env::args().skip(1))
}

fn parse_args_from(args: impl Iterator<Item = String>) -> Result<Opts, String> {
    let mut opts = Opts {
        verbose: false,
        stats: false,
        output: Output::Text,
        move_format: MoveFormat::Traditional,
        engine_path: None,
        move_ordering: MoveOrderingOptions::default(),
        max_positions: None,
        sfen: None,
    };
    let mut nnue_model_path = None;
    let mut move_ordering_mode = "current".to_owned();
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        if a == "--verbose" {
            opts.verbose = true;
        } else if a == "--stats" {
            opts.stats = true;
        } else if let Some(rest) = a.strip_prefix("--move-ordering=") {
            move_ordering_mode = rest.to_owned();
        } else if a == "--output=json" {
            opts.output = Output::Json;
        } else if let Some(rest) = a.strip_prefix("--move-format=") {
            opts.move_format = match rest {
                "kif" => MoveFormat::Kif,
                "usi" => MoveFormat::Usi,
                "csa" => MoveFormat::Csa,
                "official" => MoveFormat::Official,
                "traditional" => MoveFormat::Traditional,
                _ => return Err(format!("unknown move format: {rest}")),
            };
        } else if let Some(rest) = a.strip_prefix("--engine-path=") {
            opts.engine_path = Some(rest.to_owned());
        } else if let Some(rest) = a.strip_prefix("--nnue-model=") {
            nnue_model_path = Some(rest.to_owned());
        } else if let Some(rest) = a.strip_prefix("--max-positions=") {
            opts.max_positions = Some(
                rest.parse()
                    .map_err(|error| format!("invalid --max-positions: {error}"))?,
            );
        } else if let Some(rest) = a.strip_prefix("--sfen=") {
            opts.sfen = Some(rest.to_owned());
        } else if a == "--sfen" {
            opts.sfen = Some(
                args.next()
                    .ok_or_else(|| "--sfen requires a value".to_owned())?,
            );
        } else {
            return Err(format!("unknown argument: {a}"));
        }
    }
    opts.move_ordering = match move_ordering_mode.as_str() {
        "current" => MoveOrderingOptions::default(),
        "fixture" => MoveOrderingOptions {
            mode: MoveOrderingMode::FixtureScore,
            scorer: MoveOrderingScorer::Fixture(FixtureScorer::default()),
        },
        "nnue-fixture" => MoveOrderingOptions {
            mode: MoveOrderingMode::NnueFixture,
            scorer: MoveOrderingScorer::Nnue(NnueScorer::default()),
        },
        "nnue-model" => {
            let path = nnue_model_path
                .ok_or("--nnue-model is required with --move-ordering=nnue-model")?;
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("read NNUE model {path}: {error}"))?;
            let scorer = NnueScorer::from_model(&text)
                .map_err(|error| format!("parse NNUE model {path}: {error}"))?;
            MoveOrderingOptions {
                mode: MoveOrderingMode::NnueModel,
                scorer: MoveOrderingScorer::Nnue(scorer),
            }
        }
        other => return Err(format!("unknown move ordering mode: {other}")),
    };
    Ok(opts)
}

fn invoke_external_engine(
    position: &PartialPosition,
    exec_path: &str,
    opts: &Opts,
) -> Option<Vec<Move>> {
    let sfen = position.to_sfen_owned();
    let s = format!(
        "setoption name USI_Hash value 128
isready
usinewgame
position sfen {}
go
",
        sfen
    );

    let mut child = Command::new(exec_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    write!(stdin, "{}", s).unwrap();

    let mut scanner = BufReader::new(stdout);
    let mate_sequence;
    loop {
        let mut line = String::new();
        scanner.read_line(&mut line).unwrap();
        if opts.verbose {
            eprint!("> {}", line);
        }
        if let Some(rest) = line.strip_prefix("checkmate ") {
            mate_sequence = rest.trim().to_owned();
            break;
        }
    }
    if mate_sequence == "nomate" {
        child.wait().unwrap();
        return None;
    }
    // Get moves from mate_sequence
    let sfen_moves = "sfen ".to_string() + sfen.trim() + " moves " + &mate_sequence;
    let answer = Position::from_usi(&sfen_moves).unwrap();
    let moves = answer.moves();
    writeln!(stdin, "quit").unwrap();
    child.wait().unwrap();
    Some(moves.to_vec())
}

fn find_mate_sequence(
    df_pn: &mut DfPnTable,
    evals: &mut EvalTable,
    position: &PartialPosition,
    opt: Value,
    opts: &Opts,
    remaining_positions: &mut Option<u64>,
    position_stats: &mut PositionStats,
) -> Result<Vec<Move>, String> {
    let mut turn = 0;
    let mut beta = opt.plies_added_unchecked(1);
    let mut position = PositionWrapper::new(position.clone());
    let mut result = Vec::new();
    loop {
        let config = remaining_positions
            .as_ref()
            .copied()
            .map_or_else(SearchConfig::default, SearchConfig::with_max_positions);
        let mut ctx = evalsearch::SearchCtx::with_config(config);
        let mut eval_stats = evalsearch::SearchStats::default();
        let mut dfpn_stats = dfpnsearch::SearchStats::default();
        let (_value, mv) = if turn % 2 == 0 {
            evalsearch::alpha_beta_me_with_options_and_stats(
                &position,
                df_pn,
                evals,
                Value::ZERO,
                beta,
                &mut BTreeSet::new(),
                &mut ctx,
                opts.verbose,
                &mut eval_stats,
                &mut dfpn_stats,
                &opts.move_ordering,
            )
        } else {
            evalsearch::alpha_beta_you_with_options_and_stats(
                &position,
                df_pn,
                evals,
                Value::ZERO,
                beta,
                &mut BTreeSet::new(),
                &mut ctx,
                opts.verbose,
                &mut eval_stats,
                &mut dfpn_stats,
                &opts.move_ordering,
            )
        };
        position_stats.eval += eval_stats.positions_inspected;
        position_stats.df_pn += dfpn_stats.positions_inspected;
        if eval_stats.limit_reached || dfpn_stats.limit_reached {
            return Err("search position limit reached while building mate sequence".to_owned());
        }
        if let Some(remaining) = remaining_positions {
            *remaining = remaining
                .saturating_sub(eval_stats.positions_inspected + dfpn_stats.positions_inspected);
        }
        if let Some(mv) = mv {
            result.push(mv);
            position.make_move(mv);
        } else {
            return Ok(result);
        }
        turn += 1;
        beta = beta.plies_added_unchecked(-1);
    }
}

fn solve_myself(
    position: &PartialPosition,
    opts: &Opts,
    position_stats: &mut PositionStats,
) -> Result<Option<Vec<Move>>, String> {
    let size = 1 << 18;

    let mut df_pn = DfPnTable::new(size);

    let mut eval = EvalTable::new(size);
    let mut remaining_positions = opts.max_positions;
    let config =
        remaining_positions.map_or_else(SearchConfig::default, SearchConfig::with_max_positions);
    let mut dfpn_stats = dfpnsearch::SearchStats::default();
    let mate_result = dfpnsearch::df_pn_with_config_and_options_and_stats(
        &mut df_pn,
        &PositionWrapper::new(position.clone()),
        opts.verbose,
        &mut dfpn_stats,
        &opts.move_ordering,
        config,
    );
    position_stats.df_pn += dfpn_stats.positions_inspected;
    if dfpn_stats.limit_reached {
        return Err("search position limit reached during DF-PN".to_owned());
    }
    if let Some(remaining) = remaining_positions {
        remaining_positions = Some(remaining.saturating_sub(dfpn_stats.positions_inspected));
    }
    // 不詰。
    if mate_result == (u32::MAX, 0) {
        return Ok(None);
    }
    let wrapped = PositionWrapper::new(position.clone());
    let mut eval_stats = evalsearch::SearchStats::default();
    let mut eval_dfpn_stats = dfpnsearch::SearchStats::default();
    let eval_config =
        remaining_positions.map_or_else(SearchConfig::default, SearchConfig::with_max_positions);
    let (result, _) = evalsearch::alpha_beta_me_with_options_and_stats(
        &wrapped,
        &mut df_pn,
        &mut eval,
        Value::ZERO,
        Value::new(40, 0, 0),
        &mut BTreeSet::new(),
        &mut evalsearch::SearchCtx::with_config(eval_config),
        opts.verbose,
        &mut eval_stats,
        &mut eval_dfpn_stats,
        &opts.move_ordering,
    );
    position_stats.eval += eval_stats.positions_inspected;
    position_stats.df_pn += eval_dfpn_stats.positions_inspected;
    if eval_stats.limit_reached || eval_dfpn_stats.limit_reached {
        return Err("search position limit reached during evaluation".to_owned());
    }
    if let Some(remaining) = remaining_positions {
        remaining_positions =
            Some(remaining.saturating_sub(
                eval_stats.positions_inspected + eval_dfpn_stats.positions_inspected,
            ));
    }
    if opts.verbose {
        eprintln!("! result = {:?}", result);
    }
    if !result.is_mate() {
        return Ok(None);
    }
    let sequence = find_mate_sequence(
        &mut df_pn,
        &mut eval,
        position,
        result,
        opts,
        &mut remaining_positions,
        position_stats,
    )?;
    Ok(Some(sequence))
}

// Takes an SFEN string from --sfen or stdin, and solves the problem.
fn main() {
    let opts = match parse_args() {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("error: {message}");
            process::exit(2);
        }
    };
    let mut sfen = opts.sfen.clone().unwrap_or_default();
    if opts.sfen.is_none() {
        stdin().read_line(&mut sfen).unwrap();
    }
    if opts.verbose {
        eprintln!("! sfen = {}", sfen.trim());
    }
    let mut position = PartialPosition::from_usi(&("sfen ".to_string() + sfen.trim())).unwrap();
    let mut position_stats = PositionStats::default();
    let result = if let Some(ref exec_path) = opts.engine_path {
        if opts.max_positions.is_some() || opts.stats {
            eprintln!("error: --max-positions and --stats are not supported with --engine-path");
            process::exit(2);
        }
        Ok(invoke_external_engine(&position, exec_path, &opts))
    } else {
        solve_myself(&position, &opts, &mut position_stats)
    };
    if opts.stats {
        eprintln!(
            "stats: positions_inspected={} (df_pn={}, eval={})",
            position_stats.total(),
            position_stats.df_pn,
            position_stats.eval
        );
    }
    let moves = match result {
        Ok(moves) => moves,
        Err(message) => {
            eprintln!("error: {message}");
            process::exit(3);
        }
    };
    if let Some(moves) = moves {
        let mut first = true;
        if opts.output == Output::Json {
            print!("[");
        }
        for (index, &mv) in moves.iter().enumerate() {
            let move_str = match opts.move_format {
                MoveFormat::Usi => mv.to_usi_owned(),
                MoveFormat::Official => {
                    shogi_official_kifu::display_single_move(&position, mv).unwrap()
                }
                MoveFormat::Traditional => {
                    shogi_official_kifu::display_single_move_kansuji(&position, mv).unwrap()
                }
                _ => todo!(),
            };
            match opts.output {
                Output::Text => println!("{:2}: {}", index + 1, move_str),
                Output::Json => {
                    print!("{}{:?}", if first { "" } else { "," }, move_str);
                    first = false;
                }
            }
            position.make_move(mv).unwrap();
        }
        if opts.output == Output::Json {
            println!("]");
        }
    } else {
        println!("nomate");
    }
}

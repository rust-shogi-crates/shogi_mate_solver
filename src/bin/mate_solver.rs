use std::{
    collections::BTreeSet,
    env::args,
    io::Write,
    io::{stdin, BufRead, BufReader},
    process::{Command, Stdio},
};

use mate_solver::df_pn::search as dfpnsearch;
use mate_solver::eval::search as evalsearch;
use mate_solver::eval::Value;
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
    output: Output,
    move_format: MoveFormat,
    engine_path: Option<String>,
}

fn parse_args() -> Opts {
    let args: Vec<_> = args().collect();
    let mut opts = Opts {
        verbose: false,
        output: Output::Text,
        move_format: MoveFormat::Traditional,
        engine_path: None,
    };
    for a in args {
        if a == "--verbose" {
            opts.verbose = true;
        }
        if a == "--output=json" {
            opts.output = Output::Json;
        }
        if let Some(rest) = a.strip_prefix("--move-format=") {
            opts.move_format = match rest {
                "kif" => MoveFormat::Kif,
                "usi" => MoveFormat::Usi,
                "csa" => MoveFormat::Csa,
                "official" => MoveFormat::Official,
                "traditional" => MoveFormat::Traditional,
                _ => panic!(),
            };
        }
        if let Some(rest) = a.strip_prefix("--engine-path=") {
            opts.engine_path = Some(rest.to_owned());
        }
    }
    opts
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
) -> Vec<Move> {
    let mut turn = 0;
    let mut beta = opt.plies_added_unchecked(1);
    let mut position = PositionWrapper::new(position.clone());
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

fn solve_myself(position: &PartialPosition, opts: &Opts) -> Option<Vec<Move>> {
    let size = 1 << 16;

    let mut df_pn = DfPnTable::new(size);

    let mut eval = EvalTable::new(size);
    let mate_result = dfpnsearch::df_pn(
        &mut df_pn,
        &PositionWrapper::new(position.clone()),
        opts.verbose,
    );
    // 不詰。
    if mate_result == (u32::MAX, 0) {
        return None;
    }
    let result = evalsearch::search(position, &mut df_pn, &mut eval, opts.verbose);
    if opts.verbose {
        eprintln!("! result = {:?}", result);
    }
    if !result.is_mate() {
        return None;
    }
    let sequence = find_mate_sequence(&mut df_pn, &mut eval, position, result, opts);
    Some(sequence)
}

// Take an SFEN string from stdin, and solves the problem.
fn main() {
    let opts = parse_args();
    let mut sfen = String::new();
    stdin().read_line(&mut sfen).unwrap();
    if opts.verbose {
        eprintln!("! sfen = {}", sfen.trim());
    }
    let mut position = PartialPosition::from_usi(&("sfen ".to_string() + sfen.trim())).unwrap();
    let moves = if let Some(ref exec_path) = opts.engine_path {
        invoke_external_engine(&position, exec_path, &opts)
    } else {
        solve_myself(&position, &opts)
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

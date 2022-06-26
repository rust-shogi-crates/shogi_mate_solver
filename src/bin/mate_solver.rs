use std::{
    env::args,
    io::Write,
    io::{stdin, BufRead, BufReader},
    process::{Command, Stdio},
};

use shogi_core::{PartialPosition, Position};
use shogi_usi_parser::FromUsi;

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
}

fn parse_args() -> Opts {
    let args: Vec<_> = args().collect();
    let mut opts = Opts {
        verbose: false,
        output: Output::Text,
        move_format: MoveFormat::Traditional,
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
    }
    opts
}

// Take an SFEN string from stdin, and solves the problem.
fn main() {
    let opts = parse_args();
    let mut sfen = String::new();
    stdin().read_line(&mut sfen).unwrap();
    if opts.verbose {
        eprintln!("! sfen = {}", sfen);
    }
    let mut position = PartialPosition::from_usi(&("sfen ".to_string() + sfen.trim())).unwrap();
    let exec_path = "../YaneuraOu/source/YaneuraOu-by-gcc";
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
        println!("nomate");
        return;
    }
    let sfen_moves = "sfen ".to_string() + sfen.trim() + " moves " + &mate_sequence;
    let answer = Position::from_usi(&sfen_moves).unwrap();
    let moves = answer.moves();
    for (index, &mv) in moves.iter().enumerate() {
        match opts.move_format {
            MoveFormat::Official => println!(
                "{:2}: {}",
                index + 1,
                shogi_official_kifu::display_single_move(&position, mv).unwrap()
            ),
            MoveFormat::Traditional => println!(
                "{:2}: {}",
                index + 1,
                shogi_official_kifu::display_single_move_kansuji(&position, mv).unwrap()
            ),
            _ => todo!(),
        }
        position.make_move(mv).unwrap();
    }
    writeln!(stdin, "quit").unwrap();
}

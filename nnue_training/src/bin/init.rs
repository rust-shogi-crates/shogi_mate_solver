use std::{env, fs, process};

use mate_solver::nnue::parse::DeepModel;

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut output_path = None;
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--output=") {
            output_path = Some(value.to_owned());
        } else if arg == "--output" {
            output_path = Some(args.next().ok_or("missing value for --output")?);
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }
    let output_path = output_path.ok_or("missing --output")?;
    fs::write(&output_path, DeepModel::empty().to_text())
        .map_err(|error| format!("write {output_path}: {error}"))
}

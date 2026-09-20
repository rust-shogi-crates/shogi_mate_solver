use std::{env, fs, process};

const EMPTY_MODEL: &str = "NNUE-FIXTURE 1
hidden_units 2
hidden_bias 0 0
output_weights 2 1
output_bias 0
output_shift 7
";

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
    fs::write(&output_path, EMPTY_MODEL).map_err(|error| format!("write {output_path}: {error}"))
}

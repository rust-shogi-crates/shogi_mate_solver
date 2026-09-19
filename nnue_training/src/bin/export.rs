use std::{env, process};

fn main() {
    if let Err(message) = nnue_training::export_model(&mut env::args().skip(1)) {
        eprintln!("error: {message}");
        process::exit(1);
    }
}

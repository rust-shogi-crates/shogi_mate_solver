use std::{env, fs, process};

use mate_solver::{
    features::{FeatureRole, candidate_features},
    nnue::NnueScorer,
    position_wrapper::PositionWrapper,
};
use serde::{Deserialize, Serialize};
use shogi_core::{Move, PartialPosition};
use shogi_usi_parser::FromUsi;

#[derive(Serialize, Deserialize)]
struct TrainingExample {
    id: String,
    source_id: String,
    sfen: String,
    role: String,
    move_usi: String,
    label: u8,
    transform: String,
    ply_offset: usize,
}

#[derive(Serialize)]
struct ScoreRecord<'a> {
    id: &'a str,
    source_id: &'a str,
    sfen: &'a str,
    role: &'a str,
    move_usi: &'a str,
    label: u8,
    score: i32,
    transform: &'a str,
    ply_offset: usize,
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut model_path = None;
    let mut examples_path = None;
    let mut per_example = false;

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--model=") {
            model_path = Some(value.to_owned());
        } else if arg == "--model" {
            model_path = Some(next_arg(&mut args, "--model")?);
        } else if let Some(value) = arg.strip_prefix("--examples=") {
            examples_path = Some(value.to_owned());
        } else if arg == "--examples" {
            examples_path = Some(next_arg(&mut args, "--examples")?);
        } else if arg == "--per-example" {
            per_example = true;
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }

    let model_path = model_path.ok_or("missing --model")?;
    let examples_path = examples_path.ok_or("missing --examples")?;
    let model =
        fs::read_to_string(&model_path).map_err(|error| format!("read {model_path}: {error}"))?;
    let scorer = NnueScorer::from_model(&model)?;
    let mut count = 0usize;
    let mut positive = 0usize;
    for line in fs::read_to_string(&examples_path)
        .map_err(|error| format!("read {examples_path}: {error}"))?
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let example: TrainingExample =
            serde_json::from_str(line).map_err(|error| format!("parse example: {error}"))?;
        let position = PartialPosition::from_usi(&format!("sfen {}", example.sfen))
            .map_err(|error| format!("invalid SFEN for {}: {error:?}", example.id))?;
        let role = match example.role.as_str() {
            "attacker" => FeatureRole::Attacker,
            "defender" => FeatureRole::Defender,
            other => return Err(format!("unknown role: {other}")),
        };
        let mv = Move::from_usi(&example.move_usi)
            .map_err(|error| format!("invalid move for {}: {error:?}", example.id))?;
        let score = scorer.score(&candidate_features(
            &PositionWrapper::new(position),
            mv,
            role,
        ));
        if per_example {
            let record = ScoreRecord {
                id: &example.id,
                source_id: &example.source_id,
                sfen: &example.sfen,
                role: &example.role,
                move_usi: &example.move_usi,
                label: example.label,
                score,
                transform: &example.transform,
                ply_offset: example.ply_offset,
            };
            println!(
                "{}",
                serde_json::to_string(&record)
                    .map_err(|error| format!("serialize score: {error}"))?
            );
        }
        count += 1;
        positive += usize::from(score > 0);
    }
    if per_example {
        eprintln!("scored {count} examples; {positive} positive scores");
    } else {
        println!("scored {count} examples; {positive} positive scores");
    }
    Ok(())
}

fn next_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

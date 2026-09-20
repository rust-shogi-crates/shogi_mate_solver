use std::{env, fs, process};

use mate_solver::{
    features::{FeatureId, FeatureRole, candidate_features},
    nnue::parse::{DEEP_INPUTS, parse_deep_model, parse_model},
    position_wrapper::PositionWrapper,
};
use serde::{Deserialize, Serialize};
use shogi_core::{Move, PartialPosition};
use shogi_usi_parser::FromUsi;

const HIDDEN_WEIGHT_SCALE: i32 = 64;

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
    let mut output_path = None;
    let mut limit = None;

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--model=") {
            model_path = Some(value.to_owned());
        } else if arg == "--model" {
            model_path = Some(next_arg(&mut args, "--model")?);
        } else if let Some(value) = arg.strip_prefix("--examples=") {
            examples_path = Some(value.to_owned());
        } else if arg == "--examples" {
            examples_path = Some(next_arg(&mut args, "--examples")?);
        } else if let Some(value) = arg.strip_prefix("--output=") {
            output_path = Some(value.to_owned());
        } else if arg == "--output" {
            output_path = Some(next_arg(&mut args, "--output")?);
        } else if let Some(value) = arg.strip_prefix("--limit=") {
            limit = Some(
                value
                    .parse::<usize>()
                    .map_err(|error| format!("invalid --limit: {error}"))?,
            );
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }

    let model_path = model_path.ok_or("missing --model")?;
    let examples_path = examples_path.ok_or("missing --examples")?;
    let output_path = output_path.ok_or("missing --output")?;
    let model_text =
        fs::read_to_string(&model_path).map_err(|error| format!("read {model_path}: {error}"))?;
    let is_deep = model_text.lines().next() == Some("NNUE-FIXTURE 2");
    let mut model = (!is_deep).then(|| parse_model(&model_text)).transpose()?;
    let mut deep_model = is_deep.then(|| parse_deep_model(&model_text)).transpose()?;
    let mut example_count = 0usize;

    for line in fs::read_to_string(&examples_path)
        .map_err(|error| format!("read {examples_path}: {error}"))?
        .lines()
    {
        if line.trim().is_empty() {
            continue;
        }
        if limit.is_some_and(|value| example_count >= value) {
            break;
        }
        let example: TrainingExample =
            serde_json::from_str(line).map_err(|error| format!("parse example: {error}"))?;
        let position = PartialPosition::from_usi(&format!("sfen {}", example.sfen))
            .map_err(|error| format!("invalid SFEN for {}: {error:?}", example.id))?;
        let wrapped = PositionWrapper::new(position);
        let mv = Move::from_usi(&example.move_usi)
            .map_err(|error| format!("invalid move for {}: {error:?}", example.id))?;
        let role = match example.role.as_str() {
            "attacker" => FeatureRole::Attacker,
            "defender" => FeatureRole::Defender,
            other => return Err(format!("unknown role: {other}")),
        };
        let direction = if example.label == 1 { 1 } else { -1 };
        for FeatureId(feature) in candidate_features(&wrapped, mv, role) {
            if feature >= 30_000 {
                if let Some(model) = &mut model {
                    let weights = model.feature_weights.entry(feature).or_default();
                    weights[0] += direction * HIDDEN_WEIGHT_SCALE;
                    weights[1] += direction * (HIDDEN_WEIGHT_SCALE / 2);
                } else if let Some(model) = &mut deep_model {
                    let weights = model
                        .input_weights
                        .entry(feature)
                        .or_insert_with(|| vec![0; DEEP_INPUTS]);
                    let bucket = feature as usize % DEEP_INPUTS;
                    weights[bucket] += direction * HIDDEN_WEIGHT_SCALE;
                }
            }
        }
        example_count += 1;
    }

    let output = match (model, deep_model) {
        (Some(model), None) => model.to_text(),
        (None, Some(model)) => model.to_text(),
        _ => return Err("invalid model format".to_owned()),
    };
    fs::write(&output_path, output).map_err(|error| format!("write {output_path}: {error}"))
}

fn next_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

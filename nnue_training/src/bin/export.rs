use std::{collections::BTreeMap, env, fs, process};

use mate_solver::{
    features::{FeatureId, FeatureRole, candidate_features},
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
    let mut examples_path = None;
    let mut output_path = None;
    let mut limit = None;

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--examples=") {
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

    let examples_path = examples_path.ok_or("missing --examples")?;
    let output_path = output_path.ok_or("missing --output")?;
    let mut feature_scores = BTreeMap::<u32, i32>::new();
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
        // This first exporter is a positive-class prototype: features seen on
        // selected moves receive weight, while negative examples remain in
        // the dataset for later trainers to use.
        let direction = i32::from(example.label == 1);
        for FeatureId(feature) in candidate_features(&wrapped, mv, role) {
            if feature >= 30_000 {
                *feature_scores.entry(feature).or_default() += direction;
            }
        }
        example_count += 1;
    }

    let mut model = String::from("NNUE-FIXTURE 1\n");
    model.push_str("hidden_units 2\n");
    model.push_str("hidden_bias 0 0\n");
    model.push_str("output_weights 2 1\n");
    model.push_str("output_bias 0\n");
    model.push_str("output_shift 7\n");
    for (feature, score) in feature_scores {
        if score != 0 {
            model.push_str(&format!(
                "feature {feature} {} {}\n",
                score * HIDDEN_WEIGHT_SCALE,
                score * (HIDDEN_WEIGHT_SCALE / 2)
            ));
        }
    }
    fs::write(&output_path, model).map_err(|error| format!("write {output_path}: {error}"))
}

fn next_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

use std::{collections::BTreeMap, env, fs, process};

use mate_solver::{
    features::{FeatureId, FeatureRole, candidate_features},
    nnue::NnueScorer,
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
    NnueScorer::from_model(&model_text)?;
    let mut feature_weights = parse_feature_weights(&model_text)?;
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
                let weights = feature_weights.entry(feature).or_default();
                weights[0] += direction * HIDDEN_WEIGHT_SCALE;
                weights[1] += direction * (HIDDEN_WEIGHT_SCALE / 2);
            }
        }
        example_count += 1;
    }

    let mut model = String::new();
    for line in model_text.lines() {
        if !line.trim_start().starts_with("feature ") {
            model.push_str(line);
            model.push('\n');
        }
    }
    for (feature, [weight0, weight1]) in feature_weights {
        if weight0 != 0 || weight1 != 0 {
            model.push_str(&format!("feature {feature} {weight0} {weight1}\n"));
        }
    }
    fs::write(&output_path, model).map_err(|error| format!("write {output_path}: {error}"))
}

fn parse_feature_weights(model: &str) -> Result<BTreeMap<u32, [i32; 2]>, String> {
    let mut weights = BTreeMap::new();
    for (line_number, line) in model.lines().enumerate() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("feature") {
            continue;
        }
        let feature = fields
            .next()
            .ok_or_else(|| format!("missing feature at line {}", line_number + 1))?
            .parse::<u32>()
            .map_err(|error| format!("invalid feature at line {}: {error}", line_number + 1))?;
        let weight0 = fields
            .next()
            .ok_or_else(|| format!("missing feature weight at line {}", line_number + 1))?
            .parse::<i32>()
            .map_err(|error| {
                format!(
                    "invalid feature weight at line {}: {error}",
                    line_number + 1
                )
            })?;
        let weight1 = fields
            .next()
            .ok_or_else(|| format!("missing feature weight at line {}", line_number + 1))?
            .parse::<i32>()
            .map_err(|error| {
                format!(
                    "invalid feature weight at line {}: {error}",
                    line_number + 1
                )
            })?;
        if fields.next().is_some() {
            return Err(format!(
                "too many feature fields at line {}",
                line_number + 1
            ));
        }
        if weights.insert(feature, [weight0, weight1]).is_some() {
            return Err(format!("duplicate feature {feature}"));
        }
    }
    Ok(weights)
}

fn next_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

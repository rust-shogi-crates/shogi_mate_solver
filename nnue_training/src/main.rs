use std::{collections::BTreeMap, env, fs, process};

use mate_solver::{
    features::{FeatureId, FeatureRole, candidate_features},
    nnue::NnueScorer,
    position_wrapper::PositionWrapper,
};
use serde::{Deserialize, Serialize};
use shogi_core::{Move, PartialPosition, ToUsi};
use shogi_usi_parser::FromUsi;

const HIDDEN_WEIGHT_SCALE: i32 = 64;

#[derive(Deserialize)]
struct PositionRecord {
    id: Option<String>,
    sfen: String,
}

#[derive(Deserialize)]
struct SearchRecord {
    #[serde(rename = "type")]
    record_type: String,
    id: Option<String>,
    evaluator: Option<String>,
    root_chosen_move: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct TrainingExample {
    id: String,
    sfen: String,
    evaluator: String,
    role: String,
    move_usi: String,
    label: u8,
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("examples") => generate_examples(&mut args),
        Some("export") => export_model(&mut args),
        Some("score") => score_examples(&mut args),
        _ => {
            print_usage();
            Err("missing or unknown command".to_owned())
        }
    }
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!(
        "  nnue_training examples --positions <positions.jsonl> --results <results.jsonl> --output <examples.jsonl> [--evaluator=eval]"
    );
    eprintln!(
        "  nnue_training export --examples <examples.jsonl> --output <model.nnue> [--limit=<count>]"
    );
    eprintln!("  nnue_training score --model <model.nnue> --examples <examples.jsonl>");
}

fn generate_examples(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
    let mut positions_path = None;
    let mut results_path = None;
    let mut output_path = None;
    let mut evaluator = "eval".to_owned();

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--positions=") {
            positions_path = Some(value.to_owned());
        } else if arg == "--positions" {
            positions_path = Some(next_arg(args, "--positions")?);
        } else if let Some(value) = arg.strip_prefix("--results=") {
            results_path = Some(value.to_owned());
        } else if arg == "--results" {
            results_path = Some(next_arg(args, "--results")?);
        } else if let Some(value) = arg.strip_prefix("--output=") {
            output_path = Some(value.to_owned());
        } else if arg == "--output" {
            output_path = Some(next_arg(args, "--output")?);
        } else if let Some(value) = arg.strip_prefix("--evaluator=") {
            evaluator = value.to_owned();
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }

    let positions_path = positions_path.ok_or("missing --positions")?;
    let results_path = results_path.ok_or("missing --results")?;
    let output_path = output_path.ok_or("missing --output")?;
    let positions = read_positions(&positions_path)?;
    let results = read_results(&results_path, &evaluator)?;
    let mut output = String::new();

    for (line, position) in positions.into_iter().enumerate() {
        let id = position
            .id
            .unwrap_or_else(|| format!("{positions_path}:{}", line + 1));
        let Some(chosen_move) = results
            .get(&id)
            .and_then(|record| record.root_chosen_move.as_ref())
        else {
            continue;
        };
        let wrapped = PositionWrapper::new(
            PartialPosition::from_usi(&format!("sfen {}", position.sfen))
                .map_err(|error| format!("invalid SFEN for {id}: {error:?}"))?,
        );
        for mv in wrapped.all_checks() {
            let example = TrainingExample {
                id: id.clone(),
                sfen: position.sfen.clone(),
                evaluator: evaluator.clone(),
                role: "attacker".to_owned(),
                move_usi: mv.to_usi_owned(),
                label: u8::from(mv.to_usi_owned() == *chosen_move),
            };
            output.push_str(
                &serde_json::to_string(&example)
                    .map_err(|error| format!("serialize example: {error}"))?,
            );
            output.push('\n');
        }
    }

    fs::write(&output_path, output).map_err(|error| format!("write {output_path}: {error}"))
}

fn export_model(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
    let mut examples_path = None;
    let mut output_path = None;
    let mut limit = None;

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--examples=") {
            examples_path = Some(value.to_owned());
        } else if arg == "--examples" {
            examples_path = Some(next_arg(args, "--examples")?);
        } else if let Some(value) = arg.strip_prefix("--output=") {
            output_path = Some(value.to_owned());
        } else if arg == "--output" {
            output_path = Some(next_arg(args, "--output")?);
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
        // selected moves receive weight, while negative examples remain in the
        // dataset for later trainers to use.
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

fn score_examples(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
    let mut model_path = None;
    let mut examples_path = None;

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--model=") {
            model_path = Some(value.to_owned());
        } else if arg == "--model" {
            model_path = Some(next_arg(args, "--model")?);
        } else if let Some(value) = arg.strip_prefix("--examples=") {
            examples_path = Some(value.to_owned());
        } else if arg == "--examples" {
            examples_path = Some(next_arg(args, "--examples")?);
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
        count += 1;
        positive += usize::from(score > 0);
    }
    println!("scored {count} examples; {positive} positive scores");
    Ok(())
}

fn read_positions(path: &str) -> Result<Vec<PositionRecord>, String> {
    fs::read_to_string(path)
        .map_err(|error| format!("read {path}: {error}"))?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| format!("parse position: {error}")))
        .collect()
}

fn read_results(path: &str, evaluator: &str) -> Result<BTreeMap<String, SearchRecord>, String> {
    let mut results = BTreeMap::new();
    for line in fs::read_to_string(path)
        .map_err(|error| format!("read {path}: {error}"))?
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let record: SearchRecord =
            serde_json::from_str(line).map_err(|error| format!("parse result: {error}"))?;
        if record.record_type == "result"
            && record.evaluator.as_deref() == Some(evaluator)
            && let Some(id) = record.id.as_ref()
        {
            results.insert(id.clone(), record);
        }
    }
    Ok(results)
}

fn next_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

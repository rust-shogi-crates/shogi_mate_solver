use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, process,
};

use mate_solver::{
    eval::{Value, search as evalsearch},
    features::{FeatureId, FeatureRole, candidate_features},
    move_ordering::MoveOrderingOptions,
    nnue::NnueScorer,
    position_wrapper::PositionWrapper,
    tt::{DfPnTable, EvalTable},
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
    source_id: String,
    sfen: String,
    evaluator: String,
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
        "  nnue_training examples --positions <positions.jsonl> --results <results.jsonl> --output <examples.jsonl> [--evaluator=eval] [--mirror] [--plies=<count>]"
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
    let mut mirror = false;
    let mut plies = 0usize;

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
        } else if arg == "--mirror" {
            mirror = true;
        } else if let Some(value) = arg.strip_prefix("--plies=") {
            plies = value
                .parse()
                .map_err(|error| format!("invalid --plies: {error}"))?;
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
        let Some(source_chosen_move) = results
            .get(&id)
            .and_then(|record| record.root_chosen_move.as_ref())
        else {
            continue;
        };
        append_examples(
            &mut output,
            &id,
            &position.sfen,
            source_chosen_move,
            &evaluator,
            "identity",
            0,
        )?;
        if mirror {
            append_examples(
                &mut output,
                &id,
                &mirror_sfen(&position.sfen)?,
                &mirror_move(source_chosen_move)?,
                &evaluator,
                "mirror",
                0,
            )?;
        }
        if plies > 0 {
            let position = PartialPosition::from_usi(&format!("sfen {}", position.sfen))
                .map_err(|error| format!("invalid SFEN for {id}: {error:?}"))?;
            if let Some((sfen, chosen_move)) = replay_and_label(&position, plies) {
                append_examples(
                    &mut output,
                    &id,
                    &sfen,
                    &chosen_move,
                    &evaluator,
                    "replay",
                    plies,
                )?;
                if mirror {
                    append_examples(
                        &mut output,
                        &id,
                        &mirror_sfen(&sfen)?,
                        &mirror_move(&chosen_move)?,
                        &evaluator,
                        "replay+mirror",
                        plies,
                    )?;
                }
            }
        }
    }

    fs::write(&output_path, output).map_err(|error| format!("write {output_path}: {error}"))
}

fn append_examples(
    output: &mut String,
    source_id: &str,
    sfen: &str,
    chosen_move: &str,
    evaluator: &str,
    transform: &str,
    ply_offset: usize,
) -> Result<(), String> {
    let wrapped = PositionWrapper::new(
        PartialPosition::from_usi(&format!("sfen {sfen}"))
            .map_err(|error| format!("invalid SFEN for {source_id}: {error:?}"))?,
    );
    let id = format!("{source_id}::{transform}::ply{ply_offset}");
    for mv in wrapped.all_checks() {
        let move_usi = mv.to_usi_owned();
        let example = TrainingExample {
            id: id.clone(),
            source_id: source_id.to_owned(),
            sfen: sfen.to_owned(),
            evaluator: evaluator.to_owned(),
            role: "attacker".to_owned(),
            move_usi: move_usi.clone(),
            label: u8::from(move_usi == chosen_move),
            transform: transform.to_owned(),
            ply_offset,
        };
        output.push_str(
            &serde_json::to_string(&example)
                .map_err(|error| format!("serialize example: {error}"))?,
        );
        output.push('\n');
    }
    Ok(())
}

fn replay_and_label(position: &PartialPosition, plies: usize) -> Option<(String, String)> {
    let mut wrapped = PositionWrapper::new(position.clone());
    for ply in 0..plies {
        let mv = if ply % 2 == 0 {
            wrapped.all_checks().into_iter().next()?
        } else {
            wrapped.all_evasions().into_iter().next()?
        };
        wrapped.make_move(mv);
    }

    let mut df_pn = DfPnTable::new(1 << 16);
    let mut eval = EvalTable::new(1 << 16);
    let (_, best_move) = evalsearch::alpha_beta_me_with_options_and_stats(
        &wrapped,
        &mut df_pn,
        &mut eval,
        Value::ZERO,
        // Augmentation labels use a bounded search so replay cannot turn
        // example generation into an unbounded solver run.
        Value::new(6, 0, 0),
        &mut BTreeSet::new(),
        &mut Default::default(),
        false,
        &mut Default::default(),
        &mut Default::default(),
        &MoveOrderingOptions::default(),
    );
    Some((wrapped.inner().to_sfen_owned(), best_move?.to_usi_owned()))
}

fn mirror_sfen(sfen: &str) -> Result<String, String> {
    let mut fields = sfen.split_whitespace();
    let board = fields.next().ok_or("missing SFEN board")?;
    let side = fields.next().ok_or("missing SFEN side")?;
    let hand = fields.next().ok_or("missing SFEN hand")?;
    let ply = fields.next().ok_or("missing SFEN ply")?;
    let mirrored_board = board
        .split('/')
        .map(mirror_rank)
        .collect::<Result<Vec<_>, _>>()?
        .join("/");
    Ok(format!("{mirrored_board} {side} {hand} {ply}"))
}

fn mirror_rank(rank: &str) -> Result<String, String> {
    let mut cells = Vec::new();
    let mut chars = rank.chars();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            cells.extend(std::iter::repeat_n(None, ch.to_digit(10).unwrap() as usize));
        } else if ch == '+' {
            let piece = chars.next().ok_or("incomplete promoted piece")?;
            cells.push(Some(format!("+{piece}")));
        } else {
            cells.push(Some(ch.to_string()));
        }
    }
    if cells.len() != 9 {
        return Err(format!("SFEN rank has {} squares", cells.len()));
    }
    let mut mirrored = String::new();
    let mut empty = 0;
    for cell in cells.into_iter().rev() {
        match cell {
            Some(piece) => {
                if empty > 0 {
                    mirrored.push_str(&empty.to_string());
                    empty = 0;
                }
                mirrored.push_str(&piece);
            }
            None => empty += 1,
        }
    }
    if empty > 0 {
        mirrored.push_str(&empty.to_string());
    }
    Ok(mirrored)
}

fn mirror_move(move_usi: &str) -> Result<String, String> {
    if move_usi.len() < 4 {
        return Err(format!("invalid USI move: {move_usi}"));
    }
    if move_usi.as_bytes().get(1) == Some(&b'*') {
        return Ok(format!(
            "{}*{}",
            &move_usi[..1],
            mirror_square(&move_usi[2..4])?
        ));
    }
    let suffix = &move_usi[4..];
    Ok(format!(
        "{}{}{}{}{}",
        mirror_file(&move_usi[0..1])?,
        &move_usi[1..2],
        mirror_file(&move_usi[2..3])?,
        &move_usi[3..4],
        suffix
    ))
}

fn mirror_square(square: &str) -> Result<String, String> {
    if square.len() != 2 {
        return Err(format!("invalid USI square: {square}"));
    }
    Ok(format!("{}{}", mirror_file(&square[0..1])?, &square[1..2]))
}

fn mirror_file(file: &str) -> Result<String, String> {
    let digit = file
        .parse::<u8>()
        .map_err(|error| format!("invalid USI file {file}: {error}"))?;
    if !(1..=9).contains(&digit) {
        return Err(format!("invalid USI file {file}"));
    }
    Ok(char::from(b'0' + 10 - digit).to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_transforms_sfen_and_moves() {
        assert_eq!(mirror_move("G*5a").unwrap(), "G*5a");
        assert_eq!(mirror_move("2d4b+").unwrap(), "8d6b+");
        assert_eq!(
            mirror_sfen("3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1").unwrap(),
            "2sk1g3/2g6/4S4/1B7/9/9/9/9/9 b G2rbg2s4n4l18p 1"
        );
    }
}

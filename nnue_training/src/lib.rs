use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    time::{Duration, Instant},
};

use mate_solver::{
    df_pn::search as dfpnsearch,
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
const DFPN_LABEL_THRESHOLD: u32 = 1_024;

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
}

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

struct GenerationState {
    deadline: Instant,
    timed_out: bool,
}

impl GenerationState {
    fn new(timeout_ms: u64) -> Self {
        Self {
            deadline: Instant::now() + Duration::from_millis(timeout_ms),
            timed_out: false,
        }
    }

    fn check_timeout(&mut self) -> bool {
        if Instant::now() >= self.deadline {
            self.timed_out = true;
        }
        self.timed_out
    }
}

pub fn generate_examples(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
    let mut positions_path = None;
    let mut results_path = None;
    let mut output_path = None;
    let mut evaluator = "eval".to_owned();
    let mut mirror = false;
    let mut plies = 0usize;
    let mut timeout_ms = 60_000u64;

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
        } else if let Some(value) = arg.strip_prefix("--timeout-ms=") {
            timeout_ms = value
                .parse()
                .map_err(|error| format!("invalid --timeout-ms: {error}"))?;
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
    let mut state = GenerationState::new(timeout_ms);

    for (line, position) in positions.into_iter().enumerate() {
        if state.check_timeout() {
            break;
        }
        let id = position
            .id
            .unwrap_or_else(|| format!("{positions_path}:{}", line + 1));
        if !results.contains_key(&id) {
            continue;
        }
        append_examples(
            &mut output,
            &id,
            &position.sfen,
            &evaluator,
            "identity",
            0,
            &mut state,
        )?;
        if mirror {
            append_examples(
                &mut output,
                &id,
                &mirror_sfen(&position.sfen)?,
                &evaluator,
                "mirror",
                0,
                &mut state,
            )?;
        }
        if plies > 0 {
            let position = PartialPosition::from_usi(&format!("sfen {}", position.sfen))
                .map_err(|error| format!("invalid SFEN for {id}: {error:?}"))?;
            for ply_offset in 1..=plies {
                if state.check_timeout() {
                    break;
                }
                if let Some((sfen, _chosen_move)) = replay_and_label(&position, ply_offset) {
                    append_examples(
                        &mut output,
                        &id,
                        &sfen,
                        &evaluator,
                        "replay",
                        ply_offset,
                        &mut state,
                    )?;
                    if mirror {
                        append_examples(
                            &mut output,
                            &id,
                            &mirror_sfen(&sfen)?,
                            &evaluator,
                            "replay+mirror",
                            ply_offset,
                            &mut state,
                        )?;
                    }
                }
            }
        }
    }

    fs::write(&output_path, output).map_err(|error| format!("write {output_path}: {error}"))?;
    if state.timed_out {
        eprintln!("generation timed out after {timeout_ms} ms; wrote partial output");
    }
    Ok(())
}

fn append_examples(
    output: &mut String,
    source_id: &str,
    sfen: &str,
    evaluator: &str,
    transform: &str,
    ply_offset: usize,
    state: &mut GenerationState,
) -> Result<(), String> {
    let wrapped = PositionWrapper::new(
        PartialPosition::from_usi(&format!("sfen {sfen}"))
            .map_err(|error| format!("invalid SFEN for {source_id}: {error:?}"))?,
    );
    let id = format!("{source_id}::{transform}::ply{ply_offset}");
    let role = if ply_offset.is_multiple_of(2) {
        FeatureRole::Attacker
    } else {
        FeatureRole::Defender
    };
    let role_name = match role {
        FeatureRole::Attacker => "attacker",
        FeatureRole::Defender => "defender",
        _ => return Err("unsupported feature role".to_owned()),
    };
    let moves = match role {
        FeatureRole::Attacker => wrapped.all_checks(),
        FeatureRole::Defender => wrapped.all_evasions(),
        _ => return Err("unsupported feature role".to_owned()),
    };
    let mut df_pn = DfPnTable::new(1 << 16);
    let mut eval = EvalTable::new(1 << 16);
    for mv in moves {
        if state.check_timeout() {
            break;
        }
        let move_usi = mv.to_usi_owned();
        let label = u8::from(move_leads_to_mate(
            &wrapped, mv, role, evaluator, &mut df_pn, &mut eval,
        )?);
        let example = TrainingExample {
            id: id.clone(),
            source_id: source_id.to_owned(),
            sfen: sfen.to_owned(),
            role: role_name.to_owned(),
            move_usi: move_usi.clone(),
            label,
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

fn move_leads_to_mate(
    position: &PositionWrapper,
    mv: Move,
    role: FeatureRole,
    evaluator: &str,
    df_pn: &mut DfPnTable,
    eval: &mut EvalTable,
) -> Result<bool, String> {
    let mut child = position.clone();
    child.make_move(mv);
    match evaluator {
        "df_pn" => {
            let node_kind = match role {
                FeatureRole::Attacker => dfpnsearch::NodeKind::And,
                FeatureRole::Defender => dfpnsearch::NodeKind::Or,
                _ => return Err("unsupported feature role".to_owned()),
            };
            let result = dfpnsearch::mid_with_options_and_stats(
                df_pn,
                &child,
                (DFPN_LABEL_THRESHOLD, DFPN_LABEL_THRESHOLD),
                node_kind,
                true,
                &mut Default::default(),
                false,
                &mut Default::default(),
                &MoveOrderingOptions::default(),
            );
            Ok(result == (0, u32::MAX))
        }
        "eval" => {
            // Unproven positions are intentionally labeled non-mate. Keep
            // this bounded so generation-level timeouts remain effective.
            let (value, _) = match role {
                FeatureRole::Attacker => evalsearch::alpha_beta_you_with_options(
                    &child,
                    df_pn,
                    eval,
                    Value::ZERO,
                    Value::new(6, 0, 0),
                    &mut BTreeSet::new(),
                    &mut Default::default(),
                    false,
                    &MoveOrderingOptions::default(),
                ),
                FeatureRole::Defender => evalsearch::alpha_beta_me_with_options(
                    &child,
                    df_pn,
                    eval,
                    Value::ZERO,
                    Value::new(6, 0, 0),
                    &mut BTreeSet::new(),
                    &mut Default::default(),
                    false,
                    &MoveOrderingOptions::default(),
                ),
                _ => return Err("unsupported feature role".to_owned()),
            };
            Ok(value.is_mate())
        }
        other => Err(format!("unsupported evaluator: {other}")),
    }
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
    // Augmentation labels use a bounded search so replay cannot turn example
    // generation into an unbounded solver run. The root entry point must
    // match the side to move: odd offsets are defender positions.
    let search = if plies.is_multiple_of(2) {
        evalsearch::alpha_beta_me_with_options_and_stats
    } else {
        evalsearch::alpha_beta_you_with_options_and_stats
    };
    let (_, best_move) = search(
        &wrapped,
        &mut df_pn,
        &mut eval,
        Value::ZERO,
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

pub fn export_model(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
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

pub fn score_examples(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
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
    fn mirror_transforms_sfen() {
        assert_eq!(
            mirror_sfen("3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1").unwrap(),
            "2sk1g3/2g6/4S4/1B7/9/9/9/9/9 b G2rbg2s4n4l18p 1"
        );
    }

    #[test]
    fn defender_examples_use_legal_moves() {
        let mut output = String::new();
        let mut state = GenerationState::new(60_000);
        append_examples(
            &mut output,
            "source",
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL w - 2",
            "eval",
            "replay",
            1,
            &mut state,
        )
        .unwrap();
        let examples: Vec<TrainingExample> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(!examples.is_empty());
        assert!(examples.iter().all(|example| example.role == "defender"));
    }

    #[test]
    fn labels_are_based_on_mate_outcome() {
        let mut output = String::new();
        let mut state = GenerationState::new(60_000);
        append_examples(
            &mut output,
            "source",
            "3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1",
            "df_pn",
            "identity",
            0,
            &mut state,
        )
        .unwrap();
        let examples: Vec<TrainingExample> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(examples.iter().any(|example| example.label == 1));
        assert!(examples.iter().any(|example| example.label == 0));
    }
}

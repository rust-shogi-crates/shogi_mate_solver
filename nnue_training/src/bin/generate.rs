use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, process,
    time::{Duration, Instant},
};

use mate_solver::{
    SearchConfig,
    df_pn::search as dfpnsearch,
    eval::{Value, search as evalsearch},
    features::FeatureRole,
    move_ordering::MoveOrderingOptions,
    position_wrapper::PositionWrapper,
    tt::{DfPnTable, EvalTable},
};
use serde::{Deserialize, Serialize};
use shogi_core::{Move, PartialPosition, ToUsi};
use shogi_usi_parser::FromUsi;

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

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
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
            positions_path = Some(next_arg(&mut args, "--positions")?);
        } else if let Some(value) = arg.strip_prefix("--results=") {
            results_path = Some(value.to_owned());
        } else if arg == "--results" {
            results_path = Some(next_arg(&mut args, "--results")?);
        } else if let Some(value) = arg.strip_prefix("--output=") {
            output_path = Some(value.to_owned());
        } else if arg == "--output" {
            output_path = Some(next_arg(&mut args, "--output")?);
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
        let mut df_pn = DfPnTable::new(1 << 18);
        let mut eval = EvalTable::new(1 << 18);
        append_examples(
            &mut output,
            &id,
            &position.sfen,
            &evaluator,
            "identity",
            0,
            &mut state,
            &mut df_pn,
            &mut eval,
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
                &mut df_pn,
                &mut eval,
            )?;
        }
        if plies > 0 {
            let position = PartialPosition::from_usi(&format!("sfen {}", position.sfen))
                .map_err(|error| format!("invalid SFEN for {id}: {error:?}"))?;
            for ply_offset in 1..=plies {
                if state.check_timeout() {
                    break;
                }
                if let Some((sfen, _chosen_move)) =
                    replay_and_label(&position, ply_offset, &mut state, &mut df_pn, &mut eval)
                {
                    append_examples(
                        &mut output,
                        &id,
                        &sfen,
                        &evaluator,
                        "replay",
                        ply_offset,
                        &mut state,
                        &mut df_pn,
                        &mut eval,
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
                            &mut df_pn,
                            &mut eval,
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

#[allow(clippy::too_many_arguments)]
fn append_examples(
    output: &mut String,
    source_id: &str,
    sfen: &str,
    evaluator: &str,
    transform: &str,
    ply_offset: usize,
    state: &mut GenerationState,
    df_pn: &mut DfPnTable,
    eval: &mut EvalTable,
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
    for mv in moves {
        if state.check_timeout() {
            break;
        }
        let move_usi = mv.to_usi_owned();
        let label = match move_leads_to_mate(&wrapped, mv, role, evaluator, df_pn, eval, state)? {
            Some(label) => u8::from(label),
            None => break,
        };
        let example = TrainingExample {
            id: id.clone(),
            source_id: source_id.to_owned(),
            sfen: sfen.to_owned(),
            role: role_name.to_owned(),
            move_usi,
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
    state: &mut GenerationState,
) -> Result<Option<bool>, String> {
    if state.check_timeout() {
        state.timed_out = true;
        return Ok(None);
    }
    let mut child = position.clone();
    child.make_move(mv);
    if role == FeatureRole::Attacker && child.all_evasions().is_empty() {
        return Ok(Some(true));
    }
    match evaluator {
        "df_pn" => {
            let node_kind = match role {
                FeatureRole::Attacker => dfpnsearch::NodeKind::And,
                FeatureRole::Defender => dfpnsearch::NodeKind::Or,
                _ => return Err("unsupported feature role".to_owned()),
            };
            let config = SearchConfig::with_deadline(state.deadline);
            let mut stats = dfpnsearch::SearchStats::default();
            let result = dfpnsearch::mid_with_options_and_stats(
                df_pn,
                &child,
                (DFPN_LABEL_THRESHOLD, DFPN_LABEL_THRESHOLD),
                node_kind,
                true,
                &mut dfpnsearch::SearchCtx::with_config(config),
                false,
                &mut stats,
                &MoveOrderingOptions::default(),
            );
            if stats.timed_out || state.check_timeout() {
                state.timed_out = true;
                Ok(None)
            } else {
                Ok(Some(result == (0, u32::MAX)))
            }
        }
        "eval" => {
            // Unproven positions are intentionally labeled non-mate. Keep
            // this bounded so generation-level timeouts remain effective.
            let config = SearchConfig::with_deadline(state.deadline);
            let mut eval_stats = evalsearch::SearchStats::default();
            let mut dfpn_stats = dfpnsearch::SearchStats::default();
            let (value, _) = match role {
                FeatureRole::Attacker => evalsearch::alpha_beta_you_with_options_and_stats(
                    &child,
                    df_pn,
                    eval,
                    Value::ZERO,
                    Value::new(6, 0, 0),
                    &mut BTreeSet::new(),
                    &mut evalsearch::SearchCtx::with_config(config),
                    false,
                    &mut eval_stats,
                    &mut dfpn_stats,
                    &MoveOrderingOptions::default(),
                ),
                FeatureRole::Defender => evalsearch::alpha_beta_me_with_options_and_stats(
                    &child,
                    df_pn,
                    eval,
                    Value::ZERO,
                    Value::new(6, 0, 0),
                    &mut BTreeSet::new(),
                    &mut evalsearch::SearchCtx::with_config(config),
                    false,
                    &mut eval_stats,
                    &mut dfpn_stats,
                    &MoveOrderingOptions::default(),
                ),
                _ => return Err("unsupported feature role".to_owned()),
            };
            if eval_stats.timed_out || dfpn_stats.timed_out || state.check_timeout() {
                state.timed_out = true;
                Ok(None)
            } else {
                Ok(Some(value.is_mate()))
            }
        }
        other => Err(format!("unsupported evaluator: {other}")),
    }
}

fn replay_and_label(
    position: &PartialPosition,
    plies: usize,
    state: &mut GenerationState,
    df_pn: &mut DfPnTable,
    eval: &mut EvalTable,
) -> Option<(String, String)> {
    let mut wrapped = PositionWrapper::new(position.clone());
    for ply in 0..plies {
        let mv = if ply % 2 == 0 {
            wrapped.all_checks().into_iter().next()?
        } else {
            wrapped.all_evasions().into_iter().next()?
        };
        wrapped.make_move(mv);
    }

    let config = SearchConfig::with_deadline(state.deadline);
    let mut eval_stats = evalsearch::SearchStats::default();
    let mut dfpn_stats = dfpnsearch::SearchStats::default();
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
        df_pn,
        eval,
        Value::ZERO,
        Value::new(6, 0, 0),
        &mut BTreeSet::new(),
        &mut evalsearch::SearchCtx::with_config(config),
        false,
        &mut eval_stats,
        &mut dfpn_stats,
        &MoveOrderingOptions::default(),
    );
    if eval_stats.timed_out || dfpn_stats.timed_out || state.check_timeout() {
        state.timed_out = true;
        return None;
    }
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
        let mut df_pn = DfPnTable::new(1 << 16);
        let mut eval = EvalTable::new(1 << 16);
        append_examples(
            &mut output,
            "source",
            "lnsgkgsnl/1r5b1/ppppppppp/9/9/9/PPPPPPPPP/1B5R1/LNSGKGSNL w - 2",
            "eval",
            "replay",
            1,
            &mut state,
            &mut df_pn,
            &mut eval,
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
        let mut df_pn = DfPnTable::new(1 << 16);
        let mut eval = EvalTable::new(1 << 16);
        append_examples(
            &mut output,
            "source",
            "5kgnl/9/4+B1pp1/8p/9/9/9/9/9 b 2S2rb3g2s3n3l15p 1",
            "df_pn",
            "identity",
            0,
            &mut state,
            &mut df_pn,
            &mut eval,
        )
        .unwrap();
        let examples: Vec<TrainingExample> = output
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(examples.iter().any(|example| example.label == 1));
        assert!(examples.iter().any(|example| example.label == 0));
    }

    #[test]
    fn dfpn_labels_immediate_mate_as_positive() {
        let positions = ["sfen 8k/7R1/7G1/9/9/9/9/9/K8 b - 1"];
        let mut found_immediate_mate = false;
        for sfen in positions {
            let position = PartialPosition::from_usi(sfen).unwrap();
            let wrapped = PositionWrapper::new(position);
            for mv in wrapped.all_checks() {
                let mut child = wrapped.clone();
                child.make_move(mv);
                if child.all_evasions().is_empty() {
                    found_immediate_mate = true;
                    let mut df_pn = DfPnTable::new(1 << 16);
                    let mut eval = EvalTable::new(1 << 16);
                    let mut state = GenerationState::new(60_000);
                    assert_eq!(
                        move_leads_to_mate(
                            &wrapped,
                            mv,
                            FeatureRole::Attacker,
                            "df_pn",
                            &mut df_pn,
                            &mut eval,
                            &mut state,
                        )
                        .unwrap(),
                        Some(true)
                    );
                }
            }
        }
        assert!(found_immediate_mate);
    }
}

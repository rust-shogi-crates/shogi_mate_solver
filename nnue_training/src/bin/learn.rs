use std::{env, fs, process};

use mate_solver::{
    features::{FeatureRole, candidate_features},
    nnue::parse::{DEEP_HIDDEN_1, DEEP_HIDDEN_2, DEEP_INPUTS, DeepModel, parse_deep_model},
    position_wrapper::PositionWrapper,
};
use serde::{Deserialize, Serialize};
use shogi_core::{Move, PartialPosition};
use shogi_usi_parser::FromUsi;

const INPUT_LEARNING_RATE: f64 = 1.0;
const HIDDEN_LEARNING_RATE: f64 = 0.01;
const OUTPUT_LEARNING_RATE: f64 = 0.001;
const POSITIVE_LABEL_WEIGHT: f64 = 1_000.0;

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
    let mut epochs = None;

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
        } else if let Some(value) = arg.strip_prefix("--epochs=") {
            epochs = Some(parse_positive(value, "--epochs")?);
        } else if arg == "--epochs" {
            epochs = Some(parse_positive(
                &next_arg(&mut args, "--epochs")?,
                "--epochs",
            )?);
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }

    let model_path = model_path.ok_or("missing --model")?;
    let examples_path = examples_path.ok_or("missing --examples")?;
    let output_path = output_path.ok_or("missing --output")?;
    let model_text =
        fs::read_to_string(&model_path).map_err(|error| format!("read {model_path}: {error}"))?;
    let mut model = parse_deep_model(&model_text)?;
    let epochs = epochs.unwrap_or(10);
    let mut deep_examples = Vec::new();
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
        let features = candidate_features(&wrapped, mv, role);
        deep_examples.push((
            features.into_iter().map(|feature| feature.0).collect(),
            f64::from(example.label),
        ));
        example_count += 1;
    }

    train_deep_model(&mut model, &deep_examples, epochs);
    fs::write(&output_path, model.to_text())
        .map_err(|error| format!("write {output_path}: {error}"))
}

fn train_deep_model(model: &mut DeepModel, examples: &[(Vec<u32>, f64)], epochs: usize) {
    let mut input_weights = model
        .input_weights
        .iter()
        .map(|(&feature, weights)| {
            (
                feature,
                weights
                    .iter()
                    .map(|&weight| f64::from(weight))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for (features, _) in examples {
        for &feature in features {
            input_weights.entry(feature).or_insert_with(|| {
                let mut weights = vec![0.0; DEEP_INPUTS];
                weights[feature as usize % DEEP_INPUTS] = 1.0;
                weights
            });
        }
    }
    let mut layer1_weights = model
        .layer1_weights
        .iter()
        .map(|&weight| f64::from(weight))
        .collect::<Vec<_>>();
    let mut layer1_bias = model
        .layer1_bias
        .iter()
        .map(|&bias| f64::from(bias))
        .collect::<Vec<_>>();
    let mut layer2_weights = model
        .layer2_weights
        .iter()
        .map(|&weight| f64::from(weight))
        .collect::<Vec<_>>();
    let mut layer2_bias = model
        .layer2_bias
        .iter()
        .map(|&bias| f64::from(bias))
        .collect::<Vec<_>>();
    let mut output_weights = model
        .output_weights
        .iter()
        .map(|&weight| f64::from(weight))
        .collect::<Vec<_>>();
    let mut output_bias = f64::from(model.output_bias);

    for epoch in 1..=epochs {
        let mut epoch_loss = 0.0;
        for (features, label) in examples {
            let mut input = vec![0.0; DEEP_INPUTS];
            for &feature in features {
                if let Some(weights) = input_weights.get(&feature) {
                    for (value, weight) in input.iter_mut().zip(weights) {
                        *value += weight;
                    }
                }
            }
            let input_active = vec![true; DEEP_INPUTS];
            let input_relu = input.clone();
            let (z1, a1) = forward_layer(
                &input_relu,
                &layer1_weights,
                &layer1_bias,
                DEEP_INPUTS,
                DEEP_HIDDEN_1,
            );
            let (z2, a2) = forward_layer(
                &a1,
                &layer2_weights,
                &layer2_bias,
                DEEP_HIDDEN_1,
                DEEP_HIDDEN_2,
            );
            let output = output_bias
                + a2.iter()
                    .zip(&output_weights)
                    .map(|(value, weight)| value * weight)
                    .sum::<f64>();
            let prediction = sigmoid(output);
            let class_weight = if *label > 0.5 {
                POSITIVE_LABEL_WEIGHT
            } else {
                1.0
            };
            epoch_loss += class_weight
                * (-label * prediction.max(f64::MIN_POSITIVE).ln()
                    - (1.0 - label) * (1.0 - prediction).max(f64::MIN_POSITIVE).ln());
            let delta_output = (prediction - label) * class_weight;

            let output_weights_before = output_weights.clone();
            for (weight, value) in output_weights.iter_mut().zip(&a2) {
                *weight -= OUTPUT_LEARNING_RATE * delta_output * value;
            }
            output_bias -= OUTPUT_LEARNING_RATE * delta_output;

            let delta2 = z2
                .iter()
                .enumerate()
                .map(|(unit, &value)| {
                    delta_output
                        * output_weights_before[unit]
                        * if value >= 0.0 { 1.0 } else { 0.0 }
                })
                .collect::<Vec<_>>();
            let mut delta1 = vec![0.0; DEEP_HIDDEN_1];
            let layer2_weights_before = layer2_weights.clone();
            for (unit, delta) in delta2.iter().enumerate() {
                for input_unit in 0..DEEP_HIDDEN_1 {
                    layer2_weights[unit * DEEP_HIDDEN_1 + input_unit] -=
                        HIDDEN_LEARNING_RATE * delta * a1[input_unit];
                    delta1[input_unit] +=
                        delta * layer2_weights_before[unit * DEEP_HIDDEN_1 + input_unit];
                }
                layer2_bias[unit] -= HIDDEN_LEARNING_RATE * delta;
            }
            let delta1 = delta1
                .into_iter()
                .enumerate()
                .map(|(unit, delta)| delta * if z1[unit] >= 0.0 { 1.0 } else { 0.0 })
                .collect::<Vec<_>>();
            let mut delta_input = vec![0.0; DEEP_INPUTS];
            let layer1_weights_before = layer1_weights.clone();
            for (unit, delta) in delta1.iter().enumerate() {
                for input_unit in 0..DEEP_INPUTS {
                    layer1_weights[unit * DEEP_INPUTS + input_unit] -=
                        HIDDEN_LEARNING_RATE * delta * input_relu[input_unit];
                    delta_input[input_unit] +=
                        delta * layer1_weights_before[unit * DEEP_INPUTS + input_unit];
                }
                layer1_bias[unit] -= HIDDEN_LEARNING_RATE * delta;
            }
            for &feature in features {
                if let Some(weights) = input_weights.get_mut(&feature) {
                    for (input_unit, weight) in weights.iter_mut().enumerate() {
                        *weight -= INPUT_LEARNING_RATE
                            * delta_input[input_unit]
                            * if input_active[input_unit] { 1.0 } else { 0.0 };
                    }
                }
            }
        }
        let mean_loss = if examples.is_empty() {
            0.0
        } else {
            epoch_loss / examples.len() as f64
        };
        println!("epoch {epoch}/{epochs}: loss={mean_loss:.6}");
    }

    model.input_weights = input_weights
        .into_iter()
        .map(|(feature, weights)| (feature, weights.into_iter().map(round_weight).collect()))
        .collect();
    model.layer1_weights = layer1_weights.into_iter().map(round_weight).collect();
    model.layer1_bias = layer1_bias.into_iter().map(round_weight).collect();
    model.layer2_weights = layer2_weights.into_iter().map(round_weight).collect();
    model.layer2_bias = layer2_bias.into_iter().map(round_weight).collect();
    model.output_weights = output_weights.into_iter().map(round_weight).collect();
    model.output_bias = round_weight(output_bias);
}

fn forward_layer(
    input: &[f64],
    weights: &[f64],
    bias: &[f64],
    input_width: usize,
    output_width: usize,
) -> (Vec<f64>, Vec<f64>) {
    let z = (0..output_width)
        .map(|unit| {
            bias[unit]
                + input
                    .iter()
                    .enumerate()
                    .map(|(input_unit, value)| weights[unit * input_width + input_unit] * value)
                    .sum::<f64>()
        })
        .collect::<Vec<_>>();
    let activation = z.iter().map(|&value| value.max(0.0)).collect();
    (z, activation)
}

fn sigmoid(value: f64) -> f64 {
    if value >= 0.0 {
        1.0 / (1.0 + (-value).exp())
    } else {
        let exponent = value.exp();
        exponent / (1.0 + exponent)
    }
}

fn round_weight(value: f64) -> i32 {
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn next_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_positive(value: &str, flag: &str) -> Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|error| format!("invalid {flag}: {error}"))?;
    if value == 0 {
        return Err(format!("{flag} must be positive"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mate_solver::{features::FeatureId, nnue::NnueScorer};

    #[test]
    fn backpropagation_learns_a_separable_fixture() {
        let mut model = DeepModel::empty();
        train_deep_model(
            &mut model,
            &[([100].to_vec(), 1.0), ([200].to_vec(), 0.0)],
            10,
        );
        let scorer = NnueScorer::from_model(&model.to_text()).unwrap();
        assert_ne!(
            scorer.score(&[FeatureId(100)]),
            scorer.score(&[FeatureId(200)])
        );
    }
}

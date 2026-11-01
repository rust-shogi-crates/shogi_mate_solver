use std::{collections::BTreeMap, env, fs, io::Write, process};

use mate_solver::{
    features::{FeatureRole, position_features},
    nnue::parse::{DEEP_HIDDEN_1, DEEP_HIDDEN_2, DEEP_INPUTS, DeepModel, parse_deep_model},
    position_wrapper::PositionWrapper,
};
use serde::{Deserialize, Serialize};
use shogi_core::PartialPosition;
use shogi_usi_parser::FromUsi;

// Relative learning rates; --learning-rate applies a common multiplier.
const INPUT_LEARNING_RATE: f64 = 10.0;
const HIDDEN_LEARNING_RATE: f64 = 1.0;
const OUTPUT_LEARNING_RATE: f64 = 1.0;
// A small class-weight correction is enough; 1_000 made positive examples
// dominate the objective and drove every exported probability to saturation.
const POSITIVE_LABEL_WEIGHT: f64 = 2.0;
const EXPORT_SCALE: f64 = 32.0;

#[derive(Serialize, Deserialize)]
struct TrainingExample {
    id: String,
    source_id: String,
    sfen: String,
    role: String,
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
    let mut test_examples_path = None;
    let mut limit = None;
    let mut epochs = None;
    let mut checkpoint_dir = None;
    let mut learning_rate = 0.01;

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--checkpoint-dir=") {
            checkpoint_dir = Some(value.to_owned());
        } else if let Some(value) = arg.strip_prefix("--learning-rate=") {
            learning_rate = value.parse::<f64>().map_err(|e| e.to_string())?;
            if !learning_rate.is_finite() || learning_rate <= 0.0 {
                return Err("--learning-rate must be finite and positive".to_owned());
            }
        } else if let Some(value) = arg.strip_prefix("--model=") {
            model_path = Some(value.to_owned());
        } else if arg == "--model" {
            model_path = Some(next_arg(&mut args, "--model")?);
        } else if let Some(value) = arg.strip_prefix("--examples=") {
            examples_path = Some(value.to_owned());
        } else if arg == "--examples" {
            examples_path = Some(next_arg(&mut args, "--examples")?);
        } else if let Some(value) = arg.strip_prefix("--test-examples=") {
            test_examples_path = Some(value.to_owned());
        } else if arg == "--test-examples" {
            test_examples_path = Some(next_arg(&mut args, "--test-examples")?);
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
    let deep_examples = load_examples(&examples_path, limit)?;
    let test_examples = test_examples_path
        .as_deref()
        .map(|path| load_examples(path, limit))
        .transpose()?;

    if let Some(dir) = &checkpoint_dir {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    train_deep_model(
        &mut model,
        &deep_examples,
        test_examples.as_deref(),
        epochs,
        learning_rate,
        checkpoint_dir.as_deref(),
    )?;

    fs::write(&output_path, model.to_text())
        .map_err(|error| format!("write {output_path}: {error}"))
}

fn load_examples(path: &str, limit: Option<usize>) -> Result<Vec<(Vec<u32>, f64)>, String> {
    let mut deep_examples = Vec::new();
    for line in fs::read_to_string(path)
        .map_err(|error| format!("read {path}: {error}"))?
        .lines()
    {
        if line.trim().is_empty() {
            continue;
        }
        if limit.is_some_and(|value| deep_examples.len() >= value) {
            break;
        }
        let example: TrainingExample =
            serde_json::from_str(line).map_err(|error| format!("parse example: {error}"))?;
        if example.label > 1 {
            return Err(format!("invalid label for {}", example.id));
        }
        let position = PartialPosition::from_usi(&format!("sfen {}", example.sfen))
            .map_err(|error| format!("invalid SFEN for {}: {error:?}", example.id))?;
        let role = match example.role.as_str() {
            "attacker" => FeatureRole::Attacker,
            "defender" => FeatureRole::Defender,
            other => return Err(format!("unknown role: {other}")),
        };
        let features = position_features(&PositionWrapper::new(position), role);
        deep_examples.push((
            features.into_iter().map(|feature| feature.0).collect(),
            f64::from(example.label),
        ));
    }
    if deep_examples.is_empty() {
        return Err(format!("no examples in {path}"));
    }
    Ok(deep_examples)
}

fn train_deep_model(
    model: &mut DeepModel,
    examples: &[(Vec<u32>, f64)],
    test_examples: Option<&[(Vec<u32>, f64)]>,
    epochs: usize,
    learning_rate: f64,
    checkpoint_dir: Option<&str>,
) -> Result<(), String> {
    let logit_scale = f64::from(model.logit_scale);
    // ReLU is homogeneous: distribute the stored output shift across four matrices.
    let import_scale = 2.0_f64.powf(f64::from(model.output_shift) / 4.0);

    let mut input_weights = model
        .input_weights
        .iter()
        .map(|(&feature, weights)| {
            (
                feature,
                weights
                    .iter()
                    .map(|&weight| f64::from(weight) / import_scale)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for (features, _) in examples {
        for &feature in features {
            input_weights.entry(feature).or_insert_with(|| {
                let mut weights = vec![0.0; DEEP_INPUTS];
                weights[feature as usize % DEEP_INPUTS] = 0.1;
                weights
            });
        }
    }
    let mut layer1_weights = model
        .layer1_weights
        .iter()
        .map(|&weight| f64::from(weight) / import_scale)
        .collect::<Vec<_>>();
    let mut layer1_bias = model
        .layer1_bias
        .iter()
        .map(|&bias| f64::from(bias) / import_scale.powi(2))
        .collect::<Vec<_>>();
    let mut layer2_weights = model
        .layer2_weights
        .iter()
        .map(|&weight| f64::from(weight) / import_scale)
        .collect::<Vec<_>>();
    let mut layer2_bias = model
        .layer2_bias
        .iter()
        .map(|&bias| f64::from(bias) / import_scale.powi(3))
        .collect::<Vec<_>>();
    let mut output_weights = model
        .output_weights
        .iter()
        .map(|&weight| f64::from(weight) / import_scale)
        .collect::<Vec<_>>();
    let mut output_bias = f64::from(model.output_bias) / import_scale.powi(4);

    let mut order: Vec<_> = (0..examples.len()).collect();
    let mut random = 20261101_u64;
    for epoch in 1..=epochs {
        for i in (1..order.len()).rev() {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            order.swap(i, random as usize % (i + 1));
        }
        for &index in &order {
            let (features, label) = &examples[index];

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
            let unclipped_logit = output / logit_scale;

            let prediction = sigmoid(unclipped_logit);
            let class_weight = if *label > 0.5 {
                POSITIVE_LABEL_WEIGHT
            } else {
                1.0
            };
            let delta_output = (prediction - label) * class_weight / logit_scale;

            let output_weights_before = output_weights.clone();
            for (weight, value) in output_weights.iter_mut().zip(&a2) {
                *weight -= learning_rate * OUTPUT_LEARNING_RATE * delta_output * value;
            }
            // Optimize the bias in logit units, independent of the score scale.
            output_bias -=
                learning_rate * OUTPUT_LEARNING_RATE * delta_output * logit_scale.powi(2);

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
                        learning_rate * HIDDEN_LEARNING_RATE * delta * a1[input_unit];
                    delta1[input_unit] +=
                        delta * layer2_weights_before[unit * DEEP_HIDDEN_1 + input_unit];
                }
                layer2_bias[unit] -= learning_rate * HIDDEN_LEARNING_RATE * delta;
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
                        learning_rate * HIDDEN_LEARNING_RATE * delta * input_relu[input_unit];
                    delta_input[input_unit] +=
                        delta * layer1_weights_before[unit * DEEP_INPUTS + input_unit];
                }
                layer1_bias[unit] -= learning_rate * HIDDEN_LEARNING_RATE * delta;
            }
            for &feature in features {
                if let Some(weights) = input_weights.get_mut(&feature) {
                    for (input_unit, weight) in weights.iter_mut().enumerate() {
                        *weight -= learning_rate
                            * INPUT_LEARNING_RATE
                            * delta_input[input_unit]
                            * if input_active[input_unit] { 1.0 } else { 0.0 };
                    }
                }
            }
        }
        let floating_model = FloatingModel {
            input_weights: &input_weights,
            layer1_weights: &layer1_weights,
            layer1_bias: &layer1_bias,
            layer2_weights: &layer2_weights,
            layer2_bias: &layer2_bias,
            output_weights: &output_weights,
            output_bias,
            logit_scale,
        };
        *model = floating_model.export();
        if let Some(dir) = checkpoint_dir {
            let path = format!("{dir}/epoch{epoch:03}.nnue");
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .and_then(|mut file| file.write_all(model.to_text().as_bytes()))
                .map_err(|e| format!("write {path}: {e}"))?;
        }
        let train_loss = floating_model.mean_loss(examples);

        if let Some(test_examples) = test_examples {
            let test_loss = floating_model.mean_loss(test_examples);
            println!("epoch {epoch}/{epochs}: train_loss={train_loss:.6} test_loss={test_loss:.6}");
        } else {
            println!("epoch {epoch}/{epochs}: loss={train_loss:.6}");
        }
    }

    Ok(())
}

struct FloatingModel<'a> {
    input_weights: &'a BTreeMap<u32, Vec<f64>>,
    layer1_weights: &'a [f64],
    layer1_bias: &'a [f64],
    layer2_weights: &'a [f64],
    layer2_bias: &'a [f64],
    output_weights: &'a [f64],
    output_bias: f64,
    logit_scale: f64,
}

impl FloatingModel<'_> {
    fn export(&self) -> DeepModel {
        let quantize =
            |values: &[f64], scale: f64| values.iter().map(|v| round_weight(v * scale)).collect();
        DeepModel {
            input_weights: self
                .input_weights
                .iter()
                .map(|(&f, w)| (f, quantize(w, EXPORT_SCALE)))
                .collect(),
            layer1_weights: quantize(self.layer1_weights, EXPORT_SCALE),
            layer1_bias: quantize(self.layer1_bias, EXPORT_SCALE.powi(2)),
            layer2_weights: quantize(self.layer2_weights, EXPORT_SCALE),
            layer2_bias: quantize(self.layer2_bias, EXPORT_SCALE.powi(3)),
            output_weights: quantize(self.output_weights, EXPORT_SCALE),
            output_bias: round_weight(self.output_bias * EXPORT_SCALE.powi(4)),
            output_shift: 20,
            logit_scale: self.logit_scale as u32,
        }
    }

    fn mean_loss(&self, examples: &[(Vec<u32>, f64)]) -> f64 {
        if examples.is_empty() {
            return 0.0;
        }
        let total = examples
            .iter()
            .map(|(features, label)| {
                let mut input = vec![0.0; DEEP_INPUTS];
                for &feature in features {
                    if let Some(weights) = self.input_weights.get(&feature) {
                        for (value, weight) in input.iter_mut().zip(weights) {
                            *value += weight;
                        }
                    }
                }
                let (_, a1) = forward_layer(
                    &input,
                    self.layer1_weights,
                    self.layer1_bias,
                    DEEP_INPUTS,
                    DEEP_HIDDEN_1,
                );
                let (_, a2) = forward_layer(
                    &a1,
                    self.layer2_weights,
                    self.layer2_bias,
                    DEEP_HIDDEN_1,
                    DEEP_HIDDEN_2,
                );
                let output = self.output_bias
                    + a2.iter()
                        .zip(self.output_weights)
                        .map(|(value, weight)| value * weight)
                        .sum::<f64>();
                let logit = output / self.logit_scale;
                let class_weight = if *label > 0.5 {
                    POSITIVE_LABEL_WEIGHT
                } else {
                    1.0
                };
                class_weight * binary_loss(logit, *label)
            })
            .sum::<f64>();
        total / examples.len() as f64
    }
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

fn binary_loss(logit: f64, label: f64) -> f64 {
    logit.max(0.0) - label * logit + (-logit.abs()).exp().ln_1p()
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
    assert!(
        value.is_finite()
            && value.round() >= f64::from(i32::MIN)
            && value.round() <= f64::from(i32::MAX),
        "weight cannot be represented: {value}; reduce --learning-rate"
    );
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
    fn learns_opposite_labels_for_translated_positions() {
        let examples: Vec<_> = [
            ("8k/7G1/7K1/9/9/9/9/9/9 w - 1", 1.0),
            ("9/8k/7G1/7K1/9/9/9/9/9 w - 1", 0.0),
        ]
        .into_iter()
        .map(|(sfen, label)| {
            let position = PartialPosition::from_usi(&format!("sfen {sfen}")).unwrap();
            (
                position_features(&PositionWrapper::new(position), FeatureRole::Defender)
                    .into_iter()
                    .map(|f| f.0)
                    .collect(),
                label,
            )
        })
        .collect();
        let mut model = DeepModel::empty();
        train_deep_model(&mut model, &examples, None, 1500, 0.1, None).unwrap();
        let scorer = NnueScorer::from_model(&model.to_text()).unwrap();
        for (features, label) in examples {
            let features: Vec<_> = features.into_iter().map(FeatureId).collect();
            assert_eq!(scorer.score(&features) > 0, label == 1.0);
        }
    }

    #[test]
    fn saturated_wrong_model_can_recover() {
        let mut model = DeepModel::empty();
        model.output_bias = -2000;
        train_deep_model(&mut model, &[(vec![100], 1.0)], None, 1000, 0.01, None).unwrap();
        let scorer = NnueScorer::from_model(&model.to_text()).unwrap();
        assert!(scorer.score(&[FeatureId(100)]) > 0);
    }

    #[test]
    fn confidently_wrong_prediction_keeps_a_corrective_gradient() {
        for logit in [-100.0, 100.0] {
            let label = if logit < 0.0 { 1.0 } else { 0.0 };
            let gradient = sigmoid(logit) - label;
            assert!(gradient.abs() > 0.99);
            assert!(binary_loss(logit - gradient, label) < binary_loss(logit, label));
        }
    }

    #[test]
    fn fixed_point_export_preserves_fractional_weights() {
        let mut inputs = BTreeMap::new();
        inputs.insert(100, vec![0.25; DEEP_INPUTS]);
        let l1 = vec![0.125; DEEP_INPUTS * DEEP_HIDDEN_1];
        let b1 = vec![0.25; DEEP_HIDDEN_1];
        let l2 = vec![0.125; DEEP_HIDDEN_1 * DEEP_HIDDEN_2];
        let b2 = vec![0.25; DEEP_HIDDEN_2];
        let output = vec![0.125; DEEP_HIDDEN_2];
        let floating = FloatingModel {
            input_weights: &inputs,
            layer1_weights: &l1,
            layer1_bias: &b1,
            layer2_weights: &l2,
            layer2_bias: &b2,
            output_weights: &output,
            output_bias: 0.25,
            logit_scale: 127.0,
        };
        let mut model = floating.export();
        let scorer = NnueScorer::from_model(&model.to_text()).unwrap();
        assert_eq!(scorer.score(&[FeatureId(100)]), 261);
        assert!(scorer.score(&[FeatureId(100)]) > scorer.score(&[]));
        let before = model.to_text();
        train_deep_model(&mut model, &[(vec![100], 1.0)], None, 1, 1e-30, None).unwrap();
        assert_eq!(model.to_text(), before, "reloading must honor output_shift");
    }

    #[test]
    fn backpropagation_learns_a_separable_fixture() {
        let mut model = DeepModel::empty();
        train_deep_model(
            &mut model,
            &[([100].to_vec(), 1.0), ([200].to_vec(), 0.0)],
            None,
            // The model is exported with integer weights, so use enough steps
            // for the separable fixture to produce a visible score gap.
            1_000,
            1.0,
            None,
        )
        .unwrap();
        let scorer = NnueScorer::from_model(&model.to_text()).unwrap();
        assert_ne!(
            scorer.score(&[FeatureId(100)]),
            scorer.score(&[FeatureId(200)])
        );
    }
}

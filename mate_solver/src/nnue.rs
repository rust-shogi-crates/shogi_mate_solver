use crate::features::FeatureId;

pub mod parse;

const HIDDEN_UNITS: usize = 2;
const MAX_FEATURE_WEIGHTS: usize = 256;
pub const PROBABILITY_SCALE: u32 = 1_000_000;
const EMPTY_FEATURE_WEIGHT: FeatureWeight = FeatureWeight {
    feature: FeatureId(0),
    weights: [0; HIDDEN_UNITS],
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FeatureWeight {
    feature: FeatureId,
    weights: [i32; HIDDEN_UNITS],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NnueScorer {
    deep_model: Option<parse::DeepModel>,
    hidden_bias: [i32; HIDDEN_UNITS],
    feature_weights: Box<[FeatureWeight; MAX_FEATURE_WEIGHTS]>,
    feature_count: usize,
    output_weights: [i32; HIDDEN_UNITS],
    output_bias: i32,
    output_shift: u32,
}

impl Default for NnueScorer {
    fn default() -> Self {
        let mut scorer = Self::empty();
        // The fixture intentionally gives promoted moves a positive signal.
        scorer.add_feature(FeatureId(30_300), [64, 32]);
        scorer.add_feature(FeatureId(0), [128, -128]);
        scorer.add_feature(FeatureId(1), [-128, 128]);
        // FeatureId(30_001) is DROP_MOVE_KIND; the fixture favors drops.
        scorer.add_feature(FeatureId(30_001), [64, 32]);
        scorer
    }
}

impl NnueScorer {
    fn empty() -> Self {
        Self {
            deep_model: None,
            hidden_bias: [0, 0],
            feature_weights: Box::new([EMPTY_FEATURE_WEIGHT; MAX_FEATURE_WEIGHTS]),
            feature_count: 0,
            output_weights: [2, 1],
            output_bias: 0,
            output_shift: 7,
        }
    }

    fn add_feature(&mut self, feature: FeatureId, weights: [i32; HIDDEN_UNITS]) {
        self.feature_weights[self.feature_count] = FeatureWeight { feature, weights };
        self.feature_count += 1;
    }

    /// Loads the versioned text format emitted by `nnue_training learn`.
    pub fn from_model(text: &str) -> Result<Self, String> {
        Ok(Self {
            deep_model: Some(parse::parse_deep_model(text)?),
            ..Self::empty()
        })
    }

    pub fn score(&self, features: &[FeatureId]) -> i32 {
        if let Some(model) = &self.deep_model {
            return score_deep(model, features);
        }
        let mut hidden = self.hidden_bias;
        for &feature in features {
            if let Some(weight) = self.feature_weights[..self.feature_count]
                .iter()
                .find(|weight| weight.feature == feature)
            {
                for (value, weight) in hidden.iter_mut().zip(weight.weights) {
                    *value += weight;
                }
            }
        }

        let output = hidden
            .into_iter()
            .zip(self.output_weights)
            .map(|(value, weight)| value.max(0) * weight)
            .sum::<i32>()
            + self.output_bias;
        output >> self.output_shift
    }

    /// Returns sigmoid(score) scaled to the integer range 0..=1_000_000.
    pub fn probability(&self, features: &[FeatureId]) -> u32 {
        let logit_scale = self
            .deep_model
            .as_ref()
            .map_or(parse::DEFAULT_LOGIT_SCALE, |model| model.logit_scale);
        let logit = f64::from(self.score(features)) / f64::from(logit_scale);
        let probability = if logit >= 0.0 {
            1.0 / (1.0 + (-logit).exp())
        } else {
            let exp = logit.exp();
            exp / (1.0 + exp)
        };
        (probability * f64::from(PROBABILITY_SCALE)).round() as u32
    }
}

fn score_deep(model: &parse::DeepModel, features: &[FeatureId]) -> i32 {
    let mut input = vec![0i64; parse::DEEP_INPUTS];
    for &feature in features {
        if let Some(weights) = model.input_weights.get(&feature.0) {
            for (value, weight) in input.iter_mut().zip(weights) {
                *value += i64::from(*weight);
            }
        }
    }
    let hidden1 = dense_relu(
        &input,
        &model.layer1_weights,
        &model.layer1_bias,
        parse::DEEP_INPUTS,
    );
    let hidden2 = dense_relu(
        &hidden1,
        &model.layer2_weights,
        &model.layer2_bias,
        parse::DEEP_HIDDEN_1,
    );
    let output = i64::from(model.output_bias)
        + hidden2
            .iter()
            .zip(&model.output_weights)
            .map(|(value, weight)| value * i64::from(*weight))
            .sum::<i64>();
    clamp_i64_to_i32(output >> model.output_shift)
}

fn dense_relu(input: &[i64], weights: &[i32], bias: &[i32], input_width: usize) -> Vec<i64> {
    bias.iter()
        .enumerate()
        .map(|(row, bias)| {
            let start = row * input_width;
            let sum = i64::from(*bias)
                + input
                    .iter()
                    .zip(&weights[start..start + input_width])
                    .map(|(value, weight)| value * i64::from(*weight))
                    .sum::<i64>();
            sum.max(0)
        })
        .collect()
}

fn clamp_i64_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_inference_is_deterministic() {
        let scorer = NnueScorer::default();
        let features = [FeatureId(0), FeatureId(30_300)];

        assert_eq!(scorer.score(&features), scorer.score(&features));
    }

    #[test]
    fn fixture_inference_rewards_promotion_feature() {
        let scorer = NnueScorer::default();

        assert!(scorer.score(&[FeatureId(30_300)]) > scorer.score(&[]));
    }

    #[test]
    fn fixture_inference_rewards_drop_feature() {
        let scorer = NnueScorer::default();

        assert!(scorer.score(&[FeatureId(30_001)]) > scorer.score(&[]));
    }

    #[test]
    fn learned_model_round_trips_into_runtime() {
        let model = parse::DeepModel::empty().to_text();
        let scorer = NnueScorer::from_model(&model).unwrap();

        assert_eq!(scorer.score(&[FeatureId(30_300)]), 64);
    }

    #[test]
    fn model_requires_all_runtime_fields() {
        let model = "NNUE-FIXTURE 1\nhidden_units 512 32 32\n";

        assert_eq!(
            NnueScorer::from_model(model),
            Err("model is missing required fields".to_owned())
        );
    }

    #[test]
    fn model_rejects_unsafe_output_shift() {
        let mut model = parse::DeepModel::empty().to_text();
        model = model.replace("output_shift 0", "output_shift 32");

        assert_eq!(
            NnueScorer::from_model(&model),
            Err("output_shift must be less than 32".to_owned())
        );
    }

    #[test]
    fn fixture_inference_uses_role_features() {
        let scorer = NnueScorer::default();

        assert_ne!(scorer.score(&[FeatureId(0)]), scorer.score(&[FeatureId(1)]));
    }

    #[test]
    fn probability_is_monotonic_and_scaled() {
        let scorer = NnueScorer::default();

        assert_eq!(scorer.probability(&[]), 500_000);
        assert!(scorer.probability(&[FeatureId(30_300)]) > scorer.probability(&[]));
        assert!(scorer.probability(&[FeatureId(30_300)]) <= PROBABILITY_SCALE);
    }

    #[test]
    fn deep_model_round_trips_into_runtime() {
        let mut model = parse::DeepModel::empty();
        model
            .input_weights
            .insert(30_300, vec![1; parse::DEEP_INPUTS]);
        let scorer = NnueScorer::from_model(&model.to_text()).unwrap();

        assert!(scorer.score(&[FeatureId(30_300)]) > scorer.score(&[]));
    }
}

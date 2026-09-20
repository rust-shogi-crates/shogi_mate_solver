use crate::features::FeatureId;

pub mod parse;

const HIDDEN_UNITS: usize = 2;
const MAX_FEATURE_WEIGHTS: usize = 256;
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
        if text.lines().next() == Some("NNUE-FIXTURE 2") {
            return Ok(Self {
                deep_model: Some(parse::parse_deep_model(text)?),
                ..Self::empty()
            });
        }
        let model = parse::parse_model(text)?;
        let mut scorer = Self::empty();
        scorer.hidden_bias = model.hidden_bias;
        scorer.output_weights = model.output_weights;
        scorer.output_bias = model.output_bias;
        scorer.output_shift = model.output_shift;
        for (feature, weights) in model.feature_weights {
            scorer.add_feature(FeatureId(feature), weights);
        }
        Ok(scorer)
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
        let model = "NNUE-FIXTURE 1\nhidden_units 2\nhidden_bias 0 0\noutput_weights 2 1\noutput_bias 0\noutput_shift 7\nfeature 30300 64 32\n";
        let scorer = NnueScorer::from_model(model).unwrap();

        assert_eq!(scorer.score(&[FeatureId(30_300)]), 1);
    }

    #[test]
    fn model_requires_all_runtime_fields() {
        let model = "NNUE-FIXTURE 1\nhidden_units 2\n";

        assert_eq!(
            NnueScorer::from_model(model),
            Err("model is missing required fields".to_owned())
        );
    }

    #[test]
    fn model_rejects_unsafe_output_shift() {
        let model = "NNUE-FIXTURE 1\nhidden_units 2\nhidden_bias 0 0\noutput_weights 2 1\noutput_bias 0\noutput_shift 32\n";

        assert_eq!(
            NnueScorer::from_model(model),
            Err("output_shift must be less than 32".to_owned())
        );
    }

    #[test]
    fn fixture_inference_uses_role_features() {
        let scorer = NnueScorer::default();

        assert_ne!(scorer.score(&[FeatureId(0)]), scorer.score(&[FeatureId(1)]));
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

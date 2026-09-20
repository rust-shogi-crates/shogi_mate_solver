use crate::features::FeatureId;

mod parse;

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
        let mut scorer = Self::empty();
        let mut lines = text.lines();
        if lines.next() != Some("NNUE-FIXTURE 1") {
            return Err("expected NNUE-FIXTURE 1 header".to_owned());
        }
        let mut seen_hidden_units = false;
        let mut seen_hidden_bias = false;
        let mut seen_output_weights = false;
        let mut seen_output_bias = false;
        let mut seen_output_shift = false;

        for line in lines {
            let mut fields = line.split_whitespace();
            match fields.next() {
                Some("hidden_units") => {
                    if seen_hidden_units {
                        return Err("duplicate hidden_units".to_owned());
                    }
                    parse::expect_values(&mut fields, &["2"], "hidden_units")?;
                    seen_hidden_units = true;
                }
                Some("hidden_bias") => {
                    if seen_hidden_bias {
                        return Err("duplicate hidden_bias".to_owned());
                    }
                    scorer.hidden_bias = parse::parse_pair(&mut fields, "hidden_bias")?;
                    seen_hidden_bias = true;
                }
                Some("output_weights") => {
                    if seen_output_weights {
                        return Err("duplicate output_weights".to_owned());
                    }
                    scorer.output_weights = parse::parse_pair(&mut fields, "output_weights")?;
                    seen_output_weights = true;
                }
                Some("output_bias") => {
                    if seen_output_bias {
                        return Err("duplicate output_bias".to_owned());
                    }
                    scorer.output_bias = parse::parse_one(&mut fields, "output_bias")?;
                    seen_output_bias = true;
                }
                Some("output_shift") => {
                    if seen_output_shift {
                        return Err("duplicate output_shift".to_owned());
                    }
                    let output_shift = parse::parse_unsigned(&mut fields, "output_shift")?;
                    if output_shift >= i32::BITS {
                        return Err("output_shift must be less than 32".to_owned());
                    }
                    scorer.output_shift = output_shift;
                    seen_output_shift = true;
                }
                Some("feature") => {
                    if scorer.feature_count == MAX_FEATURE_WEIGHTS {
                        return Err("too many feature weights".to_owned());
                    }
                    let feature = parse::parse_feature(&mut fields)?;
                    let weights = parse::parse_pair(&mut fields, "feature weights")?;
                    scorer.add_feature(FeatureId(feature), weights);
                }
                Some(other) => return Err(format!("unknown model field: {other}")),
                None => {}
            }
        }
        if !seen_hidden_units
            || !seen_hidden_bias
            || !seen_output_weights
            || !seen_output_bias
            || !seen_output_shift
        {
            return Err("model is missing required fields".to_owned());
        }
        Ok(scorer)
    }

    pub fn score(&self, features: &[FeatureId]) -> i32 {
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
}

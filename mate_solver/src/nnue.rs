use crate::features::FeatureId;

const HIDDEN_UNITS: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FeatureWeight {
    feature: FeatureId,
    weights: [i32; HIDDEN_UNITS],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NnueScorer {
    hidden_bias: [i32; HIDDEN_UNITS],
    feature_weights: &'static [FeatureWeight],
    output_weights: [i32; HIDDEN_UNITS],
    output_bias: i32,
    output_shift: u32,
}

impl Default for NnueScorer {
    fn default() -> Self {
        Self {
            hidden_bias: [0, 0],
            feature_weights: &[
                // The fixture intentionally gives promoted moves a positive signal.
                FeatureWeight {
                    feature: FeatureId(30_300),
                    weights: [64, 32],
                },
                FeatureWeight {
                    feature: FeatureId(0),
                    weights: [128, -128],
                },
                FeatureWeight {
                    feature: FeatureId(1),
                    weights: [-128, 128],
                },
            ],
            output_weights: [2, 1],
            output_bias: 0,
            output_shift: 7,
        }
    }
}

impl NnueScorer {
    pub fn score(&self, features: &[FeatureId]) -> i32 {
        let mut hidden = self.hidden_bias;
        for &feature in features {
            if let Some(weight) = self
                .feature_weights
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
    fn fixture_inference_uses_role_features() {
        let scorer = NnueScorer::default();

        assert_ne!(scorer.score(&[FeatureId(0)]), scorer.score(&[FeatureId(1)]));
    }
}

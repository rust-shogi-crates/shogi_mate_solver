use crate::features::FeatureId;

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

    /// Loads the versioned text format emitted by `nnue_training export`.
    pub fn from_model(text: &str) -> Result<Self, String> {
        let mut scorer = Self::empty();
        let mut lines = text.lines();
        if lines.next() != Some("NNUE-FIXTURE 1") {
            return Err("expected NNUE-FIXTURE 1 header".to_owned());
        }

        for line in lines {
            let mut fields = line.split_whitespace();
            match fields.next() {
                Some("hidden_units") => expect_values(&mut fields, &["2"], "hidden_units")?,
                Some("hidden_bias") => {
                    scorer.hidden_bias = parse_pair(&mut fields, "hidden_bias")?;
                }
                Some("output_weights") => {
                    scorer.output_weights = parse_pair(&mut fields, "output_weights")?;
                }
                Some("output_bias") => {
                    scorer.output_bias = parse_one(&mut fields, "output_bias")?;
                }
                Some("output_shift") => {
                    scorer.output_shift = parse_unsigned(&mut fields, "output_shift")?;
                }
                Some("feature") => {
                    if scorer.feature_count == MAX_FEATURE_WEIGHTS {
                        return Err("too many feature weights".to_owned());
                    }
                    let feature = fields
                        .next()
                        .ok_or_else(|| "missing feature".to_owned())?
                        .parse::<u32>()
                        .map_err(|error| format!("invalid feature: {error}"))?;
                    let weights = parse_pair(&mut fields, "feature weights")?;
                    scorer.add_feature(FeatureId(feature), weights);
                }
                Some(other) => return Err(format!("unknown model field: {other}")),
                None => {}
            }
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

fn expect_values<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    expected: &[&str],
    name: &str,
) -> Result<(), String> {
    let values = fields.collect::<Vec<_>>();
    if values == expected {
        Ok(())
    } else {
        Err(format!("invalid {name}"))
    }
}

fn parse_one<'a>(fields: &mut impl Iterator<Item = &'a str>, name: &str) -> Result<i32, String> {
    let value = fields
        .next()
        .ok_or_else(|| format!("missing {name}"))?
        .parse()
        .map_err(|error| format!("invalid {name}: {error}"))?;
    if fields.next().is_some() {
        return Err(format!("too many values for {name}"));
    }
    Ok(value)
}

fn parse_unsigned<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    name: &str,
) -> Result<u32, String> {
    let value = fields
        .next()
        .ok_or_else(|| format!("missing {name}"))?
        .parse()
        .map_err(|error| format!("invalid {name}: {error}"))?;
    if fields.next().is_some() {
        return Err(format!("too many values for {name}"));
    }
    Ok(value)
}

fn parse_pair<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    name: &str,
) -> Result<[i32; HIDDEN_UNITS], String> {
    let values = fields
        .map(|value| {
            value
                .parse()
                .map_err(|error| format!("invalid {name}: {error}"))
        })
        .collect::<Result<Vec<i32>, _>>()?;
    values
        .try_into()
        .map_err(|_| format!("expected two values for {name}"))
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
    fn exported_model_round_trips_into_runtime() {
        let model = "NNUE-FIXTURE 1\nhidden_units 2\nhidden_bias 0 0\noutput_weights 2 1\noutput_bias 0\noutput_shift 7\nfeature 30300 64 32\n";
        let scorer = NnueScorer::from_model(model).unwrap();

        assert_eq!(scorer.score(&[FeatureId(30_300)]), 1);
    }

    #[test]
    fn fixture_inference_uses_role_features() {
        let scorer = NnueScorer::default();

        assert_ne!(scorer.score(&[FeatureId(0)]), scorer.score(&[FeatureId(1)]));
    }
}

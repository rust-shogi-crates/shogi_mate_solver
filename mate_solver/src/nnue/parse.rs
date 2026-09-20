use std::collections::BTreeMap;

const HIDDEN_UNITS: usize = 2;
const MAX_FEATURE_WEIGHTS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Model {
    pub hidden_bias: [i32; HIDDEN_UNITS],
    pub feature_weights: BTreeMap<u32, [i32; HIDDEN_UNITS]>,
    pub output_weights: [i32; HIDDEN_UNITS],
    pub output_bias: i32,
    pub output_shift: u32,
}

impl Model {
    pub fn empty() -> Self {
        Self {
            hidden_bias: [0; HIDDEN_UNITS],
            feature_weights: BTreeMap::new(),
            output_weights: [2, 1],
            output_bias: 0,
            output_shift: 7,
        }
    }

    pub fn to_text(&self) -> String {
        let mut text = String::from("NNUE-FIXTURE 1\n");
        text.push_str("hidden_units 2\n");
        text.push_str(&format!(
            "hidden_bias {} {}\n",
            self.hidden_bias[0], self.hidden_bias[1]
        ));
        text.push_str(&format!(
            "output_weights {} {}\n",
            self.output_weights[0], self.output_weights[1]
        ));
        text.push_str(&format!("output_bias {}\n", self.output_bias));
        text.push_str(&format!("output_shift {}\n", self.output_shift));
        for (&feature, &[weight0, weight1]) in &self.feature_weights {
            if weight0 != 0 || weight1 != 0 {
                text.push_str(&format!("feature {feature} {weight0} {weight1}\n"));
            }
        }
        text
    }
}

pub fn parse_model(text: &str) -> Result<Model, String> {
    let mut model = Model::empty();
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
                expect_values(&mut fields, &["2"], "hidden_units")?;
                seen_hidden_units = true;
            }
            Some("hidden_bias") => {
                if seen_hidden_bias {
                    return Err("duplicate hidden_bias".to_owned());
                }
                model.hidden_bias = parse_pair(&mut fields, "hidden_bias")?;
                seen_hidden_bias = true;
            }
            Some("output_weights") => {
                if seen_output_weights {
                    return Err("duplicate output_weights".to_owned());
                }
                model.output_weights = parse_pair(&mut fields, "output_weights")?;
                seen_output_weights = true;
            }
            Some("output_bias") => {
                if seen_output_bias {
                    return Err("duplicate output_bias".to_owned());
                }
                model.output_bias = parse_one(&mut fields, "output_bias")?;
                seen_output_bias = true;
            }
            Some("output_shift") => {
                if seen_output_shift {
                    return Err("duplicate output_shift".to_owned());
                }
                model.output_shift = parse_unsigned(&mut fields, "output_shift")?;
                if model.output_shift >= i32::BITS {
                    return Err("output_shift must be less than 32".to_owned());
                }
                seen_output_shift = true;
            }
            Some("feature") => {
                if model.feature_weights.len() == MAX_FEATURE_WEIGHTS {
                    return Err("too many feature weights".to_owned());
                }
                let feature = parse_feature(&mut fields)?;
                let weights = parse_pair(&mut fields, "feature weights")?;
                if model.feature_weights.insert(feature, weights).is_some() {
                    return Err(format!("duplicate feature {feature}"));
                }
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
    Ok(model)
}

pub(super) fn expect_values<'a>(
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

pub(super) fn parse_one<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    name: &str,
) -> Result<i32, String> {
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

pub(super) fn parse_unsigned<'a>(
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

pub(super) fn parse_pair<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    name: &str,
) -> Result<[i32; 2], String> {
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

pub(super) fn parse_feature<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Result<u32, String> {
    fields
        .next()
        .ok_or_else(|| "missing feature".to_owned())?
        .parse::<u32>()
        .map_err(|error| format!("invalid feature: {error}"))
}

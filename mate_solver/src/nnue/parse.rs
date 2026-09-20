use std::collections::BTreeMap;

pub const DEEP_INPUTS: usize = 512;
pub const DEEP_HIDDEN_1: usize = 32;
pub const DEEP_HIDDEN_2: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeepModel {
    pub input_weights: BTreeMap<u32, Vec<i32>>,
    pub layer1_weights: Vec<i32>,
    pub layer1_bias: Vec<i32>,
    pub layer2_weights: Vec<i32>,
    pub layer2_bias: Vec<i32>,
    pub output_weights: Vec<i32>,
    pub output_bias: i32,
    pub output_shift: u32,
}

impl DeepModel {
    pub fn empty() -> Self {
        let mut layer1_weights = vec![0; DEEP_HIDDEN_1 * DEEP_INPUTS];
        for input in 0..DEEP_INPUTS {
            layer1_weights[(input % DEEP_HIDDEN_1) * DEEP_INPUTS + input] = 1;
        }
        let mut layer2_weights = vec![0; DEEP_HIDDEN_2 * DEEP_HIDDEN_1];
        for unit in 0..DEEP_HIDDEN_2 {
            layer2_weights[unit * DEEP_HIDDEN_1 + unit] = 1;
        }
        Self {
            input_weights: BTreeMap::new(),
            layer1_weights,
            layer1_bias: vec![1; DEEP_HIDDEN_1],
            layer2_weights,
            layer2_bias: vec![1; DEEP_HIDDEN_2],
            output_weights: vec![1; DEEP_HIDDEN_2],
            output_bias: 0,
            output_shift: 0,
        }
    }

    pub fn to_text(&self) -> String {
        let mut text = String::from("NNUE-FIXTURE 1\nhidden_units 512 32 32\n");
        text.push_str(&format!("output_shift {}\n", self.output_shift));
        for (&feature, weights) in &self.input_weights {
            text.push_str(&format!("input_weights {feature}"));
            for weight in weights {
                text.push_str(&format!(" {weight}"));
            }
            text.push('\n');
        }
        write_vector_line(&mut text, "layer1_weights", &self.layer1_weights);
        write_vector_line(&mut text, "layer1_bias", &self.layer1_bias);
        write_vector_line(&mut text, "layer2_weights", &self.layer2_weights);
        write_vector_line(&mut text, "layer2_bias", &self.layer2_bias);
        write_vector_line(&mut text, "output_weights", &self.output_weights);
        text.push_str(&format!("output_bias {}\n", self.output_bias));
        text
    }
}

pub fn parse_deep_model(text: &str) -> Result<DeepModel, String> {
    let mut model = DeepModel::empty();
    let mut lines = text.lines();
    if lines.next() != Some("NNUE-FIXTURE 1") {
        return Err("expected NNUE-FIXTURE 1 header".to_owned());
    }
    let mut seen_units = false;
    let mut seen_shift = false;
    let mut seen_layer1 = false;
    let mut seen_layer1_bias = false;
    let mut seen_layer2 = false;
    let mut seen_layer2_bias = false;
    let mut seen_output = false;
    let mut seen_output_bias = false;
    for line in lines {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("hidden_units") => {
                if seen_units {
                    return Err("duplicate hidden_units".to_owned());
                }
                expect_values(&mut fields, &["512", "32", "32"], "hidden_units")?;
                seen_units = true;
            }
            Some("output_shift") => {
                if seen_shift {
                    return Err("duplicate output_shift".to_owned());
                }
                model.output_shift = parse_unsigned(&mut fields, "output_shift")?;
                if model.output_shift >= i32::BITS {
                    return Err("output_shift must be less than 32".to_owned());
                }
                seen_shift = true;
            }
            Some("input_weights") => {
                let feature = parse_feature(&mut fields)?;
                let weights = parse_vector(&mut fields, DEEP_INPUTS, "input_weights")?;
                if model.input_weights.insert(feature, weights).is_some() {
                    return Err(format!("duplicate input feature {feature}"));
                }
            }
            Some("layer1_weights") => {
                if seen_layer1 {
                    return Err("duplicate layer1_weights".to_owned());
                }
                model.layer1_weights =
                    parse_vector(&mut fields, DEEP_HIDDEN_1 * DEEP_INPUTS, "layer1_weights")?;
                seen_layer1 = true;
            }
            Some("layer1_bias") => {
                if seen_layer1_bias {
                    return Err("duplicate layer1_bias".to_owned());
                }
                model.layer1_bias = parse_vector(&mut fields, DEEP_HIDDEN_1, "layer1_bias")?;
                seen_layer1_bias = true;
            }
            Some("layer2_weights") => {
                if seen_layer2 {
                    return Err("duplicate layer2_weights".to_owned());
                }
                model.layer2_weights =
                    parse_vector(&mut fields, DEEP_HIDDEN_2 * DEEP_HIDDEN_1, "layer2_weights")?;
                seen_layer2 = true;
            }
            Some("layer2_bias") => {
                if seen_layer2_bias {
                    return Err("duplicate layer2_bias".to_owned());
                }
                model.layer2_bias = parse_vector(&mut fields, DEEP_HIDDEN_2, "layer2_bias")?;
                seen_layer2_bias = true;
            }
            Some("output_weights") => {
                if seen_output {
                    return Err("duplicate output_weights".to_owned());
                }
                model.output_weights = parse_vector(&mut fields, DEEP_HIDDEN_2, "output_weights")?;
                seen_output = true;
            }
            Some("output_bias") => {
                if seen_output_bias {
                    return Err("duplicate output_bias".to_owned());
                }
                model.output_bias = parse_one(&mut fields, "output_bias")?;
                seen_output_bias = true;
            }
            Some(other) => return Err(format!("unknown model field: {other}")),
            None => {}
        }
    }
    if !seen_units
        || !seen_shift
        || !seen_layer1
        || !seen_layer1_bias
        || !seen_layer2
        || !seen_layer2_bias
        || !seen_output
        || !seen_output_bias
    {
        return Err("model is missing required fields".to_owned());
    }
    Ok(model)
}

fn write_vector_line(text: &mut String, name: &str, values: &[i32]) {
    text.push_str(name);
    for value in values {
        text.push_str(&format!(" {value}"));
    }
    text.push('\n');
}

fn parse_vector<'a>(
    fields: &mut impl Iterator<Item = &'a str>,
    expected_len: usize,
    name: &str,
) -> Result<Vec<i32>, String> {
    let values = fields
        .map(|value| {
            value
                .parse()
                .map_err(|error| format!("invalid {name}: {error}"))
        })
        .collect::<Result<Vec<i32>, _>>()?;
    if values.len() != expected_len {
        return Err(format!("expected {expected_len} values for {name}"));
    }
    Ok(values)
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

pub(super) fn parse_feature<'a>(fields: &mut impl Iterator<Item = &'a str>) -> Result<u32, String> {
    fields
        .next()
        .ok_or_else(|| "missing feature".to_owned())?
        .parse::<u32>()
        .map_err(|error| format!("invalid feature: {error}"))
}

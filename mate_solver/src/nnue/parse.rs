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

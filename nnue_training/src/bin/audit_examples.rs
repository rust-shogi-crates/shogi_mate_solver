//! Recheck labels without treating an incomplete DF-PN search as non-mate.
use mate_solver::{
    SearchConfig, df_pn::search as dfpnsearch, features::FeatureRole,
    move_ordering::MoveOrderingOptions, position_wrapper::PositionWrapper, tt::DfPnTable,
};
use shogi_core::PartialPosition;
use shogi_usi_parser::FromUsi;
use std::{
    env, fs,
    io::{BufRead, BufReader, BufWriter, Write},
    process,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: audit_examples INPUT.jsonl OUTPUT.jsonl (output must not exist)".into(),
        );
    }
    let input = BufReader::new(fs::File::open(&args[0])?);
    let mut output = BufWriter::new(
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[1])?,
    );
    let mut table = DfPnTable::new(1 << 16);
    let (mut count, mut kept, mut corrected, mut unresolved) = (0, 0, 0, 0);
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut row: serde_json::Value = serde_json::from_str(&line)?;
        let sfen = row["sfen"].as_str().ok_or("missing sfen")?;
        let position = PartialPosition::from_usi(&format!("sfen {sfen}"))
            .map_err(|e| format!("invalid SFEN: {e:?}"))?;
        let role = match row["role"].as_str() {
            Some("attacker") => FeatureRole::Attacker,
            Some("defender") => FeatureRole::Defender,
            _ => return Err("invalid role".into()),
        };
        let kind = if role == FeatureRole::Attacker {
            dfpnsearch::NodeKind::Or
        } else {
            dfpnsearch::NodeKind::And
        };
        // A low proof-number threshold keeps auditing bounded. Unknowns are omitted.
        let mut stats = dfpnsearch::SearchStats::default();
        let result = dfpnsearch::mid_with_options_and_stats(
            &mut table,
            &PositionWrapper::new(position),
            (10, 10),
            kind,
            true,
            &mut dfpnsearch::SearchCtx::with_config(SearchConfig::with_max_positions(20_000)),
            false,
            &mut stats,
            &MoveOrderingOptions::default(),
        );
        count += 1;
        if stats.limit_reached || (result.0 != 0 && result.1 != 0) {
            unresolved += 1;
            continue;
        }
        let label = u8::from(if role == FeatureRole::Attacker {
            result.0 == 0
        } else {
            result.1 == 0
        });
        if row["label"].as_u64() != Some(u64::from(label)) {
            corrected += 1;
        }
        row["label"] = label.into();
        writeln!(output, "{row}")?;
        kept += 1;
    }
    output.flush()?;
    println!("checked={count} kept={kept} corrected={corrected} unresolved={unresolved}");
    Ok(())
}

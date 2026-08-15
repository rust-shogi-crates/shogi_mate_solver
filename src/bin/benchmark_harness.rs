use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File},
    io::{self, BufRead, BufReader, Write},
    process,
    time::Instant,
};

use mate_solver::{
    df_pn::search as dfpnsearch,
    eval::{Value, search as evalsearch},
    position_wrapper::PositionWrapper,
    tt::{DfPnTable, EvalTable},
};
use serde_json::{Value as JsonValue, json};
use shogi_core::PartialPosition;
use shogi_usi_parser::FromUsi;

const TABLE_SIZE: usize = 1 << 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Expected {
    Mate,
    NoMate,
}

impl Expected {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "mate" => Some(Self::Mate),
            "nomate" => Some(Self::NoMate),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Mate => "mate",
            Self::NoMate => "nomate",
        }
    }
}

#[derive(Debug)]
struct PositionRecord {
    id: String,
    source: String,
    line: u64,
    sfen: String,
    expected: Option<Expected>,
    expected_plies: Option<u64>,
}

#[derive(Clone, Debug)]
struct ResultRecord {
    id: String,
    evaluator: String,
    elapsed_ms: f64,
    positions_inspected: u64,
    correct: Option<bool>,
}

#[derive(Default)]
struct CompareTotals {
    positions: u64,
    correct_base: u64,
    correct_current: u64,
    elapsed_base: Vec<f64>,
    elapsed_current: Vec<f64>,
    inspected_base: Vec<f64>,
    inspected_current: Vec<f64>,
    ratios: Vec<f64>,
    inspected_ratios: Vec<f64>,
}

struct ComparisonSummary {
    evaluator: String,
    positions: u64,
    correct_base: u64,
    correct_current: u64,
    elapsed_base: Stats,
    elapsed_current: Stats,
    inspected_base: Stats,
    inspected_current: Stats,
    ratio: Stats,
    inspected_ratio: Stats,
    passed: bool,
}

#[derive(Clone)]
struct Stats {
    total: f64,
    mean: Option<f64>,
    median: Option<f64>,
    stddev: Option<f64>,
    p90: Option<f64>,
    p95: Option<f64>,
    p99: Option<f64>,
}

fn main() {
    if let Err(()) = run() {
        process::exit(1);
    }
}

fn run() -> Result<(), ()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run_benchmark(&args[1..]),
        Some("compare") => compare_outputs(&args[1..]),
        _ => {
            print_usage();
            Err(())
        }
    }
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!(
        "  benchmark_harness run [--strict] [--verbose] [--revision=<label>] <positions.jsonl>..."
    );
    eprintln!(
        "  benchmark_harness compare --base <base.jsonl> --current <current.jsonl> [--html <report.html>]"
    );
}

fn run_benchmark(args: &[String]) -> Result<(), ()> {
    let mut revision = "current".to_owned();
    let mut strict = false;
    let mut verbose = false;
    let mut inputs = Vec::new();

    for arg in args {
        if arg == "--strict" {
            strict = true;
        } else if arg == "--verbose" {
            verbose = true;
        } else if let Some(rest) = arg.strip_prefix("--revision=") {
            revision = rest.to_owned();
        } else {
            inputs.push(arg.clone());
        }
    }

    if inputs.is_empty() {
        print_usage();
        return Err(());
    }

    println!(
        "{}",
        json!({
            "type": "metadata",
            "mode": "run",
            "revision": revision,
        "inputs": &inputs,
        })
    );

    let mut failed = false;
    for input in inputs {
        let file = match File::open(&input) {
            Ok(file) => file,
            Err(error) => {
                emit_error(&input, 0, "open", error.to_string(), "");
                failed = true;
                continue;
            }
        };
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line_number = index as u64 + 1;
            let raw_line = match line {
                Ok(line) => line,
                Err(error) => {
                    emit_error(&input, line_number, "read", error.to_string(), "");
                    failed = true;
                    continue;
                }
            };
            if raw_line.trim().is_empty() {
                continue;
            }
            let record = match parse_position_record(&input, line_number, &raw_line, strict) {
                Ok(record) => record,
                Err(message) => {
                    emit_error(&input, line_number, "parse", message, &raw_line);
                    failed = true;
                    continue;
                }
            };
            if let Err(message) = evaluate_position(&record, verbose) {
                emit_error(&input, line_number, "evaluate", message, &raw_line);
                failed = true;
            }
        }
    }

    if failed { Err(()) } else { Ok(()) }
}

fn parse_position_record(
    source: &str,
    line: u64,
    raw_line: &str,
    strict: bool,
) -> Result<PositionRecord, String> {
    let value: JsonValue =
        serde_json::from_str(raw_line).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "line must be a JSON object".to_owned())?;
    let sfen = object
        .get("sfen")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "missing string field `sfen`".to_owned())?
        .to_owned();
    let id = object
        .get("id")
        .and_then(JsonValue::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{source}:{line}"));
    let expected = match object.get("expected") {
        Some(value) => {
            let text = value
                .as_str()
                .ok_or_else(|| "`expected` must be a string".to_owned())?;
            Some(
                Expected::parse(text)
                    .ok_or_else(|| "`expected` must be `mate` or `nomate`".to_owned())?,
            )
        }
        None if strict => return Err("missing string field `expected`".to_owned()),
        None => None,
    };
    let expected_plies = match object.get("expected_plies") {
        Some(value) => Some(
            value
                .as_u64()
                .ok_or_else(|| "`expected_plies` must be a non-negative integer".to_owned())?,
        ),
        None => None,
    };

    Ok(PositionRecord {
        id,
        source: source.to_owned(),
        line,
        sfen,
        expected,
        expected_plies,
    })
}

fn evaluate_position(record: &PositionRecord, verbose: bool) -> Result<(), String> {
    let position = PartialPosition::from_usi(&format!("sfen {}", record.sfen))
        .map_err(|error| format!("invalid SFEN: {error:?}"))?;
    evaluate_df_pn(record, &position, verbose);
    evaluate_eval(record, &position, verbose);
    Ok(())
}

fn evaluate_df_pn(record: &PositionRecord, position: &PartialPosition, verbose: bool) {
    let mut df_pn = DfPnTable::new(TABLE_SIZE);
    let mut stats = dfpnsearch::SearchStats::default();
    let started = Instant::now();
    let (proof_number, disproof_number) = dfpnsearch::df_pn_with_stats(
        &mut df_pn,
        &PositionWrapper::new(position.clone()),
        verbose,
        &mut stats,
    );
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let resolution = if (proof_number, disproof_number) == (u32::MAX, 0) {
        Expected::NoMate
    } else {
        Expected::Mate
    };
    println!(
        "{}",
        json!({
            "type": "result",
            "id": record.id,
            "source": record.source,
            "line": record.line,
            "evaluator": "df_pn",
            "elapsed_ms": elapsed_ms,
            "positions_inspected": stats.positions_inspected,
            "resolution": resolution.as_str(),
            "expected": record.expected.map(Expected::as_str),
            "correct": record.expected.map(|expected| expected == resolution),
            "proof_number": proof_number,
            "disproof_number": disproof_number,
        })
    );
}

fn evaluate_eval(record: &PositionRecord, position: &PartialPosition, verbose: bool) {
    let mut df_pn = DfPnTable::new(TABLE_SIZE);
    let mut eval = EvalTable::new(TABLE_SIZE);
    let mut seed_stats = dfpnsearch::SearchStats::default();
    let mut eval_stats = evalsearch::SearchStats::default();
    dfpnsearch::df_pn_with_stats(
        &mut df_pn,
        &PositionWrapper::new(position.clone()),
        verbose,
        &mut seed_stats,
    );
    let mut df_pn_stats = dfpnsearch::SearchStats::default();
    let started = Instant::now();
    let value = evalsearch::search_with_stats(
        position,
        &mut df_pn,
        &mut eval,
        verbose,
        &mut eval_stats,
        &mut df_pn_stats,
    );
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let resolution = if value.is_mate() {
        Expected::Mate
    } else {
        Expected::NoMate
    };
    let correct = record.expected.map(|expected| {
        expected == resolution
            && record.expected_plies.is_none_or(|expected_plies| {
                !value.is_mate() || value.plies() as u64 == expected_plies
            })
    });
    println!(
        "{}",
        json!({
            "type": "result",
            "id": record.id,
            "source": record.source,
            "line": record.line,
            "evaluator": "eval",
            "elapsed_ms": elapsed_ms,
            "positions_inspected": eval_stats.positions_inspected + df_pn_stats.positions_inspected,
            "resolution": resolution.as_str(),
            "expected": record.expected.map(Expected::as_str),
            "expected_plies": record.expected_plies,
            "correct": correct,
            "value": value_json(value),
        })
    );
}

fn value_json(value: Value) -> JsonValue {
    json!({
        "raw": value.0,
        "plies": value.plies(),
        "pieces": value.pieces(),
        "futile": value.futile(),
    })
}

fn emit_error(source: &str, line: u64, stage: &str, message: String, raw_line: &str) {
    println!(
        "{}",
        json!({
            "type": "error",
            "id": format!("{source}:{line}"),
            "source": source,
            "line": line,
            "stage": stage,
            "message": message,
            "raw_line": raw_line,
        })
    );
}

fn compare_outputs(args: &[String]) -> Result<(), ()> {
    let mut base = None;
    let mut current = None;
    let mut html = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--base" => {
                index += 1;
                base = args.get(index).cloned();
            }
            "--current" => {
                index += 1;
                current = args.get(index).cloned();
            }
            "--html" => {
                index += 1;
                html = args.get(index).cloned();
            }
            _ => {
                print_usage();
                return Err(());
            }
        }
        index += 1;
    }
    let base = base.ok_or_else(|| {
        print_usage();
    })?;
    let current = current.ok_or_else(|| {
        print_usage();
    })?;

    println!(
        "{}",
        json!({
            "type": "metadata",
            "mode": "compare",
            "base": &base,
            "current": &current,
        })
    );

    let (base_records, base_failed) = read_result_records(&base);
    let (current_records, current_failed) = read_result_records(&current);
    let mut failed = base_failed || current_failed;
    let mut evaluators = BTreeSet::new();
    evaluators.extend(base_records.keys().map(|(_, evaluator)| evaluator.clone()));
    evaluators.extend(
        current_records
            .keys()
            .map(|(_, evaluator)| evaluator.clone()),
    );

    let mut aggregate = CompareTotals::default();
    let mut summaries = Vec::new();
    for evaluator in evaluators {
        let mut totals = CompareTotals::default();
        let ids: BTreeSet<_> = base_records
            .keys()
            .filter(|(_, key_evaluator)| key_evaluator == &evaluator)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let key = (id.clone(), evaluator.clone());
            let Some(base_record) = base_records.get(&key) else {
                emit_compare_error(
                    "compare",
                    format!("missing base result for {id}/{evaluator}"),
                );
                failed = true;
                continue;
            };
            let Some(current_record) = current_records.get(&key) else {
                emit_compare_error(
                    "compare",
                    format!("missing current result for {id}/{evaluator}"),
                );
                failed = true;
                continue;
            };
            add_pair(&mut totals, base_record, current_record);
            add_pair(&mut aggregate, base_record, current_record);
        }
        let summary = comparison_summary(&evaluator, &totals);
        failed |= !summary.passed;
        emit_comparison(&summary);
        summaries.push(summary);
    }
    let aggregate_summary = comparison_summary("all", &aggregate);
    failed |= !aggregate_summary.passed;
    emit_comparison(&aggregate_summary);
    summaries.push(aggregate_summary);

    if let Some(path) = html {
        if let Err(error) = write_html_report(&path, &base, &current, &summaries) {
            emit_compare_error("html", format!("{path}: {error}"));
            failed = true;
        }
    }

    if failed { Err(()) } else { Ok(()) }
}

fn read_result_records(path: &str) -> (BTreeMap<(String, String), ResultRecord>, bool) {
    let mut records = BTreeMap::new();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            emit_compare_error("open", format!("{path}: {error}"));
            return (records, true);
        }
    };
    let mut failed = false;
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index as u64 + 1;
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                emit_compare_error("read", format!("{path}:{line_number}: {error}"));
                failed = true;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let value: JsonValue = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                emit_compare_error("parse", format!("{path}:{line_number}: {error}"));
                failed = true;
                continue;
            }
        };
        match value.get("type").and_then(JsonValue::as_str) {
            Some("result") => match parse_result_record(&value) {
                Ok(record) => {
                    records.insert((record.id.clone(), record.evaluator.clone()), record);
                }
                Err(message) => {
                    emit_compare_error("parse", format!("{path}:{line_number}: {message}"));
                    failed = true;
                }
            },
            Some("error") => {
                emit_compare_error(
                    "input",
                    format!("{path}:{line_number}: input run emitted error"),
                );
                failed = true;
            }
            _ => {}
        }
    }
    (records, failed)
}

fn parse_result_record(value: &JsonValue) -> Result<ResultRecord, String> {
    Ok(ResultRecord {
        id: required_str(value, "id")?.to_owned(),
        evaluator: required_str(value, "evaluator")?.to_owned(),
        elapsed_ms: required_f64(value, "elapsed_ms")?,
        positions_inspected: value
            .get("positions_inspected")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| "missing integer field `positions_inspected`".to_owned())?,
        correct: value.get("correct").and_then(JsonValue::as_bool),
    })
}

fn required_str<'a>(value: &'a JsonValue, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(JsonValue::as_str)
        .ok_or_else(|| format!("missing string field `{field}`"))
}

fn required_f64(value: &JsonValue, field: &str) -> Result<f64, String> {
    value
        .get(field)
        .and_then(JsonValue::as_f64)
        .ok_or_else(|| format!("missing number field `{field}`"))
}

fn add_pair(totals: &mut CompareTotals, base: &ResultRecord, current: &ResultRecord) {
    totals.positions += 1;
    totals.correct_base += u64::from(base.correct.unwrap_or(false));
    totals.correct_current += u64::from(current.correct.unwrap_or(false));
    totals.elapsed_base.push(base.elapsed_ms);
    totals.elapsed_current.push(current.elapsed_ms);
    totals.inspected_base.push(base.positions_inspected as f64);
    totals
        .inspected_current
        .push(current.positions_inspected as f64);
    if base.elapsed_ms > 0.0 {
        totals.ratios.push(current.elapsed_ms / base.elapsed_ms);
    }
    if base.positions_inspected > 0 {
        totals
            .inspected_ratios
            .push(current.positions_inspected as f64 / base.positions_inspected as f64);
    }
}

fn comparison_summary(evaluator: &str, totals: &CompareTotals) -> ComparisonSummary {
    let failed = totals.correct_current < totals.correct_base
        || totals.correct_current < totals.positions
        || totals.positions == 0;
    ComparisonSummary {
        evaluator: evaluator.to_owned(),
        positions: totals.positions,
        correct_base: totals.correct_base,
        correct_current: totals.correct_current,
        elapsed_base: stats(&totals.elapsed_base),
        elapsed_current: stats(&totals.elapsed_current),
        inspected_base: stats(&totals.inspected_base),
        inspected_current: stats(&totals.inspected_current),
        ratio: stats(&totals.ratios),
        inspected_ratio: stats(&totals.inspected_ratios),
        passed: !failed,
    }
}

fn emit_comparison(summary: &ComparisonSummary) {
    println!(
        "{}",
        json!({
            "type": "comparison",
            "evaluator": &summary.evaluator,
            "positions": summary.positions,
            "correct_base": summary.correct_base,
            "correct_current": summary.correct_current,
            "elapsed_ms_base": stats_to_json(&summary.elapsed_base),
            "elapsed_ms_current": stats_to_json(&summary.elapsed_current),
            "positions_inspected_base": stats_to_json(&summary.inspected_base),
            "positions_inspected_current": stats_to_json(&summary.inspected_current),
            "ratio": stats_to_json(&summary.ratio),
            "positions_inspected_ratio": stats_to_json(&summary.inspected_ratio),
            "passed": summary.passed,
        })
    );
}

fn stats(values: &[f64]) -> Stats {
    if values.is_empty() {
        return Stats {
            total: 0.0,
            mean: None,
            median: None,
            stddev: None,
            p90: None,
            p95: None,
            p99: None,
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let total: f64 = sorted.iter().sum();
    let mean = total / sorted.len() as f64;
    let variance = sorted
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f64>()
        / sorted.len() as f64;
    Stats {
        total,
        mean: Some(mean),
        median: Some(median(&sorted)),
        stddev: Some(variance.sqrt()),
        p90: Some(percentile(&sorted, 0.90)),
        p95: Some(percentile(&sorted, 0.95)),
        p99: Some(percentile(&sorted, 0.99)),
    }
}

fn stats_to_json(stats: &Stats) -> JsonValue {
    json!({
        "total": stats.total,
        "mean": stats.mean,
        "median": stats.median,
        "stddev": stats.stddev,
        "p90": stats.p90,
        "p95": stats.p95,
        "p99": stats.p99,
    })
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    let index = ((sorted.len() as f64 * quantile).ceil() as usize).saturating_sub(1);
    sorted[index.min(sorted.len() - 1)]
}

fn median(sorted: &[f64]) -> f64 {
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn emit_compare_error(stage: &str, message: String) {
    let _ = writeln!(
        io::stdout(),
        "{}",
        json!({
            "type": "error",
            "stage": stage,
            "message": message,
        })
    );
}

fn write_html_report(
    path: &str,
    base: &str,
    current: &str,
    summaries: &[ComparisonSummary],
) -> io::Result<()> {
    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    html.push_str("<title>Benchmark comparison</title>");
    html.push_str(
        "<style>
body{font-family:-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif;margin:2rem;color:#202124;background:#fff}
h1{font-size:1.6rem;margin:0 0 0.5rem}
h2{font-size:1.2rem;margin:2rem 0 0.5rem}
.meta{color:#5f6368;margin-bottom:1.5rem}
table{border-collapse:collapse;width:100%;margin:0.75rem 0 1.5rem}
th,td{border:1px solid #dadce0;padding:0.45rem 0.55rem;text-align:right;vertical-align:top}
th:first-child,td:first-child{text-align:left}
th{background:#f8fafd;font-weight:600}
.pass{color:#137333;font-weight:600}
.fail{color:#a50e0e;font-weight:600}
.num{font-variant-numeric:tabular-nums}
</style>",
    );
    html.push_str("</head><body>");
    html.push_str("<h1>Benchmark comparison</h1>");
    html.push_str("<div class=\"meta\">");
    html.push_str("Base: <code>");
    html_escape_into(&mut html, base);
    html.push_str("</code><br>Current: <code>");
    html_escape_into(&mut html, current);
    html.push_str("</code></div>");

    html.push_str("<h2>Summary</h2>");
    html.push_str("<table><thead><tr><th>Evaluator</th><th>Status</th><th>Positions</th><th>Base correct</th><th>Current correct</th><th>Elapsed ratio mean</th><th>Elapsed ratio median</th><th>Elapsed ratio p95</th><th>Inspected ratio mean</th></tr></thead><tbody>");
    for summary in summaries {
        html.push_str("<tr><td>");
        html_escape_into(&mut html, &summary.evaluator);
        html.push_str("</td><td class=\"");
        html.push_str(if summary.passed { "pass" } else { "fail" });
        html.push_str("\">");
        html.push_str(if summary.passed { "PASS" } else { "FAIL" });
        html.push_str("</td><td class=\"num\">");
        push_u64(&mut html, summary.positions);
        html.push_str("</td><td class=\"num\">");
        push_u64(&mut html, summary.correct_base);
        html.push_str("</td><td class=\"num\">");
        push_u64(&mut html, summary.correct_current);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(&mut html, summary.ratio.mean);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(&mut html, summary.ratio.median);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(&mut html, summary.ratio.p95);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(&mut html, summary.inspected_ratio.mean);
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");

    html.push_str("<h2>Elapsed Time</h2>");
    write_stats_table(
        &mut html,
        summaries,
        "ms",
        |summary| &summary.elapsed_base,
        |summary| &summary.elapsed_current,
    );
    html.push_str("<h2>Positions Inspected</h2>");
    write_stats_table(
        &mut html,
        summaries,
        "positions",
        |summary| &summary.inspected_base,
        |summary| &summary.inspected_current,
    );
    html.push_str("<h2>Ratios</h2>");
    write_ratio_table(&mut html, summaries);
    html.push_str("</body></html>\n");

    fs::write(path, html)
}

fn write_stats_table<FBase, FCurrent>(
    html: &mut String,
    summaries: &[ComparisonSummary],
    unit: &str,
    base_stats: FBase,
    current_stats: FCurrent,
) where
    FBase: Fn(&ComparisonSummary) -> &Stats,
    FCurrent: Fn(&ComparisonSummary) -> &Stats,
{
    html.push_str("<table><thead><tr><th>Evaluator</th><th>Unit</th><th>Base total</th><th>Current total</th><th>Base mean</th><th>Current mean</th><th>Base median</th><th>Current median</th><th>Base p95</th><th>Current p95</th><th>Base stddev</th><th>Current stddev</th></tr></thead><tbody>");
    for summary in summaries {
        let base = base_stats(summary);
        let current = current_stats(summary);
        html.push_str("<tr><td>");
        html_escape_into(html, &summary.evaluator);
        html.push_str("</td><td>");
        html_escape_into(html, unit);
        html.push_str("</td><td class=\"num\">");
        push_f64(html, base.total);
        html.push_str("</td><td class=\"num\">");
        push_f64(html, current.total);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(html, base.mean);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(html, current.mean);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(html, base.median);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(html, current.median);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(html, base.p95);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(html, current.p95);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(html, base.stddev);
        html.push_str("</td><td class=\"num\">");
        push_opt_f64(html, current.stddev);
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
}

fn write_ratio_table(html: &mut String, summaries: &[ComparisonSummary]) {
    html.push_str("<table><thead><tr><th>Evaluator</th><th>Metric</th><th>Mean</th><th>Median</th><th>Stddev</th><th>P90</th><th>P95</th><th>P99</th></tr></thead><tbody>");
    for summary in summaries {
        write_ratio_row(html, &summary.evaluator, "Elapsed time", &summary.ratio);
        write_ratio_row(
            html,
            &summary.evaluator,
            "Positions inspected",
            &summary.inspected_ratio,
        );
    }
    html.push_str("</tbody></table>");
}

fn write_ratio_row(html: &mut String, evaluator: &str, metric: &str, stats: &Stats) {
    html.push_str("<tr><td>");
    html_escape_into(html, evaluator);
    html.push_str("</td><td>");
    html_escape_into(html, metric);
    html.push_str("</td><td class=\"num\">");
    push_opt_f64(html, stats.mean);
    html.push_str("</td><td class=\"num\">");
    push_opt_f64(html, stats.median);
    html.push_str("</td><td class=\"num\">");
    push_opt_f64(html, stats.stddev);
    html.push_str("</td><td class=\"num\">");
    push_opt_f64(html, stats.p90);
    html.push_str("</td><td class=\"num\">");
    push_opt_f64(html, stats.p95);
    html.push_str("</td><td class=\"num\">");
    push_opt_f64(html, stats.p99);
    html.push_str("</td></tr>");
}

fn html_escape_into(output: &mut String, input: &str) {
    for ch in input.chars() {
        match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(ch),
        }
    }
}

fn push_u64(output: &mut String, value: u64) {
    output.push_str(&value.to_string());
}

fn push_f64(output: &mut String, value: f64) {
    output.push_str(&format!("{value:.3}"));
}

fn push_opt_f64(output: &mut String, value: Option<f64>) {
    match value {
        Some(value) => push_f64(output, value),
        None => output.push('-'),
    }
}

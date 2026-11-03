use std::{
    collections::{BTreeSet, HashSet},
    env, fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
    process,
    time::{Duration, Instant},
};

use mate_solver::{
    SearchConfig, df_pn::search as dfpnsearch, features::FeatureRole,
    move_ordering::MoveOrderingOptions, position_wrapper::PositionWrapper, tt::DfPnTable,
};
use serde::Serialize;
use shogi_core::{Move, PartialPosition, ToUsi};
use shogi_usi_parser::FromUsi;

const SOURCE_FILES: [&str; 5] = [
    "mate3.sfen",
    "mate5.sfen",
    "mate7.sfen",
    "mate9.sfen",
    "mate11.sfen",
];
const OUTPUT_FLUSH_INTERVAL: usize = 1_000;
const PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Serialize)]
struct TrainingExample {
    id: String,
    source_id: String,
    sfen: String,
    role: &'static str,
    label: u8,
    transform: &'static str,
    ply_offset: usize,
}

struct SourcePosition {
    source_id: String,
    sfen: String,
}

struct Limits {
    deadline: Option<Instant>,
    max_positions: Option<u64>,
    positions: u64,
    reached: bool,
    started_at: Instant,
    last_progress: Instant,
}

impl Limits {
    fn new(timeout_ms: u64, max_positions: Option<u64>) -> Self {
        let started_at = Instant::now();
        Self {
            deadline: Some(started_at + Duration::from_millis(timeout_ms)),
            max_positions,
            positions: 0,
            reached: false,
            started_at,
            last_progress: started_at,
        }
    }

    fn check_time(&mut self) -> bool {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.reached = true;
        }
        self.reached
    }

    fn check(&mut self) -> bool {
        self.check_time();
        self.reached
    }

    fn config(&self) -> SearchConfig {
        match (self.deadline, self.max_positions) {
            (Some(deadline), Some(max_positions)) => {
                SearchConfig::with_deadline_and_max_positions(deadline, max_positions)
            }
            (Some(deadline), None) => SearchConfig::with_deadline(deadline),
            (None, Some(max_positions)) => SearchConfig::with_max_positions(max_positions),
            (None, None) => SearchConfig::default(),
        }
    }

    fn add(&mut self, df_pn: &dfpnsearch::SearchStats) {
        self.positions = self.positions.saturating_add(df_pn.positions_inspected);
        self.check_time();
    }

    fn report_progress(
        &mut self,
        file_name: &str,
        processed: usize,
        sampled: usize,
        examples: usize,
    ) {
        let now = Instant::now();
        if now.duration_since(self.last_progress) < PROGRESS_INTERVAL {
            return;
        }
        self.last_progress = now;
        eprintln!(
            "progress: file={file_name} sources={processed}/{sampled} examples={examples} positions_inspected={} elapsed_ms={}",
            self.positions,
            now.duration_since(self.started_at).as_millis(),
        );
    }

    fn report_sampling(&mut self, file_name: &str, scanned: usize) {
        let now = Instant::now();
        if now.duration_since(self.last_progress) < PROGRESS_INTERVAL {
            return;
        }
        self.last_progress = now;
        eprintln!(
            "progress: file={file_name} sampling_lines={scanned} examples=0 positions_inspected=0 elapsed_ms={}",
            now.duration_since(self.started_at).as_millis(),
        );
    }
}

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 7;
        x ^= x >> 9;
        x ^= x << 8;
        self.0 = x;
        x
    }

    fn index(&mut self, len: usize) -> usize {
        (self.next() as usize) % len
    }
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut input_dir = None;
    let mut output = None;
    let mut samples_per_file = 50_000usize;
    let mut negatives_per_position = 2usize;
    let mut plies = 3usize;
    let mut timeout_ms = 60_000u64;
    let mut max_positions = None;
    let mut seed = 0x5eeda11u64;
    let mut mirror = true;
    let mut root_role = FeatureRole::Defender;

    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--input-dir=") {
            input_dir = Some(value.to_owned());
        } else if arg == "--input-dir" {
            input_dir = Some(next_arg(&mut args, "--input-dir")?);
        } else if let Some(value) = arg.strip_prefix("--output=") {
            output = Some(value.to_owned());
        } else if arg == "--output" {
            output = Some(next_arg(&mut args, "--output")?);
        } else if let Some(value) = arg.strip_prefix("--samples-per-file=") {
            samples_per_file = parse_usize(value, "--samples-per-file")?;
        } else if arg == "--samples-per-file" {
            samples_per_file = parse_usize(
                &next_arg(&mut args, "--samples-per-file")?,
                "--samples-per-file",
            )?;
        } else if let Some(value) = arg.strip_prefix("--negatives-per-position=") {
            negatives_per_position = parse_usize(value, "--negatives-per-position")?;
        } else if arg == "--negatives-per-position" {
            negatives_per_position = parse_usize(
                &next_arg(&mut args, "--negatives-per-position")?,
                "--negatives-per-position",
            )?;
        } else if let Some(value) = arg.strip_prefix("--plies=") {
            plies = parse_usize(value, "--plies")?;
        } else if arg == "--plies" {
            plies = parse_usize(&next_arg(&mut args, "--plies")?, "--plies")?;
        } else if let Some(value) = arg.strip_prefix("--timeout-ms=") {
            timeout_ms = parse_u64(value, "--timeout-ms")?;
        } else if arg == "--timeout-ms" {
            timeout_ms = parse_u64(&next_arg(&mut args, "--timeout-ms")?, "--timeout-ms")?;
        } else if let Some(value) = arg.strip_prefix("--max-positions=") {
            max_positions = Some(parse_u64(value, "--max-positions")?);
        } else if arg == "--max-positions" {
            max_positions = Some(parse_u64(
                &next_arg(&mut args, "--max-positions")?,
                "--max-positions",
            )?);
        } else if let Some(value) = arg.strip_prefix("--seed=") {
            seed = parse_u64(value, "--seed")?;
        } else if arg == "--seed" {
            seed = parse_u64(&next_arg(&mut args, "--seed")?, "--seed")?;
        } else if arg == "--mirror" {
            mirror = true;
        } else if arg == "--no-mirror" {
            mirror = false;
        } else if let Some(value) = arg.strip_prefix("--root-role=") {
            root_role = parse_role(value)?;
        } else if arg == "--root-role" {
            root_role = parse_role(&next_arg(&mut args, "--root-role")?)?;
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
    }

    let input_dir = input_dir.ok_or("missing --input-dir")?;
    let output = output.ok_or("missing --output")?;
    let mut rng = Rng::new(seed);
    let mut limits = Limits::new(timeout_ms, max_positions);
    let output_file =
        fs::File::create(&output).map_err(|error| format!("write {output}: {error}"))?;
    let mut output_writer = BufWriter::new(output_file);
    let mut total_examples = 0usize;
    let mut skipped_sources = 0usize;

    for file_name in SOURCE_FILES {
        if limits.check() {
            break;
        }
        let path = Path::new(&input_dir).join(file_name);
        let positions = sample_positions(&path, samples_per_file, &mut rng, &mut limits)?;
        eprintln!("{}: sampled {} positions", file_name, positions.len());
        for (processed, source) in positions.iter().enumerate() {
            if limits.check() {
                break;
            }
            let mut df_pn = DfPnTable::new(1 << 18);
            let mut emitted = HashSet::new();
            let Some(position) = parse_source_position(source) else {
                skipped_sources += 1;
                continue;
            };
            let mut current = PositionWrapper::new(position);

            emit_known_position(
                &mut output_writer,
                &mut total_examples,
                &mut emitted,
                &source.source_id,
                &current,
                root_role,
                "identity",
                0,
            )?;
            if mirror && !limits.reached {
                emit_known_mirror(
                    &mut output_writer,
                    &mut total_examples,
                    &mut emitted,
                    &source.source_id,
                    &current,
                    root_role,
                    "mirror",
                    0,
                )?;
            }

            let mut candidate_moves = legal_moves(&current);
            if matches!(root_role, FeatureRole::Attacker) {
                let check_moves = current
                    .all_checks()
                    .into_iter()
                    .map(|mv| mv.to_usi_owned())
                    .collect::<BTreeSet<_>>();
                candidate_moves.retain(|(_, mv)| !check_moves.contains(&mv.to_usi_owned()));
            }
            for _ in 0..negatives_per_position {
                if limits.check() || candidate_moves.is_empty() {
                    break;
                }
                let index = rng.index(candidate_moves.len());
                let (_, mv) = candidate_moves.swap_remove(index);
                let mut child = current.clone();
                child.make_move(mv);
                emit_position(
                    &mut output_writer,
                    &mut total_examples,
                    &mut emitted,
                    &source.source_id,
                    &child,
                    toggled_role(root_role),
                    "replay",
                    1,
                    &mut limits,
                    &mut df_pn,
                )?;
                if mirror && !limits.reached {
                    emit_mirror(
                        &mut output_writer,
                        &mut total_examples,
                        &mut emitted,
                        &source.source_id,
                        &child,
                        toggled_role(root_role),
                        "replay+mirror",
                        1,
                        &mut limits,
                        &mut df_pn,
                    )?;
                }
            }

            for ply_offset in 1..=plies {
                if limits.check() {
                    break;
                }
                let role = role_at_offset(root_role, ply_offset - 1);
                let moves = moves_for_role(&current, role);
                if moves.is_empty() {
                    break;
                }
                let (_, mv) = moves[rng.index(moves.len())];
                current.make_move(mv);
                let role = toggled_role(role);
                emit_position(
                    &mut output_writer,
                    &mut total_examples,
                    &mut emitted,
                    &source.source_id,
                    &current,
                    role,
                    "replay",
                    ply_offset,
                    &mut limits,
                    &mut df_pn,
                )?;
                if mirror && !limits.reached {
                    emit_mirror(
                        &mut output_writer,
                        &mut total_examples,
                        &mut emitted,
                        &source.source_id,
                        &current,
                        role,
                        "replay+mirror",
                        ply_offset,
                        &mut limits,
                        &mut df_pn,
                    )?;
                }
            }
            limits.report_progress(file_name, processed + 1, positions.len(), total_examples);
        }
    }

    output_writer
        .flush()
        .map_err(|error| format!("write {output}: {error}"))?;
    eprintln!(
        "wrote {total_examples} examples; skipped_sources={skipped_sources}; positions_inspected={}",
        limits.positions
    );
    if limits.reached {
        eprintln!("generation limit reached; wrote partial output");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_position(
    output: &mut BufWriter<fs::File>,
    total_examples: &mut usize,
    emitted: &mut HashSet<(String, &'static str)>,
    source_id: &str,
    position: &PositionWrapper,
    role: FeatureRole,
    transform: &'static str,
    ply_offset: usize,
    limits: &mut Limits,
    df_pn: &mut DfPnTable,
) -> Result<(), String> {
    if limits.check() {
        return Ok(());
    }
    let sfen = position.inner().to_sfen_owned();
    let Some(label) = classify(position, role, limits, df_pn) else {
        return Ok(());
    };
    emit_example(
        output,
        total_examples,
        emitted,
        source_id,
        &sfen,
        role,
        transform,
        ply_offset,
        label,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_known_position(
    output: &mut BufWriter<fs::File>,
    total_examples: &mut usize,
    emitted: &mut HashSet<(String, &'static str)>,
    source_id: &str,
    position: &PositionWrapper,
    role: FeatureRole,
    transform: &'static str,
    ply_offset: usize,
) -> Result<(), String> {
    let sfen = position.inner().to_sfen_owned();
    emit_example(
        output,
        total_examples,
        emitted,
        source_id,
        &sfen,
        role,
        transform,
        ply_offset,
        true,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_example(
    output: &mut BufWriter<fs::File>,
    total_examples: &mut usize,
    emitted: &mut HashSet<(String, &'static str)>,
    source_id: &str,
    sfen: &str,
    role: FeatureRole,
    transform: &'static str,
    ply_offset: usize,
    label: bool,
) -> Result<(), String> {
    let Some(role_name) = role_name(role) else {
        return Ok(());
    };
    let key = (sfen.to_owned(), role_name);
    if !emitted.insert(key) {
        return Ok(());
    }
    let id = format!("{source_id}::{transform}::ply{ply_offset}");
    let example = TrainingExample {
        id,
        source_id: source_id.to_owned(),
        sfen: sfen.to_owned(),
        role: role_name,
        label: u8::from(label),
        transform,
        ply_offset,
    };
    let serialized = serde_json::to_string(&example).map_err(|error| error.to_string())?;
    output
        .write_all(serialized.as_bytes())
        .and_then(|_| output.write_all(b"\n"))
        .map_err(|error| error.to_string())?;
    *total_examples += 1;
    if *total_examples % OUTPUT_FLUSH_INTERVAL == 0 {
        output.flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_known_mirror(
    output: &mut BufWriter<fs::File>,
    total_examples: &mut usize,
    emitted: &mut HashSet<(String, &'static str)>,
    source_id: &str,
    position: &PositionWrapper,
    role: FeatureRole,
    transform: &'static str,
    ply_offset: usize,
) -> Result<(), String> {
    let mirrored = mirror_sfen(&position.inner().to_sfen_owned())?;
    let mirrored = PositionWrapper::new(parse_position(&mirrored, source_id)?);
    emit_known_position(
        output,
        total_examples,
        emitted,
        source_id,
        &mirrored,
        role,
        transform,
        ply_offset,
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_mirror(
    output: &mut BufWriter<fs::File>,
    total_examples: &mut usize,
    emitted: &mut HashSet<(String, &'static str)>,
    source_id: &str,
    position: &PositionWrapper,
    role: FeatureRole,
    transform: &'static str,
    ply_offset: usize,
    limits: &mut Limits,
    df_pn: &mut DfPnTable,
) -> Result<(), String> {
    let mirrored = mirror_sfen(&position.inner().to_sfen_owned())?;
    let mirrored = PositionWrapper::new(parse_position(&mirrored, source_id)?);
    emit_position(
        output,
        total_examples,
        emitted,
        source_id,
        &mirrored,
        role,
        transform,
        ply_offset,
        limits,
        df_pn,
    )
}

fn classify(
    position: &PositionWrapper,
    role: FeatureRole,
    limits: &mut Limits,
    df_pn: &mut DfPnTable,
) -> Option<bool> {
    let config = limits.config();
    let node_kind = match role {
        FeatureRole::Attacker => dfpnsearch::NodeKind::Or,
        FeatureRole::Defender => dfpnsearch::NodeKind::And,
        _ => return None,
    };
    let mut stats = dfpnsearch::SearchStats::default();
    let result = dfpnsearch::mid_with_options_and_stats(
        df_pn,
        position,
        (10, 10),
        node_kind,
        true,
        &mut dfpnsearch::SearchCtx::with_config(config),
        false,
        &mut stats,
        &MoveOrderingOptions::default(),
    );
    let search_limit_reached = stats.limit_reached;
    limits.add(&stats);
    if limits.reached || search_limit_reached || (result.0 != 0 && result.1 != 0) {
        None
    } else {
        Some(
            result
                == match role {
                    // DF-PN numbers are relative to the side to move.
                    FeatureRole::Attacker => (0, u32::MAX),
                    FeatureRole::Defender => (u32::MAX, 0),
                    _ => unreachable!(),
                },
        )
    }
}

fn legal_moves(position: &PositionWrapper) -> Vec<(String, Move)> {
    let mut moves = position
        .all_evasions()
        .into_iter()
        .map(|mv| (mv.to_usi_owned(), mv))
        .collect::<Vec<_>>();
    moves.sort_by(|left, right| left.0.cmp(&right.0));
    moves
}

fn moves_for_role(position: &PositionWrapper, role: FeatureRole) -> Vec<(String, Move)> {
    let mut moves = match role {
        FeatureRole::Attacker => position
            .all_checks()
            .into_iter()
            .map(|mv| (mv.to_usi_owned(), mv))
            .collect(),
        FeatureRole::Defender => legal_moves(position),
        _ => Vec::new(),
    };
    moves.sort_by(|left, right| left.0.cmp(&right.0));
    moves
}

fn role_at_offset(root_role: FeatureRole, offset: usize) -> FeatureRole {
    if offset.is_multiple_of(2) {
        root_role
    } else {
        toggled_role(root_role)
    }
}

fn toggled_role(role: FeatureRole) -> FeatureRole {
    match role {
        FeatureRole::Attacker => FeatureRole::Defender,
        FeatureRole::Defender => FeatureRole::Attacker,
        _ => FeatureRole::Defender,
    }
}

fn sample_positions(
    path: &Path,
    sample_count: usize,
    rng: &mut Rng,
    limits: &mut Limits,
) -> Result<Vec<SourcePosition>, String> {
    let file_name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid dataset filename: {}", path.display()))?;
    let file = fs::File::open(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let reader = BufReader::new(file);
    let mut samples = Vec::with_capacity(sample_count);
    for (line_number, line) in reader.lines().enumerate() {
        if limits.check_time() {
            break;
        }
        let line = line.map_err(|error| format!("read {}: {error}", path.display()))?;
        let sfen = line.trim().strip_prefix("sfen ").unwrap_or(line.trim());
        if sfen.is_empty() {
            continue;
        }
        let candidate = SourcePosition {
            source_id: format!("{file_name}-{:08}", line_number + 1),
            sfen: sfen.to_owned(),
        };
        if samples.len() < sample_count {
            samples.push(candidate);
        } else {
            let index = rng.index(line_number + 1);
            if index < sample_count {
                samples[index] = candidate;
            }
        }
        if line_number % 100_000 == 0 {
            limits.report_sampling(&file_name, line_number + 1);
        }
    }
    samples.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    Ok(samples)
}

// Malformed raw records must not discard the rest of a long generation run.
fn parse_source_position(source: &SourcePosition) -> Option<PartialPosition> {
    match parse_position(&source.sfen, &source.source_id) {
        Ok(position) => Some(position),
        Err(error) => {
            eprintln!("warning: skipping source: {error}");
            None
        }
    }
}

fn parse_position(sfen: &str, source_id: &str) -> Result<PartialPosition, String> {
    PartialPosition::from_usi(&format!("sfen {sfen}"))
        .map_err(|error| format!("invalid SFEN for {source_id}: {error:?}"))
}

fn role_name(role: FeatureRole) -> Option<&'static str> {
    match role {
        FeatureRole::Attacker => Some("attacker"),
        FeatureRole::Defender => Some("defender"),
        _ => None,
    }
}

fn parse_role(value: &str) -> Result<FeatureRole, String> {
    match value {
        "attacker" => Ok(FeatureRole::Attacker),
        "defender" => Ok(FeatureRole::Defender),
        _ => Err(format!("invalid --root-role: {value}")),
    }
}

fn mirror_sfen(sfen: &str) -> Result<String, String> {
    let mut fields = sfen.split_whitespace();
    let board = fields.next().ok_or("missing SFEN board")?;
    let side = fields.next().ok_or("missing SFEN side")?;
    let hand = fields.next().ok_or("missing SFEN hand")?;
    let ply = fields.next().ok_or("missing SFEN ply")?;
    let mirrored_board = board
        .split('/')
        .map(mirror_rank)
        .collect::<Result<Vec<_>, _>>()?
        .join("/");
    Ok(format!("{mirrored_board} {side} {hand} {ply}"))
}

fn mirror_rank(rank: &str) -> Result<String, String> {
    let mut cells = Vec::new();
    let mut chars = rank.chars();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            cells.extend(std::iter::repeat_n(None, ch.to_digit(10).unwrap() as usize));
        } else if ch == '+' {
            let piece = chars.next().ok_or("incomplete promoted piece")?;
            cells.push(Some(format!("+{piece}")));
        } else {
            cells.push(Some(ch.to_string()));
        }
    }
    if cells.len() != 9 {
        return Err(format!("SFEN rank has {} squares", cells.len()));
    }
    let mut mirrored = String::new();
    let mut empty = 0;
    for cell in cells.into_iter().rev() {
        match cell {
            Some(piece) => {
                if empty > 0 {
                    mirrored.push_str(&empty.to_string());
                    empty = 0;
                }
                mirrored.push_str(&piece);
            }
            None => empty += 1,
        }
    }
    if empty > 0 {
        mirrored.push_str(&empty.to_string());
    }
    Ok(mirrored)
}

fn next_arg(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_usize(value: &str, flag: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|error| format!("invalid {flag}: {error}"))
}

fn parse_u64(value: &str, flag: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|error| format!("invalid {flag}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn threshold_exit_is_not_a_negative_label() {
        let position = PositionWrapper::new(
            PartialPosition::from_usi(
                "sfen ln1g2k2/2s2R3/p2ppS2p/3b5/2P2p1p1/4n4/P1NPP+l2P/1SG2G3/+r1S1K3L b GNL6Pbp 71",
            )
            .unwrap(),
        );
        let mut table = DfPnTable::new(1 << 16);
        let mut limits = Limits::new(60_000, Some(20_000));
        assert_eq!(
            classify(&position, FeatureRole::Attacker, &mut limits, &mut table),
            None
        );
        let mut stats = dfpnsearch::SearchStats::default();
        let result = dfpnsearch::mid_with_options_and_stats(
            &mut table,
            &position,
            (u32::MAX, u32::MAX),
            dfpnsearch::NodeKind::Or,
            true,
            &mut dfpnsearch::SearchCtx::with_config(SearchConfig::with_max_positions(20_000)),
            false,
            &mut stats,
            &MoveOrderingOptions::default(),
        );
        assert_eq!(result, (0, u32::MAX));
    }

    #[test]
    fn malformed_source_is_skipped_without_losing_following_positions() {
        let valid = "8k/7RG/9/9/9/9/9/9/K8 w - 2";
        // Missing newline joins two records, including the ply and next board.
        let sources = [
            SourcePosition {
                source_id: "mate3-00000001".to_owned(),
                sfen: format!("{valid}{valid}"),
            },
            SourcePosition {
                source_id: "mate3-00000002".to_owned(),
                sfen: valid.to_owned(),
            },
        ];
        assert!(parse_source_position(&sources[0]).is_none());
        let positions: Vec<_> = sources.iter().filter_map(parse_source_position).collect();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0], parse_position(valid, "expected").unwrap());
    }

    #[test]
    fn dfpn_labels_mate_and_escape_for_each_role() {
        for (sfen, role, expected) in [
            ("8k/7R1/7G1/9/9/9/9/9/K8 b - 1", FeatureRole::Attacker, true),
            ("8k/7RG/9/9/9/9/9/9/K8 w - 2", FeatureRole::Defender, true),
            ("8k/9/9/9/9/9/9/9/K8 b - 1", FeatureRole::Attacker, false),
            ("8k/9/9/9/9/9/9/9/K8 w - 1", FeatureRole::Defender, false),
        ] {
            let position =
                PositionWrapper::new(PartialPosition::from_usi(&format!("sfen {sfen}")).unwrap());
            let mut df_pn = DfPnTable::new(1 << 16);
            let mut limits = Limits::new(60_000, Some(10_000));
            assert_eq!(
                classify(&position, role, &mut limits, &mut df_pn),
                Some(expected),
                "{sfen} {role:?}"
            );
        }
    }
}

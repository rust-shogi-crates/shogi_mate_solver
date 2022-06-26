use shogi_core::PartialPosition;

pub mod kif;

#[derive(Copy, Clone, Debug)]
pub enum RecordType {
    Kif,
    Csa,
    Psn,
}

pub fn check_record_type(a: &str) -> RecordType {
    if a.starts_with("#KIF version=") || a.starts_with("# --- Kifu for Windows") {
        return RecordType::Kif;
    }
    RecordType::Psn
}

pub fn parse(a: &str, record_type: RecordType) -> PartialPosition {
    match record_type {
        RecordType::Kif => kif::parse_kif(a),
        _ => todo!(),
    }
}

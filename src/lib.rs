use shogi_core::PartialPosition;

pub mod kif;

#[derive(Copy, Clone, Debug)]
pub enum RecordType {
    Kif,
    Csa,
    Psn,
}

pub fn check_record_type(a: &str) -> RecordType {
    if a.starts_with("#KIF version=") {
        return RecordType::Kif;
    }
    RecordType::Psn
}

pub fn parse(a: &str, record_type: RecordType) -> PartialPosition {
    match record_type {
        RecordType::Kif => parse_kif(a),
        _ => todo!(),
    }
}

pub fn parse_kif(a: &str) -> PartialPosition {
    todo!();
}

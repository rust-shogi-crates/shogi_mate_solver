use core::cmp::max;
use shogi_core::{Color, Hand, PartialPosition, Piece, PieceKind, Square};

fn parse_piece_kind(c: char) -> Option<PieceKind> {
    match c {
        '竜' | '龍' => Some(PieceKind::ProRook),
        '馬' => Some(PieceKind::ProBishop),
        '全' => Some(PieceKind::ProSilver),
        '圭' => Some(PieceKind::ProKnight),
        '杏' => Some(PieceKind::ProLance),
        'と' => Some(PieceKind::ProPawn),
        '玉' => Some(PieceKind::King),
        '飛' => Some(PieceKind::Rook),
        '角' => Some(PieceKind::Bishop),
        '金' => Some(PieceKind::Gold),
        '銀' => Some(PieceKind::Silver),
        '桂' => Some(PieceKind::Knight),
        '香' => Some(PieceKind::Lance),
        '歩' => Some(PieceKind::Pawn),
        _ => None,
    }
}

fn parse_kansuji(c: char) -> Option<u8> {
    match c {
        '十' => Some(10),
        '一' => Some(1),
        '二' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}

fn add(hand: &mut Hand, piece_kind: PieceKind, count: u8) {
    for _ in 0..count {
        *hand = hand.added(piece_kind).unwrap();
    }
}

fn parse_hand(a: &str) -> Hand {
    let mut hand = Hand::new();
    let mut count = 0;
    for c in a.chars().rev() {
        match c {
            '飛' => add(&mut hand, PieceKind::Rook, max(count, 1)),
            '角' => add(&mut hand, PieceKind::Bishop, max(count, 1)),
            '金' => add(&mut hand, PieceKind::Gold, max(count, 1)),
            '銀' => add(&mut hand, PieceKind::Silver, max(count, 1)),
            '桂' => add(&mut hand, PieceKind::Knight, max(count, 1)),
            '香' => add(&mut hand, PieceKind::Lance, max(count, 1)),
            '歩' => add(&mut hand, PieceKind::Pawn, max(count, 1)),
            '　' => count = 0,
            _ => {}
        }
        if let Some(num) = parse_kansuji(c) {
            count += num;
        }
    }
    hand
}

pub fn parse_kif(a: &str) -> PartialPosition {
    let mut position = PartialPosition::empty();
    for line in a.split("\n") {
        if let Some(rest) = line.strip_prefix("先手の持駒：") {
            *position.hand_of_a_player_mut(Color::Black) = parse_hand(rest);
        }
        if let Some(rest) = line.strip_prefix("後手の持駒：") {
            *position.hand_of_a_player_mut(Color::White) = parse_hand(rest);
        }
        if let Some(rest) = line.strip_prefix("|") {
            let mut v = Vec::new();
            let mut color = Color::Black;
            for c in rest.chars() {
                if let Some(piece_kind) = parse_piece_kind(c) {
                    v.push(Some(Piece::new(piece_kind, color)));
                }
                if c == ' ' {
                    color = Color::Black;
                }
                if c == 'v' {
                    color = Color::White;
                }
                if c == '・' {
                    v.push(None);
                }
                if let Some(num) = parse_kansuji(c) {
                    for i in 0..9 {
                        let square = Square::new(9 - i, num).unwrap();
                        position.piece_set(square, v[i as usize]);
                    }
                }
            }
        }
    }
    position
}

#[cfg(test)]
mod tests {
    use super::*;
    use shogi_usi_parser::FromUsi;

    #[test]
    fn parse_hand_works() {
        let hands = <[Hand; 2]>::from_usi("S2r4gs3n3l15p").unwrap();
        assert_eq!(parse_hand("銀"), hands[0]);
        assert_eq!(parse_hand("飛二　金四　銀　桂三　香三　歩十五"), hands[1]);
    }
}

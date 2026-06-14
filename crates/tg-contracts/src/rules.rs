use rust_decimal::Decimal;

use crate::Board;

pub const LOT_SIZE: i64 = 100;
pub const STAMP_DUTY_PCT: Decimal = Decimal::from_parts(5, 0, 0, false, 4);
pub const COMMISSION_MAX_PCT: Decimal = Decimal::from_parts(3, 0, 0, false, 4);
pub const TRANSFER_FEE_PCT: Decimal = Decimal::from_parts(1, 0, 0, false, 5);

pub fn limit_up_pct(board: Board) -> Decimal {
    match board {
        Board::MainBoard => Decimal::new(10, 2),
        Board::Star | Board::ChiNext => Decimal::new(20, 2),
        Board::Bj => Decimal::new(30, 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(value: i64, scale: u32) -> Decimal {
        Decimal::new(value, scale)
    }

    #[test]
    fn limit_up_pct_matches_a_share_boards() {
        assert_eq!(limit_up_pct(Board::MainBoard), dec(10, 2));
        assert_eq!(limit_up_pct(Board::Star), dec(20, 2));
        assert_eq!(limit_up_pct(Board::ChiNext), dec(20, 2));
        assert_eq!(limit_up_pct(Board::Bj), dec(30, 2));
    }
}

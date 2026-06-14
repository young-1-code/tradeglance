use chrono::{DateTime, NaiveTime, Utc};
use chrono_tz::Asia::Shanghai;

pub fn is_continuous_auction(ts: DateTime<Utc>) -> bool {
    let cst = ts.with_timezone(&Shanghai);
    let time = cst.time();
    in_closed_range(time, hms(9, 30, 0), hms(11, 30, 0))
        || in_closed_range(time, hms(13, 0, 0), hms(15, 0, 0))
}

pub fn is_call_auction(ts: DateTime<Utc>) -> bool {
    let cst = ts.with_timezone(&Shanghai);
    let time = cst.time();
    in_closed_range(time, hms(9, 15, 0), hms(9, 25, 0))
        || in_closed_range(time, hms(14, 57, 0), hms(15, 0, 0))
}

fn in_closed_range(time: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    time >= start && time <= end
}

fn hms(hour: u32, minute: u32, second: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(hour, minute, second).expect("valid A-share session time")
}

#[cfg(test)]
mod tests {
    use chrono::{LocalResult, TimeZone};

    use super::*;

    fn cst(hour: u32, minute: u32) -> DateTime<Utc> {
        match Shanghai.with_ymd_and_hms(2026, 6, 15, hour, minute, 0) {
            LocalResult::Single(ts) => ts.with_timezone(&Utc),
            _ => panic!("invalid CST test timestamp"),
        }
    }

    #[test]
    fn call_auction_boundaries_are_cst() {
        assert!(is_call_auction(cst(9, 20)));
        assert!(is_call_auction(cst(14, 58)));
        assert!(!is_call_auction(cst(9, 30)));
        assert!(!is_call_auction(cst(11, 0)));
        assert!(!is_call_auction(cst(14, 0)));
    }

    #[test]
    fn continuous_auction_boundaries_are_cst() {
        assert!(is_continuous_auction(cst(10, 0)));
        assert!(is_continuous_auction(cst(14, 0)));
        assert!(!is_continuous_auction(cst(9, 20)));
        assert!(!is_continuous_auction(cst(12, 0)));
        assert!(!is_continuous_auction(cst(15, 1)));
    }
}

pub mod tg {
    pub mod v1 {
        tonic::include_proto!("tg.v1");
    }
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::tg::v1::{Bar, BarPeriod, Exchange};

    #[test]
    fn proto_bar_round_trips_bytes() {
        let bar = Bar {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh as i32,
            period: BarPeriod::Daily as i32,
            ts_epoch_millis: 1_797_249_600_000,
            trading_date: "2026-12-14".to_owned(),
            open: "1500.00".to_owned(),
            high: "1510.00".to_owned(),
            low: "1490.00".to_owned(),
            close: "1505.00".to_owned(),
            volume: 10_000,
            amount: "15050000.00".to_owned(),
        };

        let mut buf = Vec::new();
        bar.encode(&mut buf).expect("encode proto bar");
        let decoded = Bar::decode(buf.as_slice()).expect("decode proto bar");
        assert_eq!(decoded, bar);
    }
}

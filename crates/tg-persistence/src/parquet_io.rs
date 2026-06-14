use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use arrow::array::{ArrayRef, Decimal128Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use tg_contracts::{Bar, BarPeriod, BarQuery, Result, Snapshot, TgError};

use crate::model::{
    exchange_from_str, exchange_to_str, fixed_5, invalid_data, other_error, period_from_str,
    period_to_str,
};

const DECIMAL_PRECISION: u8 = 18;
const DECIMAL_SCALE: i8 = 4;
const DECIMAL_FACTOR: i128 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BarPartition {
    pub period: BarPeriod,
    pub symbol: String,
    pub year: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotPartition {
    pub symbol: String,
    pub date: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct ParquetStore {
    root: PathBuf,
}

impl ParquetStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn write_bars(&self, bars: &[Bar]) -> Result<()> {
        let root = self.root.clone();
        let bars = bars.to_vec();
        tokio::task::spawn_blocking(move || write_bars_sync(&root, &bars))
            .await
            .map_err(other_error)?
    }

    pub async fn query_bars(&self, query: &BarQuery) -> Result<Vec<Bar>> {
        let root = self.root.clone();
        let query = query.clone();
        tokio::task::spawn_blocking(move || query_bars_sync(&root, &query))
            .await
            .map_err(other_error)?
    }

    pub async fn write_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        let root = self.root.clone();
        let snapshot = snapshot.clone();
        tokio::task::spawn_blocking(move || write_snapshot_sync(&root, &snapshot))
            .await
            .map_err(other_error)?
    }

    pub async fn read_snapshots(&self, symbol: &str, date: NaiveDate) -> Result<Vec<Snapshot>> {
        let root = self.root.clone();
        let symbol = symbol.to_owned();
        tokio::task::spawn_blocking(move || {
            let partition = SnapshotPartition { symbol, date };
            read_snapshot_partition(&snapshot_partition_path(&root, &partition))
        })
        .await
        .map_err(other_error)?
    }
}

pub fn bar_partition_path(root: &Path, partition: &BarPartition) -> PathBuf {
    root.join("data")
        .join("bars")
        .join(period_to_str(partition.period))
        .join(format!("symbol={}", partition.symbol))
        .join(format!("year={}", partition.year))
        .join("part.parquet")
}

pub fn snapshot_partition_path(root: &Path, partition: &SnapshotPartition) -> PathBuf {
    root.join("data")
        .join("snapshots")
        .join(format!("symbol={}", partition.symbol))
        .join(format!("date={}", partition.date.format("%Y%m%d")))
        .join("part.parquet")
}

fn write_bars_sync(root: &Path, bars: &[Bar]) -> Result<()> {
    let mut by_partition: HashMap<BarPartition, Vec<Bar>> = HashMap::new();
    for bar in bars {
        by_partition
            .entry(BarPartition {
                period: bar.period,
                symbol: bar.symbol.clone(),
                year: bar.trading_date.year(),
            })
            .or_default()
            .push(bar.clone());
    }

    for (partition, mut new_bars) in by_partition {
        let path = bar_partition_path(root, &partition);
        let mut bars = read_bar_partition(&path)?;
        bars.append(&mut new_bars);
        bars.sort_by_key(|bar| bar.ts);
        bars.dedup_by(|left, right| {
            left.symbol == right.symbol && left.period == right.period && left.ts == right.ts
        });
        write_bar_partition_atomic(&path, &bars)?;
    }

    Ok(())
}

// TODO(duckdb): swap read path to DuckDB when SQL-over-parquet is needed.
fn query_bars_sync(root: &Path, query: &BarQuery) -> Result<Vec<Bar>> {
    let mut bars = Vec::new();
    for year in query.range.start.year()..=query.range.end.year() {
        let partition = BarPartition {
            period: query.period,
            symbol: query.symbol.clone(),
            year,
        };
        let path = bar_partition_path(root, &partition);
        let mut partition_bars = read_bar_partition(&path)?;
        bars.append(&mut partition_bars);
    }

    bars.retain(|bar| {
        bar.symbol == query.symbol
            && bar.period == query.period
            && bar.ts >= query.range.start
            && bar.ts < query.range.end
    });
    bars.sort_by_key(|bar| bar.ts);
    Ok(bars)
}

fn write_snapshot_sync(root: &Path, snapshot: &Snapshot) -> Result<()> {
    let partition = SnapshotPartition {
        symbol: snapshot.symbol.clone(),
        date: snapshot.trading_date,
    };
    let path = snapshot_partition_path(root, &partition);
    let mut snapshots = read_snapshot_partition(&path)?;
    snapshots.push(snapshot.clone());
    snapshots.sort_by_key(|snapshot| snapshot.ts);
    snapshots.dedup_by(|left, right| left.symbol == right.symbol && left.ts == right.ts);
    write_snapshot_partition_atomic(&path, &snapshots)
}

fn read_bar_partition(path: &Path) -> Result<Vec<Bar>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(other_error)?
        .with_batch_size(8192)
        .build()
        .map_err(other_error)?;

    let mut bars = Vec::new();
    for batch in reader {
        bars.extend(bars_from_batch(&batch.map_err(other_error)?)?);
    }
    Ok(bars)
}

fn read_snapshot_partition(path: &Path) -> Result<Vec<Snapshot>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(other_error)?
        .with_batch_size(8192)
        .build()
        .map_err(other_error)?;

    let mut snapshots = Vec::new();
    for batch in reader {
        snapshots.extend(snapshots_from_batch(&batch.map_err(other_error)?)?);
    }
    Ok(snapshots)
}

fn write_bar_partition_atomic(path: &Path, bars: &[Bar]) -> Result<()> {
    write_batch_atomic(path, bar_batch(bars)?)
}

fn write_snapshot_partition_atomic(path: &Path, snapshots: &[Snapshot]) -> Result<()> {
    write_batch_atomic(path, snapshot_batch(snapshots)?)
}

fn write_batch_atomic(path: &Path, batch: RecordBatch) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_data(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;

    let temp_path = parent.join(format!(
        ".part.{}.{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(other_error)?
            .as_nanos()
    ));

    let write_result = (|| -> Result<()> {
        let file =
            File::create(&temp_path).with_context(|| format!("create {}", temp_path.display()))?;
        let mut writer = ArrowWriter::try_new(file, batch.schema(), None).map_err(other_error)?;
        writer.write(&batch).map_err(other_error)?;
        writer.close().map_err(other_error)?;
        fs::rename(&temp_path, path)
            .with_context(|| format!("rename {} to {}", temp_path.display(), path.display()))?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result
}

fn bar_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("symbol", DataType::Utf8, false),
        Field::new("exchange", DataType::Utf8, false),
        Field::new("period", DataType::Utf8, false),
        Field::new("ts_ms", DataType::Int64, false),
        Field::new("trading_date", DataType::Utf8, false),
        decimal_field("open"),
        decimal_field("high"),
        decimal_field("low"),
        decimal_field("close"),
        Field::new("volume", DataType::Int64, false),
        decimal_field("amount"),
    ]))
}

fn snapshot_schema() -> Arc<Schema> {
    let mut fields = vec![
        Field::new("symbol", DataType::Utf8, false),
        Field::new("exchange", DataType::Utf8, false),
        Field::new("ts_ms", DataType::Int64, false),
        Field::new("trading_date", DataType::Utf8, false),
        decimal_field("last"),
        decimal_field("open"),
        decimal_field("high"),
        decimal_field("low"),
        decimal_field("pre_close"),
        Field::new("volume", DataType::Int64, false),
        decimal_field("amount"),
    ];

    for side in ["bid", "ask"] {
        for level in 1..=5 {
            fields.push(decimal_field(&format!("{side}_price_{level}")));
            fields.push(Field::new(
                format!("{side}_volume_{level}"),
                DataType::Int64,
                false,
            ));
        }
    }

    Arc::new(Schema::new(fields))
}

fn decimal_field(name: &str) -> Field {
    Field::new(
        name,
        DataType::Decimal128(DECIMAL_PRECISION, DECIMAL_SCALE),
        false,
    )
}

fn bar_batch(bars: &[Bar]) -> Result<RecordBatch> {
    let schema = bar_schema();
    RecordBatch::try_new(
        schema,
        vec![
            string_array(bars.iter().map(|bar| bar.symbol.clone())),
            string_array(
                bars.iter()
                    .map(|bar| exchange_to_str(bar.exchange).to_owned()),
            ),
            string_array(bars.iter().map(|bar| period_to_str(bar.period).to_owned())),
            int64_array(bars.iter().map(|bar| bar.ts.timestamp_millis())),
            string_array(bars.iter().map(|bar| bar.trading_date.to_string())),
            decimal_array(bars.iter().map(|bar| bar.open))?,
            decimal_array(bars.iter().map(|bar| bar.high))?,
            decimal_array(bars.iter().map(|bar| bar.low))?,
            decimal_array(bars.iter().map(|bar| bar.close))?,
            int64_array(bars.iter().map(|bar| bar.volume)),
            decimal_array(bars.iter().map(|bar| bar.amount))?,
        ],
    )
    .map_err(other_error)
}

fn snapshot_batch(snapshots: &[Snapshot]) -> Result<RecordBatch> {
    let schema = snapshot_schema();
    let mut columns = vec![
        string_array(snapshots.iter().map(|snapshot| snapshot.symbol.clone())),
        string_array(
            snapshots
                .iter()
                .map(|snapshot| exchange_to_str(snapshot.exchange).to_owned()),
        ),
        int64_array(
            snapshots
                .iter()
                .map(|snapshot| snapshot.ts.timestamp_millis()),
        ),
        string_array(
            snapshots
                .iter()
                .map(|snapshot| snapshot.trading_date.to_string()),
        ),
        decimal_array(snapshots.iter().map(|snapshot| snapshot.last))?,
        decimal_array(snapshots.iter().map(|snapshot| snapshot.open))?,
        decimal_array(snapshots.iter().map(|snapshot| snapshot.high))?,
        decimal_array(snapshots.iter().map(|snapshot| snapshot.low))?,
        decimal_array(snapshots.iter().map(|snapshot| snapshot.pre_close))?,
        int64_array(snapshots.iter().map(|snapshot| snapshot.volume)),
        decimal_array(snapshots.iter().map(|snapshot| snapshot.amount))?,
    ];

    for side in [BookSide::Bid, BookSide::Ask] {
        for level in 0..5 {
            columns.push(decimal_array(snapshots.iter().map(
                |snapshot| match side {
                    BookSide::Bid => snapshot.bid_price[level],
                    BookSide::Ask => snapshot.ask_price[level],
                },
            ))?);
            columns.push(int64_array(snapshots.iter().map(|snapshot| match side {
                BookSide::Bid => snapshot.bid_volume[level],
                BookSide::Ask => snapshot.ask_volume[level],
            })));
        }
    }

    RecordBatch::try_new(schema, columns).map_err(other_error)
}

#[derive(Clone, Copy)]
enum BookSide {
    Bid,
    Ask,
}

fn string_array(values: impl Iterator<Item = String>) -> ArrayRef {
    Arc::new(StringArray::from(values.collect::<Vec<_>>()))
}

fn int64_array(values: impl Iterator<Item = i64>) -> ArrayRef {
    Arc::new(Int64Array::from(values.collect::<Vec<_>>()))
}

fn decimal_array(values: impl Iterator<Item = Decimal>) -> Result<ArrayRef> {
    let values = values.map(decimal_to_i128).collect::<Result<Vec<_>>>()?;
    let array = Decimal128Array::from(values)
        .with_precision_and_scale(DECIMAL_PRECISION, DECIMAL_SCALE)
        .map_err(other_error)?;
    Ok(Arc::new(array))
}

fn decimal_to_i128(value: Decimal) -> Result<i128> {
    (value * Decimal::from(DECIMAL_FACTOR))
        .round()
        .to_i128()
        .ok_or_else(|| TgError::Validation(format!("decimal out of range: {value}")))
}

fn decimal_from_i128(value: i128) -> Decimal {
    Decimal::from_i128_with_scale(value, DECIMAL_SCALE as u32)
}

fn bars_from_batch(batch: &RecordBatch) -> Result<Vec<Bar>> {
    let symbol = string_column(batch, "symbol")?;
    let exchange = string_column(batch, "exchange")?;
    let period = string_column(batch, "period")?;
    let ts_ms = int64_column(batch, "ts_ms")?;
    let trading_date = string_column(batch, "trading_date")?;
    let open = decimal_column(batch, "open")?;
    let high = decimal_column(batch, "high")?;
    let low = decimal_column(batch, "low")?;
    let close = decimal_column(batch, "close")?;
    let volume = int64_column(batch, "volume")?;
    let amount = decimal_column(batch, "amount")?;

    let mut bars = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        bars.push(Bar {
            symbol: symbol.value(row).to_owned(),
            exchange: exchange_from_str(exchange.value(row))?,
            period: period_from_str(period.value(row))?,
            ts: utc_from_ms(ts_ms.value(row))?,
            trading_date: NaiveDate::parse_from_str(trading_date.value(row), "%Y-%m-%d")
                .map_err(other_error)?,
            open: decimal_from_i128(open.value(row)),
            high: decimal_from_i128(high.value(row)),
            low: decimal_from_i128(low.value(row)),
            close: decimal_from_i128(close.value(row)),
            volume: volume.value(row),
            amount: decimal_from_i128(amount.value(row)),
        });
    }
    Ok(bars)
}

fn snapshots_from_batch(batch: &RecordBatch) -> Result<Vec<Snapshot>> {
    let symbol = string_column(batch, "symbol")?;
    let exchange = string_column(batch, "exchange")?;
    let ts_ms = int64_column(batch, "ts_ms")?;
    let trading_date = string_column(batch, "trading_date")?;
    let last = decimal_column(batch, "last")?;
    let open = decimal_column(batch, "open")?;
    let high = decimal_column(batch, "high")?;
    let low = decimal_column(batch, "low")?;
    let pre_close = decimal_column(batch, "pre_close")?;
    let volume = int64_column(batch, "volume")?;
    let amount = decimal_column(batch, "amount")?;

    let bid_price = decimal_level_columns(batch, "bid_price")?;
    let bid_volume = int64_level_columns(batch, "bid_volume")?;
    let ask_price = decimal_level_columns(batch, "ask_price")?;
    let ask_volume = int64_level_columns(batch, "ask_volume")?;

    let mut snapshots = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        snapshots.push(Snapshot {
            symbol: symbol.value(row).to_owned(),
            exchange: exchange_from_str(exchange.value(row))?,
            ts: utc_from_ms(ts_ms.value(row))?,
            trading_date: NaiveDate::parse_from_str(trading_date.value(row), "%Y-%m-%d")
                .map_err(other_error)?,
            last: decimal_from_i128(last.value(row)),
            open: decimal_from_i128(open.value(row)),
            high: decimal_from_i128(high.value(row)),
            low: decimal_from_i128(low.value(row)),
            pre_close: decimal_from_i128(pre_close.value(row)),
            volume: volume.value(row),
            amount: decimal_from_i128(amount.value(row)),
            bid_price: decimal_levels_at(&bid_price, row),
            bid_volume: int64_levels_at(&bid_volume, row),
            ask_price: decimal_levels_at(&ask_price, row),
            ask_volume: int64_levels_at(&ask_volume, row),
        });
    }
    Ok(snapshots)
}

fn string_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    let index = batch.schema().index_of(name).map_err(other_error)?;
    batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| invalid_data(format!("{name} is not a StringArray")))
}

fn int64_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a Int64Array> {
    let index = batch.schema().index_of(name).map_err(other_error)?;
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| invalid_data(format!("{name} is not an Int64Array")))
}

fn decimal_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a Decimal128Array> {
    let index = batch.schema().index_of(name).map_err(other_error)?;
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .ok_or_else(|| invalid_data(format!("{name} is not a Decimal128Array")))
}

fn decimal_level_columns<'a>(
    batch: &'a RecordBatch,
    prefix: &str,
) -> Result<[&'a Decimal128Array; 5]> {
    let columns = (1..=5)
        .map(|level| decimal_column(batch, &format!("{prefix}_{level}")))
        .collect::<Result<Vec<_>>>()?;
    fixed_5(&columns, prefix)
}

fn int64_level_columns<'a>(batch: &'a RecordBatch, prefix: &str) -> Result<[&'a Int64Array; 5]> {
    let columns = (1..=5)
        .map(|level| int64_column(batch, &format!("{prefix}_{level}")))
        .collect::<Result<Vec<_>>>()?;
    fixed_5(&columns, prefix)
}

fn decimal_levels_at(columns: &[&Decimal128Array; 5], row: usize) -> [Decimal; 5] {
    [
        decimal_from_i128(columns[0].value(row)),
        decimal_from_i128(columns[1].value(row)),
        decimal_from_i128(columns[2].value(row)),
        decimal_from_i128(columns[3].value(row)),
        decimal_from_i128(columns[4].value(row)),
    ]
}

fn int64_levels_at(columns: &[&Int64Array; 5], row: usize) -> [i64; 5] {
    [
        columns[0].value(row),
        columns[1].value(row),
        columns[2].value(row),
        columns[3].value(row),
        columns[4].value(row),
    ]
}

fn utc_from_ms(value: i64) -> Result<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .ok_or_else(|| TgError::Validation(format!("invalid timestamp millis: {value}")))
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{Adjustment, Bar, BarPeriod, BarQuery, Exchange, Snapshot};

    use super::{
        bar_partition_path, snapshot_partition_path, BarPartition, ParquetStore, SnapshotPartition,
    };

    fn dec(value: i64, scale: u32) -> Decimal {
        Decimal::new(value, scale)
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    fn bar() -> Bar {
        Bar {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            period: BarPeriod::Daily,
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 7, 0, 0).unwrap(),
            trading_date: date(2026, 6, 15),
            open: dec(123_4567, 4),
            high: dec(124_0001, 4),
            low: dec(122_9900, 4),
            close: dec(123_9000, 4),
            volume: 12_300,
            amount: dec(1_523_970_000, 4),
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 1).unwrap(),
            trading_date: date(2026, 6, 15),
            last: dec(123_4567, 4),
            open: dec(123_0000, 4),
            high: dec(124_0000, 4),
            low: dec(122_0000, 4),
            pre_close: dec(121_5000, 4),
            volume: 9_900,
            amount: dec(1_222_220_000, 4),
            bid_price: [
                dec(123_4500, 4),
                dec(123_4400, 4),
                dec(123_4300, 4),
                dec(123_4200, 4),
                dec(123_4100, 4),
            ],
            bid_volume: [100, 200, 300, 400, 500],
            ask_price: [
                dec(123_4600, 4),
                dec(123_4700, 4),
                dec(123_4800, 4),
                dec(123_4900, 4),
                dec(123_5000, 4),
            ],
            ask_volume: [150, 250, 350, 450, 550],
        }
    }

    #[tokio::test]
    async fn bars_round_trip_preserves_decimal_precision() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = ParquetStore::new(temp.path());
        let bar = bar();
        store
            .write_bars(std::slice::from_ref(&bar))
            .await
            .expect("write bars");

        let actual = store
            .query_bars(&BarQuery {
                symbol: "600519".to_owned(),
                period: BarPeriod::Daily,
                range: Utc.with_ymd_and_hms(2026, 6, 15, 0, 0, 0).unwrap()
                    ..Utc.with_ymd_and_hms(2026, 6, 16, 0, 0, 0).unwrap(),
                adjustment: Adjustment::None,
            })
            .await
            .expect("query bars");

        assert_eq!(actual, vec![bar]);
    }

    #[tokio::test]
    async fn snapshots_round_trip_preserves_decimal_precision() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = ParquetStore::new(temp.path());
        let snapshot = snapshot();
        store
            .write_snapshot(&snapshot)
            .await
            .expect("write snapshot");

        let actual = store
            .read_snapshots("600519", date(2026, 6, 15))
            .await
            .expect("read snapshots");

        assert_eq!(actual, vec![snapshot]);
    }

    #[test]
    fn partition_paths_match_spec_layout() {
        let root = std::path::Path::new("/tmp/tg");
        assert_eq!(
            bar_partition_path(
                root,
                &BarPartition {
                    period: BarPeriod::Min1,
                    symbol: "000001".to_owned(),
                    year: 2026,
                },
            ),
            root.join("data/bars/minute1/symbol=000001/year=2026/part.parquet")
        );
        assert_eq!(
            snapshot_partition_path(
                root,
                &SnapshotPartition {
                    symbol: "000001".to_owned(),
                    date: date(2026, 6, 15),
                },
            ),
            root.join("data/snapshots/symbol=000001/date=20260615/part.parquet")
        );
    }
}

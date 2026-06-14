use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use arrow::array::{Array, ArrayRef, Float64Array, Int64Array, StringArray, UInt32Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use chrono::{DateTime, NaiveDate, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use tg_contracts::FactorValue;

use crate::error::{FactorError, Result};

#[derive(Debug, Clone)]
pub struct FactorValueStore {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FactorPartition {
    pub factor: String,
    pub date: NaiveDate,
}

impl FactorValueStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub async fn write_values(&self, values: &[FactorValue]) -> Result<()> {
        let root = self.root.clone();
        let values = values.to_vec();
        tokio::task::spawn_blocking(move || write_values_sync(&root, &values))
            .await
            .map_err(|error| FactorError::Other(anyhow::anyhow!(error)))?
    }

    pub async fn query_values(
        &self,
        factor: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        symbols: &[String],
    ) -> Result<Vec<FactorValue>> {
        let root = self.root.clone();
        let factor = factor.to_owned();
        let symbols = symbols.to_vec();
        tokio::task::spawn_blocking(move || query_values_sync(&root, &factor, start, end, &symbols))
            .await
            .map_err(|error| FactorError::Other(anyhow::anyhow!(error)))?
    }

    pub async fn read_partition(&self, factor: &str, date: NaiveDate) -> Result<Vec<FactorValue>> {
        let path = factor_partition_path(
            &self.root,
            &FactorPartition {
                factor: factor.to_owned(),
                date,
            },
        );
        tokio::task::spawn_blocking(move || read_partition(&path))
            .await
            .map_err(|error| FactorError::Other(anyhow::anyhow!(error)))?
    }
}

pub fn factor_partition_path(root: &Path, partition: &FactorPartition) -> PathBuf {
    root.join("data")
        .join("factors")
        .join(format!("factor={}", partition.factor))
        .join(format!("date={}", partition.date.format("%Y%m%d")))
        .join("part.parquet")
}

fn write_values_sync(root: &Path, values: &[FactorValue]) -> Result<()> {
    let mut by_partition: HashMap<FactorPartition, Vec<FactorValue>> = HashMap::new();
    for value in values {
        by_partition
            .entry(FactorPartition {
                factor: value.factor.clone(),
                date: value.trading_date,
            })
            .or_default()
            .push(value.clone());
    }

    for (partition, mut new_values) in by_partition {
        let path = factor_partition_path(root, &partition);
        let mut existing = read_partition(&path)?;
        existing.append(&mut new_values);
        existing.sort_by(|left, right| {
            left.symbol
                .cmp(&right.symbol)
                .then_with(|| left.ts.cmp(&right.ts))
        });
        existing.dedup_by(|left, right| {
            left.symbol == right.symbol && left.factor == right.factor && left.ts == right.ts
        });
        write_partition_atomic(&path, &existing)?;
    }
    Ok(())
}

fn query_values_sync(
    root: &Path,
    factor: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    symbols: &[String],
) -> Result<Vec<FactorValue>> {
    let mut values = Vec::new();
    let mut date = start.date_naive();
    let end_date = end.date_naive();
    while date <= end_date {
        let path = factor_partition_path(
            root,
            &FactorPartition {
                factor: factor.to_owned(),
                date,
            },
        );
        values.extend(read_partition(&path)?);
        date = date
            .succ_opt()
            .ok_or_else(|| FactorError::InvalidInput("date overflow".to_owned()))?;
    }

    values.retain(|value| {
        value.factor == factor
            && value.ts >= start
            && value.ts < end
            && (symbols.is_empty() || symbols.contains(&value.symbol))
    });
    values.sort_by(|left, right| {
        left.ts
            .cmp(&right.ts)
            .then_with(|| left.symbol.cmp(&right.symbol))
    });
    Ok(values)
}

fn write_partition_atomic(path: &Path, values: &[FactorValue]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| FactorError::Storage(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;

    let temp_path = parent.join(format!(
        ".part.{}.{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| FactorError::Other(anyhow::anyhow!(error)))?
            .as_nanos()
    ));

    let write_result = (|| -> Result<()> {
        let file =
            File::create(&temp_path).with_context(|| format!("create {}", temp_path.display()))?;
        let batch = value_batch(values)?;
        let mut writer = ArrowWriter::try_new(file, batch.schema(), None)
            .map_err(|error| FactorError::Storage(error.to_string()))?;
        writer
            .write(&batch)
            .map_err(|error| FactorError::Storage(error.to_string()))?;
        writer
            .close()
            .map_err(|error| FactorError::Storage(error.to_string()))?;
        fs::rename(&temp_path, path)
            .with_context(|| format!("rename {} to {}", temp_path.display(), path.display()))?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn read_partition(path: &Path) -> Result<Vec<FactorValue>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|error| FactorError::Storage(error.to_string()))?
        .with_batch_size(8192)
        .build()
        .map_err(|error| FactorError::Storage(error.to_string()))?;
    let mut values = Vec::new();
    for batch in reader {
        values.extend(values_from_batch(
            &batch.map_err(|error| FactorError::Storage(error.to_string()))?,
        )?);
    }
    Ok(values)
}

fn value_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("symbol", DataType::Utf8, false),
        Field::new("factor", DataType::Utf8, false),
        Field::new("ts_ms", DataType::Int64, false),
        Field::new("trading_date", DataType::Utf8, false),
        Field::new("value", DataType::Float64, false),
        Field::new("rank", DataType::UInt32, true),
    ]))
}

fn value_batch(values: &[FactorValue]) -> Result<RecordBatch> {
    let schema = value_schema();
    let ranks = values
        .iter()
        .map(|value| value.rank)
        .collect::<Vec<Option<u32>>>();
    RecordBatch::try_new(
        schema,
        vec![
            string_array(values.iter().map(|value| value.symbol.clone())),
            string_array(values.iter().map(|value| value.factor.clone())),
            int64_array(values.iter().map(|value| value.ts.timestamp_millis())),
            string_array(values.iter().map(|value| value.trading_date.to_string())),
            float64_array(values.iter().map(|value| value.value)),
            Arc::new(UInt32Array::from(ranks)) as ArrayRef,
        ],
    )
    .map_err(|error| FactorError::Storage(error.to_string()))
}

fn values_from_batch(batch: &RecordBatch) -> Result<Vec<FactorValue>> {
    let symbol = string_column(batch, "symbol")?;
    let factor = string_column(batch, "factor")?;
    let ts_ms = int64_column(batch, "ts_ms")?;
    let trading_date = string_column(batch, "trading_date")?;
    let value = float64_column(batch, "value")?;
    let rank = uint32_column(batch, "rank")?;

    let mut values = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        values.push(FactorValue {
            symbol: symbol.value(row).to_owned(),
            factor: factor.value(row).to_owned(),
            ts: DateTime::<Utc>::from_timestamp_millis(ts_ms.value(row)).ok_or_else(|| {
                FactorError::InvalidInput(format!("invalid timestamp millis: {}", ts_ms.value(row)))
            })?,
            trading_date: NaiveDate::parse_from_str(trading_date.value(row), "%Y-%m-%d")
                .map_err(|error| FactorError::InvalidInput(error.to_string()))?,
            value: value.value(row),
            rank: if rank.is_null(row) {
                None
            } else {
                Some(rank.value(row))
            },
        });
    }
    Ok(values)
}

fn string_array(values: impl Iterator<Item = String>) -> ArrayRef {
    Arc::new(StringArray::from(values.collect::<Vec<_>>()))
}

fn int64_array(values: impl Iterator<Item = i64>) -> ArrayRef {
    Arc::new(Int64Array::from(values.collect::<Vec<_>>()))
}

fn float64_array(values: impl Iterator<Item = f64>) -> ArrayRef {
    Arc::new(Float64Array::from(values.collect::<Vec<_>>()))
}

fn string_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    let index = batch
        .schema()
        .index_of(name)
        .map_err(|error| FactorError::Storage(error.to_string()))?;
    batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| FactorError::Storage(format!("{name} is not a StringArray")))
}

fn int64_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a Int64Array> {
    let index = batch
        .schema()
        .index_of(name)
        .map_err(|error| FactorError::Storage(error.to_string()))?;
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Int64Array>()
        .ok_or_else(|| FactorError::Storage(format!("{name} is not an Int64Array")))
}

fn float64_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a Float64Array> {
    let index = batch
        .schema()
        .index_of(name)
        .map_err(|error| FactorError::Storage(error.to_string()))?;
    batch
        .column(index)
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| FactorError::Storage(format!("{name} is not a Float64Array")))
}

fn uint32_column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a UInt32Array> {
    let index = batch
        .schema()
        .index_of(name)
        .map_err(|error| FactorError::Storage(error.to_string()))?;
    batch
        .column(index)
        .as_any()
        .downcast_ref::<UInt32Array>()
        .ok_or_else(|| FactorError::Storage(format!("{name} is not a UInt32Array")))
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};
    use tg_contracts::FactorValue;

    use super::{factor_partition_path, FactorPartition, FactorValueStore};

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()
    }

    #[tokio::test]
    async fn factor_values_round_trip_with_partition_layout() {
        let temp = tempfile::tempdir().unwrap();
        let store = FactorValueStore::new(temp.path());
        let row = FactorValue {
            symbol: "600519".to_owned(),
            factor: "momentum_20d".to_owned(),
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 7, 0, 0).unwrap(),
            trading_date: date(),
            value: 0.12,
            rank: Some(3),
        };
        store
            .write_values(std::slice::from_ref(&row))
            .await
            .unwrap();
        let actual = store.read_partition("momentum_20d", date()).await.unwrap();
        assert_eq!(actual, vec![row]);
        assert_eq!(
            factor_partition_path(
                temp.path(),
                &FactorPartition {
                    factor: "momentum_20d".to_owned(),
                    date: date(),
                },
            ),
            temp.path()
                .join("data/factors/factor=momentum_20d/date=20260615/part.parquet")
        );
    }
}

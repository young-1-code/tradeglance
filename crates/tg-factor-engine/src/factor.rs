use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tg_contracts::Bar;

use crate::error::{FactorError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FactorCategory {
    Momentum,
    Reversal,
    Volatility,
    Volume,
    MoneyFlow,
    Size,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FactorDirection {
    Positive,
    Negative,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataDependency {
    DailyBars(usize),
    MinuteBars { period: String, lookback: usize },
    SnapshotQuote,
    Fundamentals(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactorMeta {
    pub name: String,
    pub category: FactorCategory,
    pub logic: String,
    pub data_dependencies: Vec<DataDependency>,
    pub params: HashMap<String, f64>,
    pub direction: FactorDirection,
    pub enabled: bool,
}

#[async_trait]
pub trait Factor: Send + Sync {
    fn meta(&self) -> &FactorMeta;

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>>;

    async fn compute_cross_section(
        &self,
        universe: &[(String, &[Bar])],
    ) -> Result<Vec<(String, f64)>> {
        let mut out = Vec::with_capacity(universe.len());
        for (symbol, bars) in universe {
            let series = self.compute_timeseries(bars).await?;
            let value = series.last().copied().unwrap_or(f64::NAN);
            out.push((symbol.clone(), value));
        }
        Ok(out)
    }
}

#[derive(Clone, Default)]
pub struct FactorRegistry {
    factors: BTreeMap<String, Arc<dyn Factor>>,
}

impl FactorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<F>(&mut self, factor: F) -> Result<()>
    where
        F: Factor + 'static,
    {
        let name = factor.meta().name.clone();
        if self.factors.contains_key(&name) {
            return Err(FactorError::InvalidInput(format!(
                "duplicate factor registration: {name}"
            )));
        }
        self.factors.insert(name, Arc::new(factor));
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn Factor>> {
        self.factors
            .get(name)
            .cloned()
            .ok_or_else(|| FactorError::UnknownFactor(name.to_owned()))
    }

    pub fn list(&self) -> Vec<FactorMeta> {
        self.factors
            .values()
            .map(|factor| factor.meta().clone())
            .collect()
    }

    pub fn list_by_category(
        &self,
        category: FactorCategory,
        enabled_only: bool,
    ) -> Vec<FactorMeta> {
        self.list()
            .into_iter()
            .filter(|meta| meta.category == category)
            .filter(|meta| !enabled_only || meta.enabled)
            .collect()
    }
}

pub fn default_registry() -> FactorRegistry {
    crate::factors::default_registry()
}

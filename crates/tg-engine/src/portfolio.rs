use std::{
    collections::HashMap,
    sync::{RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use rust_decimal::Decimal;
use tg_contracts::{Account, Bar, Fill, Order, OrderSide, Position, Result, Snapshot, TgError};

use crate::traits::{CrossSection, Portfolio};

#[derive(Debug)]
pub struct InMemoryPortfolio {
    account: RwLock<Account>,
    open_orders: RwLock<HashMap<String, Order>>,
}

impl InMemoryPortfolio {
    pub fn new(account: Account) -> Self {
        Self {
            account: RwLock::new(account),
            open_orders: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_cash(cash: Decimal) -> Self {
        Self::new(Account {
            cash,
            frozen_cash: Decimal::ZERO,
            total_value: cash,
            positions: HashMap::new(),
        })
    }

    pub fn upsert_open_order(&self, order: Order) -> Result<()> {
        self.write_orders()?.insert(order.id.clone(), order);
        Ok(())
    }

    pub fn remove_open_order(&self, order_id: &str) -> Result<Option<Order>> {
        Ok(self.write_orders()?.remove(order_id))
    }

    fn read_account(&self) -> Result<RwLockReadGuard<'_, Account>> {
        self.account
            .read()
            .map_err(|_| TgError::Other(anyhow::anyhow!("portfolio account lock poisoned")))
    }

    fn write_account(&self) -> Result<RwLockWriteGuard<'_, Account>> {
        self.account
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("portfolio account lock poisoned")))
    }

    fn read_orders(&self) -> Result<RwLockReadGuard<'_, HashMap<String, Order>>> {
        self.open_orders
            .read()
            .map_err(|_| TgError::Other(anyhow::anyhow!("portfolio orders lock poisoned")))
    }

    fn write_orders(&self) -> Result<RwLockWriteGuard<'_, HashMap<String, Order>>> {
        self.open_orders
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("portfolio orders lock poisoned")))
    }
}

impl Default for InMemoryPortfolio {
    fn default() -> Self {
        Self::with_cash(Decimal::ZERO)
    }
}

impl Portfolio for InMemoryPortfolio {
    fn account(&self) -> Account {
        self.read_account()
            .expect("portfolio account lock should not be poisoned")
            .clone()
    }

    fn position(&self, symbol: &str) -> Option<Position> {
        self.read_account()
            .expect("portfolio account lock should not be poisoned")
            .positions
            .get(symbol)
            .cloned()
    }

    fn positions(&self) -> Vec<Position> {
        self.read_account()
            .expect("portfolio account lock should not be poisoned")
            .positions
            .values()
            .cloned()
            .collect()
    }

    fn open_orders(&self) -> Vec<Order> {
        self.read_orders()
            .expect("portfolio orders lock should not be poisoned")
            .values()
            .cloned()
            .collect()
    }

    fn apply_fill(&self, fill: &Fill) -> Result<()> {
        let mut account = self.write_account()?;
        let fees = fill.commission + fill.tax + fill.transfer_fee;
        let notional = fill.price * Decimal::from(fill.quantity);

        match fill.side {
            OrderSide::Buy => {
                account.cash -= notional + fees;
                let entry = account
                    .positions
                    .entry(fill.symbol.clone())
                    .or_insert_with(|| Position {
                        symbol: fill.symbol.clone(),
                        exchange: fill.exchange,
                        total_quantity: 0,
                        t1_locked_quantity: 0,
                        available_quantity: 0,
                        avg_cost: Decimal::ZERO,
                        last_price: fill.price,
                        market_value: Decimal::ZERO,
                        unrealized_pnl: Decimal::ZERO,
                    });

                let old_quantity = entry.total_quantity;
                let new_quantity = old_quantity + fill.quantity;
                let old_cost_basis = entry.avg_cost * Decimal::from(old_quantity);
                let new_cost_basis = old_cost_basis + notional + fees;

                entry.total_quantity = new_quantity;
                entry.t1_locked_quantity += fill.quantity;
                entry.avg_cost = if new_quantity == 0 {
                    Decimal::ZERO
                } else {
                    new_cost_basis / Decimal::from(new_quantity)
                };
                entry.last_price = fill.price;
                entry.market_value = fill.price * Decimal::from(entry.total_quantity);
                entry.unrealized_pnl =
                    (fill.price - entry.avg_cost) * Decimal::from(entry.total_quantity);
            }
            OrderSide::Sell => {
                account.cash += notional - fees;
                let mut remove_position = false;
                if let Some(entry) = account.positions.get_mut(&fill.symbol) {
                    entry.total_quantity -= fill.quantity;
                    entry.available_quantity =
                        entry.available_quantity.saturating_sub(fill.quantity);
                    if entry.total_quantity <= 0 {
                        remove_position = true;
                    } else {
                        entry.t1_locked_quantity =
                            entry.t1_locked_quantity.min(entry.total_quantity);
                        entry.last_price = fill.price;
                        entry.market_value = fill.price * Decimal::from(entry.total_quantity);
                        entry.unrealized_pnl =
                            (fill.price - entry.avg_cost) * Decimal::from(entry.total_quantity);
                    }
                }
                if remove_position {
                    account.positions.remove(&fill.symbol);
                }
            }
        }

        let positions_value = account
            .positions
            .values()
            .fold(Decimal::ZERO, |acc, position| acc + position.market_value);
        account.total_value = account.cash + positions_value;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct InMemoryCrossSection {
    bars: RwLock<HashMap<String, Bar>>,
    snapshots: RwLock<HashMap<String, Snapshot>>,
}

impl InMemoryCrossSection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_bar(&self, bar: Bar) -> Result<()> {
        self.bars
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("cross-section bar lock poisoned")))?
            .insert(bar.symbol.clone(), bar);
        Ok(())
    }

    pub fn update_snapshot(&self, snapshot: Snapshot) -> Result<()> {
        self.snapshots
            .write()
            .map_err(|_| TgError::Other(anyhow::anyhow!("cross-section snapshot lock poisoned")))?
            .insert(snapshot.symbol.clone(), snapshot);
        Ok(())
    }
}

impl CrossSection for InMemoryCrossSection {
    fn latest_bar(&self, symbol: &str) -> Option<Bar> {
        self.bars
            .read()
            .expect("cross-section bar lock should not be poisoned")
            .get(symbol)
            .cloned()
    }

    fn latest_snapshot(&self, symbol: &str) -> Option<Snapshot> {
        self.snapshots
            .read()
            .expect("cross-section snapshot lock should not be poisoned")
            .get(symbol)
            .cloned()
    }

    fn universe(&self) -> Vec<String> {
        let mut symbols: Vec<String> = self
            .bars
            .read()
            .expect("cross-section bar lock should not be poisoned")
            .keys()
            .cloned()
            .collect();
        for symbol in self
            .snapshots
            .read()
            .expect("cross-section snapshot lock should not be poisoned")
            .keys()
        {
            if !symbols.contains(symbol) {
                symbols.push(symbol.clone());
            }
        }
        symbols.sort();
        symbols
    }
}

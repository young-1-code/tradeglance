use std::sync::Arc;

use chrono::{NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use tg_contracts::proto::tg::v1 as pb;
use tg_contracts::{
    Account, Exchange, Fill, Order, OrderIntent, OrderSide, OrderStatus, OrderType, Position,
    StrategyStyle, TgError, TimeInForce,
};
use tg_engine::ExecutionHandler;
use tonic::{Request, Response, Status};

use crate::handler::MockExecutionHandler;

pub struct OrderGrpcService {
    handler: Arc<MockExecutionHandler>,
}

impl OrderGrpcService {
    pub fn new(handler: Arc<MockExecutionHandler>) -> Self {
        Self { handler }
    }
}

#[allow(clippy::result_large_err)]
#[tonic::async_trait]
impl pb::order_service_server::OrderService for OrderGrpcService {
    async fn submit_order(
        &self,
        request: Request<pb::OrderIntent>,
    ) -> std::result::Result<Response<pb::Order>, Status> {
        let intent = intent_from_proto(request.into_inner())?;
        let order_id = self
            .handler
            .submit(intent)
            .await
            .map_err(status_from_error)?;
        let order = self
            .handler
            .get_order(&order_id)
            .map_err(status_from_error)?;
        Ok(Response::new(order_to_proto(order)))
    }

    async fn cancel_order(
        &self,
        request: Request<pb::CancelOrderRequest>,
    ) -> std::result::Result<Response<pb::Order>, Status> {
        let order_id = request.into_inner().order_id;
        self.handler
            .cancel(&order_id)
            .await
            .map_err(status_from_error)?;
        let order = self
            .handler
            .get_order(&order_id)
            .map_err(status_from_error)?;
        Ok(Response::new(order_to_proto(order)))
    }

    async fn get_order(
        &self,
        request: Request<pb::GetOrderRequest>,
    ) -> std::result::Result<Response<pb::Order>, Status> {
        let order = self
            .handler
            .get_order(&request.into_inner().order_id)
            .map_err(status_from_error)?;
        Ok(Response::new(order_to_proto(order)))
    }

    async fn query_positions(
        &self,
        request: Request<pb::QueryPositionsRequest>,
    ) -> std::result::Result<Response<pb::QueryPositionsResponse>, Status> {
        let symbols = request.into_inner().symbols;
        let positions = self
            .handler
            .snapshot_positions()
            .await
            .map_err(status_from_error)?
            .into_iter()
            .filter(|position| symbols.is_empty() || symbols.contains(&position.symbol))
            .map(position_to_proto)
            .collect();
        Ok(Response::new(pb::QueryPositionsResponse { positions }))
    }

    async fn query_account(
        &self,
        _request: Request<pb::Empty>,
    ) -> std::result::Result<Response<pb::Account>, Status> {
        let account = self
            .handler
            .snapshot_account()
            .await
            .map_err(status_from_error)?;
        Ok(Response::new(account_to_proto(account)))
    }
}

#[allow(clippy::result_large_err)]
fn intent_from_proto(intent: pb::OrderIntent) -> std::result::Result<OrderIntent, Status> {
    Ok(OrderIntent {
        client_order_id: intent.client_order_id,
        symbol: intent.symbol,
        exchange: exchange_from_i32(intent.exchange)?,
        side: side_from_i32(intent.side)?,
        order_type: order_type_from_i32(intent.order_type)?,
        price: if intent.has_price {
            Some(parse_decimal(&intent.price)?)
        } else {
            None
        },
        quantity: intent.quantity,
        time_in_force: tif_from_i32(intent.time_in_force)?,
        strategy_tag: style_from_i32(intent.strategy_tag)?,
    })
}

fn order_to_proto(order: Order) -> pb::Order {
    pb::Order {
        id: order.id,
        client_order_id: order.client_order_id,
        symbol: order.symbol,
        exchange: exchange_to_proto(order.exchange) as i32,
        side: side_to_proto(order.side) as i32,
        order_type: order_type_to_proto(order.order_type) as i32,
        price: order
            .price
            .map(|price| price.to_string())
            .unwrap_or_default(),
        has_price: order.price.is_some(),
        quantity: order.quantity,
        time_in_force: tif_to_proto(order.time_in_force) as i32,
        strategy_tag: style_to_proto(order.strategy_tag) as i32,
        created_at_epoch_millis: order.created_at.timestamp_millis(),
        status: status_to_proto(order.status) as i32,
        filled_quantity: order.filled_quantity,
        avg_fill_price: order.avg_fill_price.to_string(),
    }
}

fn position_to_proto(position: Position) -> pb::Position {
    pb::Position {
        symbol: position.symbol,
        exchange: exchange_to_proto(position.exchange) as i32,
        total_quantity: position.total_quantity,
        t1_locked_quantity: position.t1_locked_quantity,
        available_quantity: position.available_quantity,
        avg_cost: position.avg_cost.to_string(),
        last_price: position.last_price.to_string(),
        market_value: position.market_value.to_string(),
        unrealized_pnl: position.unrealized_pnl.to_string(),
    }
}

fn account_to_proto(account: Account) -> pb::Account {
    pb::Account {
        cash: account.cash.to_string(),
        frozen_cash: account.frozen_cash.to_string(),
        total_value: account.total_value.to_string(),
        positions: account
            .positions
            .into_iter()
            .map(|(symbol, position)| (symbol, position_to_proto(position)))
            .collect(),
    }
}

#[allow(dead_code)]
fn fill_to_proto(fill: Fill) -> pb::Fill {
    pb::Fill {
        order_id: fill.order_id,
        fill_id: fill.fill_id,
        symbol: fill.symbol,
        exchange: exchange_to_proto(fill.exchange) as i32,
        side: side_to_proto(fill.side) as i32,
        price: fill.price.to_string(),
        quantity: fill.quantity,
        commission: fill.commission.to_string(),
        tax: fill.tax.to_string(),
        transfer_fee: fill.transfer_fee.to_string(),
        ts_epoch_millis: fill.ts.timestamp_millis(),
        trading_date: fill.trading_date.to_string(),
    }
}

#[allow(clippy::result_large_err)]
fn parse_decimal(raw: &str) -> std::result::Result<Decimal, Status> {
    raw.parse::<Decimal>()
        .map_err(|error| Status::invalid_argument(error.to_string()))
}

#[allow(clippy::result_large_err)]
fn exchange_from_i32(value: i32) -> std::result::Result<Exchange, Status> {
    match pb::Exchange::from_i32(value) {
        Some(pb::Exchange::Sh) => Ok(Exchange::Sh),
        Some(pb::Exchange::Sz) => Ok(Exchange::Sz),
        Some(pb::Exchange::Bj) => Ok(Exchange::Bj),
        _ => Err(Status::invalid_argument(format!(
            "invalid exchange: {value}"
        ))),
    }
}

#[allow(clippy::result_large_err)]
fn side_from_i32(value: i32) -> std::result::Result<OrderSide, Status> {
    match pb::OrderSide::from_i32(value) {
        Some(pb::OrderSide::Buy) => Ok(OrderSide::Buy),
        Some(pb::OrderSide::Sell) => Ok(OrderSide::Sell),
        _ => Err(Status::invalid_argument(format!("invalid side: {value}"))),
    }
}

#[allow(clippy::result_large_err)]
fn order_type_from_i32(value: i32) -> std::result::Result<OrderType, Status> {
    match pb::OrderType::from_i32(value) {
        Some(pb::OrderType::Limit) => Ok(OrderType::Limit),
        Some(pb::OrderType::Market) => Ok(OrderType::Market),
        _ => Err(Status::invalid_argument(format!(
            "invalid order type: {value}"
        ))),
    }
}

#[allow(clippy::result_large_err)]
fn tif_from_i32(value: i32) -> std::result::Result<TimeInForce, Status> {
    match pb::TimeInForce::from_i32(value) {
        Some(pb::TimeInForce::Day) => Ok(TimeInForce::Day),
        Some(pb::TimeInForce::Gtc) => Ok(TimeInForce::Gtc),
        _ => Err(Status::invalid_argument(format!(
            "invalid time in force: {value}"
        ))),
    }
}

#[allow(clippy::result_large_err)]
fn style_from_i32(value: i32) -> std::result::Result<StrategyStyle, Status> {
    match pb::StrategyStyle::from_i32(value) {
        Some(pb::StrategyStyle::Swing) => Ok(StrategyStyle::Swing),
        Some(pb::StrategyStyle::T0) => Ok(StrategyStyle::T0),
        Some(pb::StrategyStyle::LimitUp) => Ok(StrategyStyle::LimitUp),
        _ => Err(Status::invalid_argument(format!(
            "invalid strategy style: {value}"
        ))),
    }
}

fn exchange_to_proto(exchange: Exchange) -> pb::Exchange {
    match exchange {
        Exchange::Sh => pb::Exchange::Sh,
        Exchange::Sz => pb::Exchange::Sz,
        Exchange::Bj => pb::Exchange::Bj,
    }
}

fn side_to_proto(side: OrderSide) -> pb::OrderSide {
    match side {
        OrderSide::Buy => pb::OrderSide::Buy,
        OrderSide::Sell => pb::OrderSide::Sell,
    }
}

fn order_type_to_proto(order_type: OrderType) -> pb::OrderType {
    match order_type {
        OrderType::Limit => pb::OrderType::Limit,
        OrderType::Market => pb::OrderType::Market,
    }
}

fn tif_to_proto(tif: TimeInForce) -> pb::TimeInForce {
    match tif {
        TimeInForce::Day => pb::TimeInForce::Day,
        TimeInForce::Gtc => pb::TimeInForce::Gtc,
    }
}

fn style_to_proto(style: StrategyStyle) -> pb::StrategyStyle {
    match style {
        StrategyStyle::Swing => pb::StrategyStyle::Swing,
        StrategyStyle::T0 => pb::StrategyStyle::T0,
        StrategyStyle::LimitUp => pb::StrategyStyle::LimitUp,
    }
}

fn status_to_proto(status: OrderStatus) -> pb::OrderStatus {
    match status {
        OrderStatus::New => pb::OrderStatus::New,
        OrderStatus::PartiallyFilled => pb::OrderStatus::PartiallyFilled,
        OrderStatus::Filled => pb::OrderStatus::Filled,
        OrderStatus::Cancelled => pb::OrderStatus::Cancelled,
        OrderStatus::Rejected => pb::OrderStatus::Rejected,
    }
}

#[allow(clippy::result_large_err, dead_code)]
fn utc_from_ms(ms: i64) -> std::result::Result<chrono::DateTime<Utc>, Status> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .ok_or_else(|| Status::invalid_argument(format!("invalid epoch millis: {ms}")))
}

#[allow(clippy::result_large_err, dead_code)]
fn date_from_proto(raw: &str) -> std::result::Result<NaiveDate, Status> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|error| Status::invalid_argument(error.to_string()))
}

fn status_from_error(error: TgError) -> Status {
    match error {
        TgError::Validation(message) => Status::invalid_argument(message),
        TgError::NotFound(message) => Status::not_found(message),
        TgError::RateLimited => Status::resource_exhausted("rate limited"),
        TgError::Upstream(message) => Status::unavailable(message),
        TgError::InvalidOrder(message) => Status::failed_precondition(message),
        TgError::RiskRejected(message) => Status::failed_precondition(message),
        TgError::Other(error) => Status::internal(error.to_string()),
    }
}

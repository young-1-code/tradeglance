# tg-contracts — 共享契约层 详细设计 Spec

> **子项目**：`tg-contracts`（全系统单一事实来源）
> **状态**：设计完成，待用户评审
> **日期**：2026-06-14
> **上游文档**：`2026-06-14-tradeglance-architecture-design.md`（v0.6）
> **关系**：本文**整合并权威化** Phase 0 spec §3 的数据类型，并补充订单/持仓/账户/指标/因子/信号/决策/事件等跨阶段共享类型。Phase 0 spec §3 不再单独维护，以本文为准。

---

## 1. 概述与范围

### 1.1 目标
为全系统提供**单一事实来源**：所有跨服务数据类型、枚举、常量、gRPC 服务接口定义集中于此。任何模块（Rust crate 或 C++ 服务）都只引用 `tg-contracts`，不在各自仓库重复定义共享类型——这是 ADR 契约先行原则的落地。

### 1.2 In Scope
- Rust 领域类型 + 枚举 + 常量（rust_decimal / chrono / ulid）
- protobuf `.proto`：所有 gRPC service/message 定义（供 Rust 与 C++ 各自生成）
- A 股交易规则常量与判定函数
- 全系统编码规范（数值/时间/ID/错误/异步/gRPC/测试/日志/提交）

### 1.3 Out of Scope
- 任何业务逻辑（采集、计算、撮合、决策）——本 crate 纯定义，零业务。
- 存储实现（见 `tg-persistence`）、采集（见 `tg-market-data`）。

### 1.4 形态
- Rust **library crate** `tg-contracts`（workspace 成员）。
- proto 子目录 `proto/tg/*.proto`，由 build.rs 用 `tonic-build` 给 Rust 生成、由 CMake 给 `tg-indicators` 生成 C++ stub。
- 版本：proto 文件带 `package tg.v1;`，未来破坏性变更走 v2。

---

## 2. 数据类型（权威定义）

> 价格一律 `rust_decimal::Decimal`；时间戳内部 `DateTime<Utc>`；交易日 `NaiveDate`（CST）。

### 2.1 枚举与常量

```rust
// —— 市场 / 品种 / 板块（继承自 Phase 0）——
pub enum Exchange { Sh, Sz, Bj }
pub enum InstrumentType { Stock, Etf }
pub enum Board { MainBoard, Star, ChiNext, Bj }

pub enum BarPeriod { Daily, Min1, Min5 }
pub enum Adjustment { None, PreAdjust, PostAdjust }

// —— 订单领域 ——
pub enum OrderSide   { Buy, Sell }
pub enum OrderType   { Limit, Market }        // A股：限价为主；市价部分品种支持
pub enum TimeInForce { Day, Gtc }             // A股基本为 Day（当日有效）
pub enum OrderStatus {
    New, PartiallyFilled, Filled, Cancelled, Rejected,
}

// —— 信号 / 决策 / 策略 ——
pub enum SignalDirection { Long, Short, Flat, CloseLong, CloseShort }
pub enum StrategyStyle   { Swing, T0, LimitUp }   // 波段 / T0做T / 打板（ADR-014）
pub enum DecisionAction  { Open, Add, Reduce, Close, Hold }

// —— 事件（tg-engine 事件循环）——
pub enum EventKind { Bar, Snapshot, Timer, Fill }

// —— A 股规则常量 ——
pub const LOT_SIZE: i64 = 100;                 // 最小交易单位（股）
pub const STAMP_DUTY_PCT: Decimal = dec!(0.0005); // 印花税：卖出 0.05%（ADR：ETF 免）
pub const COMMISSION_MAX_PCT: Decimal = dec!(0.0003); // 佣金上限 0.03%（可配）
pub const TRANSFER_FEE_PCT: Decimal = dec!(0.00001);  // 过户费 0.001%（仅沪市）

/// 涨停幅度（按板块）：主板±10% / 科创·创业±20% / 北交±30%
pub fn limit_up_pct(board: Board) -> Decimal {
    match board {
        Board::MainBoard => dec!(0.10),
        Board::Star | Board::ChiNext => dec!(0.20),
        Board::Bj => dec!(0.30),
    }
}
/// 交易时段判定（CST）：09:30-11:30 / 13:00-15:00 连续竞价
pub fn is_continuous_auction(ts: DateTime<Utc>) -> bool { /* ... */ }
/// 集合竞价：09:15-09:25（开盘）/ 14:57-15:00（收盘）
pub fn is_call_auction(ts: DateTime<Utc>) -> bool { /* ... */ }
```

### 2.2 行情 / 元数据（继承自 Phase 0，以本文为权威）

```rust
pub struct Instrument {
    pub symbol: String,            // 6 位代码
    pub exchange: Exchange,
    pub instrument_type: InstrumentType,
    pub name: String,
    pub list_date: NaiveDate,
    pub delist_date: Option<NaiveDate>,
    pub is_st: bool,
    pub board: Board,
}

pub struct Bar {                   // K 线（不复权原始存储）
    pub symbol: String, pub exchange: Exchange, pub period: BarPeriod,
    pub ts: DateTime<Utc>,         // K 线结束时刻（UTC）
    pub trading_date: NaiveDate,
    pub open: Decimal, pub high: Decimal, pub low: Decimal, pub close: Decimal,
    pub volume: i64,               // 股
    pub amount: Decimal,           // 元
}

pub struct Snapshot {              // 实时秒级快照
    pub symbol: String, pub exchange: Exchange,
    pub ts: DateTime<Utc>, pub trading_date: NaiveDate,
    pub last: Decimal, pub open: Decimal, pub high: Decimal, pub low: Decimal,
    pub pre_close: Decimal, pub volume: i64, pub amount: Decimal,
    pub bid_price: [Decimal; 5], pub bid_volume: [i64; 5],
    pub ask_price: [Decimal; 5], pub ask_volume: [i64; 5],
}

pub struct AdjustmentFactor { pub symbol: String, pub ex_date: NaiveDate, pub factor: Decimal }
pub struct TradingCalendar   { pub date: NaiveDate, pub is_trading_day: bool }
```

### 2.3 订单 / 成交 / 持仓 / 账户（Phase 2+）

```rust
pub type OrderId = String;         // ULID 字符串

/// 策略 / 决策层产出的订单意图（尚无 id、未校验），由 ExecutionHandler 落地为 Order
pub struct OrderIntent {
    pub client_order_id: String,    // 提交幂等键
    pub symbol: String, pub exchange: Exchange,
    pub side: OrderSide, pub order_type: OrderType,
    pub price: Option<Decimal>,     // 限价必填；市价 None
    pub quantity: i64,              // 期望为 LOT_SIZE 整数倍，由执行器复核
    pub time_in_force: TimeInForce,
    pub strategy_tag: StrategyStyle, // 溯源：swing/t0/limitup
}

pub struct Order {
    pub id: OrderId,
    pub client_order_id: String,    // 调用方幂等键
    pub symbol: String, pub exchange: Exchange,
    pub side: OrderSide, pub order_type: OrderType,
    pub price: Option<Decimal>,     // 限价必填；市价 None
    pub quantity: i64,              // 必须为 LOT_SIZE 正整数倍
    pub time_in_force: TimeInForce,
    pub strategy_tag: StrategyStyle, // 溯源：swing/t0/limitup
    pub created_at: DateTime<Utc>,
    pub status: OrderStatus,
    pub filled_quantity: i64,
    pub avg_fill_price: Decimal,
}

pub struct Fill {
    pub order_id: OrderId, pub fill_id: String,
    pub symbol: String, pub exchange: Exchange, pub side: OrderSide,
    pub price: Decimal, pub quantity: i64,
    pub commission: Decimal, pub tax: Decimal, pub transfer_fee: Decimal,
    pub ts: DateTime<Utc>, pub trading_date: NaiveDate,
}

pub struct Position {
    pub symbol: String, pub exchange: Exchange,
    pub total_quantity: i64,
    pub t1_locked_quantity: i64,    // 今日买入、T+1 锁定不可卖
    pub available_quantity: i64,    // = total - t1_locked，今日可卖
    pub avg_cost: Decimal,          // 持仓均价（含费用）
    pub last_price: Decimal,
    pub market_value: Decimal,
    pub unrealized_pnl: Decimal,
}

pub struct Account {
    pub cash: Decimal,
    pub frozen_cash: Decimal,       // 挂单冻结
    pub total_value: Decimal,       // cash + 持仓市值
    pub positions: std::collections::HashMap<String, Position>,
}
```

### 2.4 指标 / 因子（Phase 1）

```rust
pub struct IndicatorRequest {
    pub indicator: String,                       // "RSI" / "MACD" / ...
    pub params: std::collections::HashMap<String, f64>, // {"period":14}
    pub bars: Vec<Bar>,
}
pub struct IndicatorResult {
    pub indicator: String,
    pub ts: Vec<DateTime<Utc>>,                  // 对齐时间轴
    pub series: std::collections::HashMap<String, Vec<f64>>, // 命名子序列
    // 例：MACD → {"dif":[..],"dea":[..],"hist":[..]}；布林 → {"upper","mid","lower"}
}

pub struct FactorValue {
    pub symbol: String, pub factor: String,
    pub ts: DateTime<Utc>, pub trading_date: NaiveDate,
    pub value: f64,                // 因子原始值（横截面需标准化）
    pub rank: Option<u32>,         // 横截面排名（选股用）
}
pub struct FactorEvaluation {
    pub factor: String,
    pub ic_mean: f64, pub ic_std: f64, pub ir: f64,   // IR = ic_mean/ic_std
    pub decay: Vec<f64>,           // 各滞后日的 IC
    pub quantile_returns: Vec<f64>, // 分层（分位组）收益
}
```

### 2.5 信号 / 决策（Phase 2 / Phase 3）

```rust
pub type SignalId = String;

pub struct Signal {
    pub id: SignalId,
    pub symbol: String, pub exchange: Exchange,
    pub direction: SignalDirection,
    pub strength: f64,             // 0.0..1.0
    pub confidence: f64,           // 0.0..1.0
    pub style: StrategyStyle,
    pub reason: Vec<String>,       // 审计：触发的指标/因子条件（可解释）
    pub suggested_quantity: Option<i64>,
    pub ts: DateTime<Utc>, pub trading_date: NaiveDate,
}

pub struct RiskCheckResult { pub rule: String, pub passed: bool, pub detail: String }

pub struct Decision {
    pub id: String,                // ULID
    pub signal_id: Option<SignalId>,
    pub symbol: String, pub exchange: Exchange,
    pub action: DecisionAction, pub side: OrderSide,
    pub target_quantity: i64,
    pub rationale: String,         // LLM 理由（审计）
    pub risk_checks: Vec<RiskCheckResult>,
    pub ts: DateTime<Utc>,
}
```

### 2.6 事件（Phase 2，tg-engine）

```rust
pub enum Event {
    Bar(Bar),
    Snapshot(Snapshot),
    Timer(DateTime<Utc>),
    Fill(Fill),
}
```

### 2.7 查询参数

```rust
pub struct BarQuery {
    pub symbol: String, pub period: BarPeriod,
    pub range: Range<DateTime<Utc>>,
    pub adjustment: Adjustment,    // 查询时按需换算
}
```

---

## 3. gRPC 服务目录（全系统）

> 所有跨进程 RPC 集中定义于 `proto/tg/`。Rust 用 tonic；C++（indicators）用 grpc++。每个服务对应一个模块。

| proto service | 所属模块 | 关键 RPC | Phase |
|---|---|---|---|
| `MarketDataControl` | tg-market-data | TriggerFullSync / TriggerIncrementalSync / GetSyncStatus / UpdateWatchlist / GetWatchlist | 0 |
| `IndicatorService` | tg-indicators | Compute(IndicatorRequest) → IndicatorResult；BatchCompute(stream in) | 1 |
| `FactorService` | tg-factor-engine | ComputeFactor / EvaluateFactor / QueryFactorValues | 1 |
| `BacktestService` | tg-backtest | SubmitBacktest / GetBacktestStatus / GetBacktestResult | 2 |
| `SignalService` | tg-signal-engine | SubscribeSignals(stream) / QuerySignals | 2 |
| `DecisionService` | tg-decision-agent | Decide(Signal/Context) → Decision；SubscribeDecisions(stream) | 3 |
| `OrderService` | tg-mock-order-engine | SubmitOrder / CancelOrder / GetOrder / QueryPositions / QueryAccount | 3 |
| `MonitoringApi` | tg-monitoring-viz | 以 REST 为主（见 Phase 4） | 4 |

> 注：行情**查询**不经 RPC，消费者直接链 `tg-persistence` crate 读写（ADR-017）。gRPC 仅用于控制面与流式事件。

---

## 4. 错误模型

```rust
// 每个领域一个具名错误（thiserror），跨边界用 anyhow
#[derive(Debug, thiserror::Error)]
pub enum TgError {
    #[error("data validation: {0}")]   Validation(String),
    #[error("not found: {0}")]         NotFound(String),
    #[error("rate limited")]           RateLimited,
    #[error("upstream: {0}")]          Upstream(String),
    #[error("invalid order: {0}")]     InvalidOrder(String),
    #[error("risk rejected: {0}")]     RiskRejected(String),
    #[error(transparent)]              Other(#[from] anyhow::Error),
}
pub type Result<T> = std::result::Result<T, TgError>;
```
gRPC 侧由 tonic `Status`（code + message）映射；`InvalidArgument/NotFound/ResourceExhausted/FailedPrecondition` 对应上述变体。

---

## 5. 编码规范（全系统统一）

| 维度 | 规范 |
|---|---|
| **价格/金额** | `rust_decimal::Decimal`；存储 Arrow `Decimal128(18,4)` / PG `NUMERIC(18,4)`；因子值用 `f64`（横截面统计）。 |
| **时间** | 内部 `DateTime<Utc>` / epoch millis；`trading_date` 为 CST `NaiveDate`，作分区键。转换集中在 `tg-contracts::time`。 |
| **ID** | 订单/信号/决策用 **ULID**（`ulid` crate，时间有序 + 唯一）；`client_order_id` 作幂等键。 |
| **异步** | 全 `tokio`；trait 方法用 `#[async_trait]`；不阻塞 runtime。 |
| **错误** | 各 crate 具名 `thiserror` 错误；binary 边界 `anyhow`；RPC 边界 tonic `Status`。 |
| **日志** | `tracing`（结构化字段）； spans 带 symbol/period/request_id。 |
| **gRPC** | tonic + prost；流式用 tonic streaming；`tonic-build` 在 build.rs 生成。 |
| **测试** | 单元（确定性）/ 集成（mock）/ 冒烟（`#[ignore]` 真实外部依赖）。 |
| **提交** | Conventional Commits：`feat:/fix:/test:/docs:/chore:/refactor:`。 |
| **命名** | crate `tg-*`；模块 snake_case；类型 PascalCase；proto message PascalCase、field snake_case。 |

---

## 6. 仓库布局

```
tradeglance/                         # Cargo workspace 根
├── Cargo.toml                       # [workspace] members = [crates/*]
├── crates/
│   ├── tg-contracts/                # 本 crate
│   │   ├── Cargo.toml
│   │   ├── src/lib.rs
│   │   ├── build.rs                 # tonic-build::compile_protos
│   │   └── proto/tg/*.proto
│   ├── tg-persistence/
│   ├── tg-market-data/
│   ├── tg-factor-engine/
│   ├── tg-engine/
│   ├── tg-backtest/
│   ├── tg-signal-engine/
│   ├── tg-decision-agent/
│   └── tg-mock-order-engine/
├── cpp/
│   └── tg-indicators/               # C++20 gRPC 服务（CMake）
├── apps/
│   ├── tg-monitoring-viz/           # Axum 后端 + TS 前端
│   └── tg-infra/                    # docker-compose / 配置
└── docs/superpowers/specs/          # 本目录
```
> 决策：12 模块为独立仓库语义，但开发期置于**单 workspace 多 crate**（monorepo）以利跨 crate 重构与共享 contracts；后续可按需拆分独立 repo。proto 单点维护。

---

## 7. 测试策略
- 单元：常量/判定函数（涨跌停、交易时段）、类型序列化往返、proto 编解码。
- 一致性：`tg-contracts` 编译为 no_std-friendly 的纯定义 crate，无副作用，测试以编译通过 + 序列化快照为主。
- 版本：proto 破坏性变更走 v2，Rust 类型提供转换层。

---

## 8. 验收标准（DoD）
1. `tg-contracts` crate 编译通过，`cargo doc` 无警告。
2. §2 全部类型可经 proto round-trip（Rust ↔ 模拟 C++ 端 bytes）无损。
3. A 股规则函数 `limit_up_pct / is_continuous_auction / is_call_auction` 单测覆盖各板块/时段边界。
4. 价格全程 `Decimal`，无 `f64` 混入订单/持仓/账户路径（lint 检查）。
5. 各下游 crate 仅依赖 `tg-contracts`，不重复定义共享类型（grep 校验）。

---

## 9. 依赖的 ADR
- ADR-008 语言分工（C++ 仅 indicators，余 Rust）
- ADR-014 策略风格（波段/T0/打板 → StrategyStyle 枚举）
- ADR-017 persistence 共享库 crate（消费方直接链 contracts + persistence）
- 契约先行（设计原则 3）

---

## 10. 后续 / 延期项
- proto v2 演进策略（字段废弃流程）。
- 跨语言契约测试（Rust ↔ C++ buf schema registry）。
- 货币精度策略在 ETF/可转债扩展时的复核。

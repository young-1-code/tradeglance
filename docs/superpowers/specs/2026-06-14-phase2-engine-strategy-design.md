# Phase 2 引擎与策略 — 详细设计 Spec

> **子项目**：`tg-engine` + `tg-backtest` + `tg-signal-engine`
> **状态**：设计完成，待用户评审
> **日期**：2026-06-14
> **上游文档**：`2026-06-14-tradeglance-architecture-design.md`（v0.6）
> **权威类型来源**：`2026-06-14-tg-contracts-design.md`（本文不重复定义共享类型，仅引用）
> **相关 ADR**：ADR-001 / 006 / 009 / 010 / 013 / 014 / 017；新增 ADR-025 ~ ADR-029

---

## 1. 概述与范围

### 1.1 目标
为 TradeGlance 构建"一套引擎、两种运行模式"的事件驱动内核（`tg-engine`），并在其上落地两条相互独立的能力线：
1. **回测与绩效**（`tg-backtest`）：用历史 Bar 回放驱动同一套引擎，按 A 股规则做历史撮合，产出可对比的绩效报告。
2. **结构化信号**（`tg-signal-engine`）：把指标与因子编排成带审计理由的 `Signal`，按 ADR-014 的三套策略原型（波段 / T0 做T / 打板）产出候选，供 Phase 3 的 `decision-agent` 消费。

三条线索共享 `tg-engine` 抽象（`Strategy` / `ExecutionHandler` / `DataFeed` / `Clock`），确保"回测能跑通的策略，模拟与未来实盘也能跑同一份代码"（ADR-006）。

### 1.2 In Scope
- `tg-engine`：事件循环、四类事件调度、四个核心 trait 与组合视图、横截面快照、两种运行模式的装配点。
- `tg-backtest`：历史回放器、历史撮合器（OHLC 模型 + A 股规则落地）、绩效指标计算、运行记录 schema、`BacktestService` gRPC。
- `tg-signal-engine`：规则引擎、三套策略原型的信号规则、横截面选股打分、信号发布、`SignalService` gRPC。

### 1.3 Out of Scope（YAGNI，留给后续 Phase）
- 实时撮合、虚拟账户、A 股规则的**实时**执行（Phase 3 `tg-mock-order-engine`）；本 Phase 的 A 股规则仅落在历史撮合器（回测路径）。
- LLM 决策、风险否决、订单生命周期管理（Phase 3）。
- 实时墙钟驱动的 DataFeed 实现（Phase 3 实现，本 Phase 只定义 trait 与历史回放实现）。
- `tg-mock-order-engine` 对 `ExecutionHandler` 的实现（Phase 3）。
- 参数寻优（网格 / 贝叶斯）——仅留接口位与运行记录 schema，算法延期。
- 监控可视化（Phase 4）。

### 1.4 设计基线
- 共享类型一律引用 `tg-contracts`：`Bar` / `Snapshot` / `Fill` / `Order` / `Position` / `Account` / `Signal` / `Event` / `EventKind` / `SignalDirection` / `StrategyStyle` / `OrderSide` / `OrderType` / `OrderStatus` / `TimeInForce` / `Instrument` / `Exchange` / `Board` / `BarPeriod` / `Decision` 等。本 spec **不重复定义**这些类型。
- 价格 / 金额用 `rust_decimal::Decimal`；绩效统计比率、因子值用 `f64`；时间 `DateTime<Utc>`，`trading_date` 为 CST `NaiveDate`。
- 全异步 `tokio`；trait 方法 `#[async_trait]`；错误用 `thiserror` 具名错误，binary 边界 `anyhow`，RPC 边界 tonic `Status`。
- ADR-006（共用引擎）、ADR-009（`ExecutionHandler` 为 mock/实盘切换点）、ADR-010（signal-engine 仅产候选、不直接下单）为不可违背约束。

---

## 2. 模块形态与依赖

| 模块 | 形态 | 语言 | 角色 |
|---|---|---|---|
| `tg-engine` | **library crate**（非服务） | Rust | 事件驱动内核：trait + 事件循环 + 组合视图；被 backtest / signal-engine / 后续 mock-order-engine 链接复用 |
| `tg-backtest` | 服务 crate + gRPC binary | Rust | 历史回放驱动 + 历史撮合器 + 绩效分析；装配历史版 DataFeed / ExecutionHandler / Clock 注入 engine |
| `tg-signal-engine` | 服务 crate + gRPC binary | Rust | 一组 `Strategy` 实现；编排 indicators + factors 产 `Signal`；横截面选股 |

依赖关系图：
```
                           tg-contracts (proto + 领域类型，权威)
                                  ▲
                                  │ 链接
            ┌─────────────────────┼─────────────────────┐
            │                     │                     │
       tg-persistence ◀────── tg-engine ◀──────── tg-factor-engine
       (PG + Parquet 读)        │  (trait/事件循环)    (因子值/评估)
            ▲                    │
            │ 注入历史回放/撮合     │ 链接复用
            │                    │
       tg-backtest ───────────────┤
       (回放器/历史撮合器/绩效)     │
                                 │
                          tg-signal-engine ───gRPC──▶ tg-indicators (C++)
                          (Strategy 实现/规则引擎)    (MACD/RSI/... 计算服务)
```

横向数据流（运行期）：
```
回测路径：  persistence(历史 Bar) ─▶ BacktestReplay(DataFeed) ─▶ Engine 事件循环
                                                                  │
                                                                  ▼
                                                    HistoricalMatcher(ExecutionHandler)
                                                                  │ Fill
                                                                  ▼
                                                          绩效分析 → persistence(回测记录)

信号路径：  persistence(Bar) ─▶ Engine(DataFeed=回放或 Phase3 实时) ─▶ SignalStrategy::on_bar
                                                                    │ gRPC 调 indicators / 链接 factor-engine
                                                                    ▼
                                                            规则引擎 → Signal 事件
                                                                    │
                                                                    ▼ 发布
                                                       SignalService::SubscribeSignals(stream)
                                                                    │ Phase 3
                                                                    ▼
                                                            decision-agent (最终决策者)
```

---

## 3. tg-engine 详细设计

### 3.1 设计原则
1. **引擎零业务**：`tg-engine` 只定义抽象与调度，不含任何策略逻辑、撮合规则、A 股规则落地（这些由注入器实现）。
2. **两种模式同一装配**：回测 / 模拟的差异完全封装在四个注入器（`DataFeed` / `ExecutionHandler` / `Clock` / `Strategy` 集合）里，事件循环代码不变（ADR-006）。
3. **策略只读组合视图**：策略不直接访问 `Account` 内部状态，只经 `Portfolio` 视图读取快照，写操作必须经 `ExecutionHandler` 投递 `Order`——保证回测与实盘的下单路径一致。
4. **事件确定性**：事件队列按 `(timestamp, kind_priority, seq)` 稳定排序，回测可逐事件复现。

### 3.2 核心抽象：四个 trait

#### 3.2.1 `Strategy` —— 策略回调契约
策略实现者面向此 trait 编程。回调返回的是**订单意图**（`OrderIntent`，非最终 `Order`），由事件循环交执行器裁决。

```rust
// OrderIntent 权威定义见 tg-contracts §2.3（策略/决策产出，由 ExecutionHandler 落地为 Order）

/// 策略可向引擎声明的需求：定时器节奏、关注的 symbol/period
pub struct StrategyContext<'a> {
    pub now: DateTime<Utc>,
    pub clock: &'a dyn Clock,
    pub portfolio: &'a dyn Portfolio,    // 只读组合视图
    pub cross_section: &'a dyn CrossSection, // 横截面快照
    pub broker: &'a dyn OrderSink,        // 投递订单意图的出口
}

#[async_trait]
pub trait Strategy: Send + Sync {
    /// 引擎启动前调用一次：注册关注标的、定时器、加载持久状态
    async fn on_init(&mut self, ctx: &mut StrategyContext<'_>) -> Result<()>;

    /// 历史/回测路径：新 K 线收盘触发（按 period 分别回调或合并由实现决定）
    async fn on_bar(&mut self, bar: &Bar, ctx: &mut StrategyContext<'_>) -> Result<()>;

    /// 模拟/实时路径：秒级快照触发；回测路径若 DataFeed 不产快照则不回调
    async fn on_snapshot(&mut self, snap: &Snapshot, ctx: &mut StrategyContext<'_>) -> Result<()>;

    /// 定时器触发（由策略在 on_init 注册：如每分钟、收盘前 5 分钟）
    async fn on_timer(&mut self, at: DateTime<Utc>, ctx: &mut StrategyContext<'_>) -> Result<()>;

    /// 订单成交通知（来自 ExecutionHandler 的 Fill 回灌）
    async fn on_fill(&mut self, fill: &Fill, ctx: &mut StrategyContext<'_>) -> Result<()>;

    /// 引擎停止前调用：持久化策略内部状态、产出总结
    async fn on_shutdown(&mut self, ctx: &mut StrategyContext<'_>) -> Result<()>;

    /// 该策略的标签（用于订单溯源、绩效分组）
    fn style(&self) -> StrategyStyle;
}
```

> 注：所有共享类型（`Bar`/`Snapshot`/`Fill`/`Exchange`/`OrderSide`/`OrderType`/`TimeInForce`/`StrategyStyle`/`Decimal`/`DateTime<Utc>`）均来自 `tg-contracts`，本 spec 仅引用。

#### 3.2.2 `ExecutionHandler` —— 下单与撮合切换点（ADR-009）
```rust
#[async_trait]
pub trait ExecutionHandler: Send + Sync {
    /// 提交订单意图，返回分配的 OrderId（回测内同步撮合、模拟异步接受；
    /// 成交统一经 fill_channel 回灌，不在返回值携带 Fill）。
    async fn submit(&self, intent: OrderIntent) -> Result<OrderId, TgError>;

    /// 撤单（按 OrderId 幂等）
    async fn cancel(&self, order_id: &OrderId) -> Result<(), TgError>;

    /// 当前持仓快照（供 Portfolio 视图组装）
    async fn snapshot_positions(&self) -> Result<Vec<Position>, TgError>;

    /// 当前账户快照
    async fn snapshot_account(&self) -> Result<Account, TgError>;

    /// 成交广播通道（引擎订阅；回测/模拟统一经此通道回灌 Fill）
    fn fill_channel(&self) -> tokio::sync::broadcast::Receiver<Fill>;
}
```
- **回测实现**：`HistoricalMatcher`（在 `tg-backtest`，§4.3）——`submit` 同步按当前 Bar 的 OHLC 撮合，把 `Fill` 即时推入 `fill_channel`。
- **模拟实现**：`tg-mock-order-engine`（Phase 3）——`submit` 先接受返回 `OrderId`，由盘口快照驱动异步撮合并推入 `fill_channel`。
- **未来实盘实现**：`tg-broker-gateway`（Phase 5+）——同一 trait，切注入器即升级（ADR-009）。

#### 3.2.3 `DataFeed` —— 行情来源切换点
```rust
#[async_trait]
pub trait DataFeed: Send + Sync {
    /// 阻塞拉取下一个事件（按时间顺序）。返回 None 表示数据耗尽（回测结束）。
    async fn next_event(&mut self) -> Result<Option<Event>>;

    /// 预告下个事件的时间戳（供 Clock/Timer 对齐，可选）
    async fn peek_next_ts(&mut self) -> Result<Option<DateTime<Utc>>>;
}
```
- **回测实现**：`BacktestReplay`（在 `tg-backtest`）——从 persistence 读历史 Bar，按 `ts` 升序回放，多标的按时间归并。
- **模拟实现**（Phase 3）：实时版 `DataFeed`——消费 `tg-market-data` 实时快照轮询结果，封装为 `Event::Snapshot`。

#### 3.2.4 `Clock` —— 时间来源切换点
```rust
#[async_trait]
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn trading_date(&self, ts: DateTime<Utc>) -> NaiveDate;   // CST 交易日
}
```
- **回测实现**：`HistoricalClock`——`now()` 返回当前回放事件的时间戳（驱动策略看到"历史当下"）。
- **模拟实现**：`WallClock`——`now()` 返回系统墙钟。

### 3.3 事件循环（核心调度器）

```rust
pub struct Engine {
    feed: Box<dyn DataFeed>,
    executor: Arc<dyn ExecutionHandler>,
    clock: Arc<dyn Clock>,
    strategies: Vec<Box<dyn Strategy>>,
    timer_queue: BinaryHeap<TimerEntry>,          // 策略注册的定时器（按时间倒序弹出最早）
    fill_rx: tokio::sync::broadcast::Receiver<Fill>, // 订阅 ExecutionHandler::fill_channel 回灌的成交
    portfolio: Arc<dyn Portfolio>,
    cross_section: Arc<dyn CrossSection>,
    seq: u64,                                      // 全局事件序号（确定性 tie-break）
}

impl Engine {
    pub async fn run(&mut self) -> Result<RunSummary> {
        // 1. on_init：所有策略注册关注/定时器
        for s in &mut self.strategies { s.on_init(&mut self.ctx()).await?; }

        // 2. 主循环：按 (timestamp, kind_priority, seq) 稳定排序消费
        loop {
            let next = self.pick_next_event().await?;   // 见 ADR-025 优先级
            match next {
                None => break,                          // 回测回放耗尽
                Some(Event::Bar(b))     => { self.advance_clock(b.ts); self.update_cross_section_bar(&b);
                                              for s in &mut self.strategies { s.on_bar(&b, &mut self.ctx()).await?; } }
                Some(Event::Snapshot(s))=> { self.advance_clock(s.ts); self.update_cross_section_snap(&s);
                                              for s in &mut self.strategies { s.on_snapshot(&s, &mut self.ctx()).await?; } }
                Some(Event::Timer(t))   => { for s in &mut self.strategies { s.on_timer(t, &mut self.ctx()).await?; } }
                Some(Event::Fill(f))    => { self.portfolio.apply_fill(&f);
                                              for s in &mut self.strategies { s.on_fill(&f, &mut self.ctx()).await?; } }
            }
            // 3. 排空成交回灌（执行器可能在 submit 后立即产生 Fill）
            self.drain_fills().await?;
            // 4. 排空到期定时器（ts <= clock.now()）
            self.drain_due_timers().await?;
        }

        // 5. on_shutdown
        for s in &mut self.strategies { s.on_shutdown(&mut self.ctx()).await?; }
        Ok(self.summary())
    }
}
```

**事件优先级（ADR-025）**：同一时间戳下，处理顺序固定为 `Fill < Bar < Snapshot < Timer`——即先消化成交（更新持仓）再处理新行情，避免策略用过期持仓下单。`seq` 作为同优先级内的稳定 tie-break，保证逐事件可复现。

### 3.4 组合视图（Portfolio）与 OrderSink
```rust
/// 策略只读视图（不可直接改 Account）
#[async_trait]
pub trait Portfolio: Send + Sync {
    fn account(&self) -> &Account;
    fn position(&self, symbol: &str) -> Option<&Position>;
    fn positions(&self) -> impl Iterator<Item = &Position>;
    fn open_orders(&self) -> impl Iterator<Item = &Order>;
    fn apply_fill(&self, fill: &Fill);   // 仅供引擎内部回灌调用
}

/// 策略下单出口（封装 ExecutionHandler，便于风控/审计插桩）
#[async_trait]
pub trait OrderSink: Send + Sync {
    async fn submit(&self, intent: OrderIntent) -> Result<OrderId, TgError>;
    async fn cancel(&self, order_id: &OrderId) -> Result<(), TgError>;
}
```
`StrategyContext.broker` 的默认实现把 `submit` 转发到 `ExecutionHandler`，并在 Phase 3 可被装饰为带风控否决的 `RiskCheckedSink`（本 Phase 不实现风控层）。

### 3.5 横截面快照（CrossSection）
多标的策略（如打板扫全 watchlist、横截面选股）需要"某时刻全 universe 数据视图"。
```rust
#[async_trait]
pub trait CrossSection: Send + Sync {
    /// 取当前时刻（clock.now()）所有关注标的的最新 Bar/Snapshot
    fn latest_bar(&self, symbol: &str) -> Option<&Bar>;
    fn latest_snapshot(&self, symbol: &str) -> Option<&Snapshot>;
    fn universe(&self) -> impl Iterator<Item = &str>;
}
```
- 引擎在每收到一个 `Bar`/`Snapshot` 后更新内部 `latest_by_symbol` 表，供策略在 `on_bar`/`on_snapshot`/`on_timer` 中读取全市场切片。
- `signal-engine` 的横截面选股直接依赖此视图（§5.5）。

### 3.6 两种运行模式如何切换
**仅装配不同，事件循环代码完全相同**：
```rust
// 回测模式（本 Phase 在 tg-backtest 装配）
let engine = Engine::new()
    .with_feed(BacktestReplay::from_persistence(...))
    .with_executor(HistoricalMatcher::new(rules))
    .with_clock(HistoricalClock::new())
    .with_strategies(vec![Box::new(my_strategy)])
    .build();

// 模拟模式（Phase 3 在 tg-mock-order-engine 装配；本 Phase 仅占位说明）
let engine = Engine::new()
    .with_feed(LiveSnapshotFeed::from(market_data_channel))   // Phase 3 实现
    .with_executor(MockOrderEngine::new(account))             // Phase 3 实现
    .with_clock(WallClock::new())
    .with_strategies(vec![Box::new(my_strategy)])
    .build();
```
两条路径下，`my_strategy` 是同一份代码——这是 ADR-006 的核心承诺，也是 Phase 2 验收的硬指标。

---

## 4. tg-backtest 详细设计

### 4.1 回放器（BacktestReplay，DataFeed 的历史实现）

```rust
pub struct BacktestReplay {
    /// 按时间归并的多标的多周期 Bar 流；内存或流式从 persistence 拉
    bars: MergeHeap<Bar>,                          // 按 ts 升序归并
    /// 预读的交易日历，用于生成 Timer（如"收盘前 5 分钟"）
    calendar: Vec<NaiveDate>,
    /// 策略注册的定时器需求（on_init 时收集）
    timer_schedules: Vec<TimerSchedule>,
}

impl BacktestReplay {
    pub fn from_persistence(
        repo: &dyn BarRepo,
        universe: &[String],
        periods: &[BarPeriod],
        range: Range<DateTime<Utc>>,
        adjustment: Adjustment,
    ) -> Result<Self>;

    fn emit_timer_events(&mut self, before_ts: DateTime<Utc>) -> Vec<Event>; // 在两条 Bar 之间插入到期定时器
}
```

**回放语义（ADR-025）**：
- Bar 按 `(ts, symbol)` 升序回放；多标的同时刻 Bar 以 `symbol` 字典序为次级排序（确定性）。
- 当某个策略在 `on_init` 注册了定时器（如"每个交易日 14:55 CST"），回放器在两条 Bar 之间插入对应的 `Event::Timer`，确保 `on_timer` 在历史时间轴上被正确触发。
- **回测只回放 Bar**（不含 Snapshot），故策略的 `on_snapshot` 在回测路径不会被调用——依赖 Snapshot 的策略（打板）在回测路径需降级为用分钟 Bar 近似，或在 Phase 3 用实时数据验证（见 §4.6 局限）。

### 4.2 历史撮合器（HistoricalMatcher，ExecutionHandler 的回测实现）

#### 4.2.1 撮合输入与触发
撮合器持有当前正在处理的 Bar（由事件循环在 `on_bar` 前注入），策略在 `on_bar` 内 `submit(OrderIntent)` 时，撮合器用**当前 Bar 的 OHLC** 判断是否成交（ADR-026，OHLC 精度模型）。

```rust
pub struct HistoricalMatcher {
    rules: ASHareRules,            // A 股规则引擎（涨跌停/T+1/手数/费用）
    current_bar: HashMap<String, Bar>,  // symbol -> 当前 Bar
    account: Account,              // 虚拟账户（回测专用）
}
```

#### 4.2.2 成交判定（ADR-026：保守 OHLC 模型）
- **市价单**：以 Bar 的 `open` 价成交（假设策略在收到上一根 Bar 收盘后、下一根开盘下单）。若开盘即涨跌停且方向不利 → 部分成交或拒绝。
- **限价单**：
  - 买单 `price >= Bar.low`：成交，成交价 = `min(price, Bar.open)`（若开盘价已低于限价，以更优的开盘价成交）；否则不成交。
  - 卖单 `price <= Bar.high`：成交，成交价 = `max(price, Bar.open)`；否则不成交。
- **涨跌停约束**：若当日 `close` 已触及涨跌停板，且订单方向与封板方向相同 → 拒绝（封板买不到/卖不出）。
- **成交数量**：受 `Bar.volume` 约束（可选，配置项 `volume_cap`，默认关闭——回测中假定小单不影响盘口）。

> **精度取舍（ADR-026）**：选用 OHLC 四点模型而非 tick 级。理由：A 股免费历史数据最高到分钟 K，tick 不可得；OHLC 模型在小单、低频（日/分钟）回测中误差可接受；打板等高频策略回测结果仅作参考，最终在 Phase 3 实时验证。

#### 4.2.3 A 股规则落地（rules: ASHareRules）
撮合器在 `submit` 时按序执行下列校验，任一失败即返回 `Err(TgError::InvalidOrder(...))` 并不入挂单：

| 规则 | 校验逻辑 |
|---|---|
| **手数** | `quantity % LOT_SIZE == 0 && quantity > 0`（`LOT_SIZE=100`） |
| **价格档位** | A 股报价 0.01 元最小变动单位（`Decimal` 量化到 2 位小数） |
| **涨跌停** | `limit_price` 必须落在 `[pre_close*(1-pct), pre_close*(1+pct)]`；`pct = limit_up_pct(board)` |
| **T+1**（股票） | 卖出时检查 `Position.available_quantity`（扣除当日买入），不足拒绝；ETF 视品种放宽（ADR-013） |
| **集合竞价** | 14:57-15:00 提交的订单标记为收盘集合竞价，撮合价统一用收盘价 |
| **资金** | 买单：`cash >= price*quantity + 预估费用`；否则按可用资金反算最大可买手数或拒绝（配置项） |

#### 4.2.4 费用模型（精确 Decimal）
每笔 `Fill` 携带三类费用，全部 `Decimal`：
```rust
fn compute_cost(side: OrderSide, price: Decimal, qty: i64, exchange: Exchange) -> CostBreakdown {
    let notional = price * Decimal::from(qty);
    // 佣金：min(notional * COMMISSION_RATE, COMMISSION_MAX_PCT*notional)，且不少于 5 元
    let commission = (notional * commission_rate).max(dec!(5));
    // 印花税：仅卖出，0.05%（ETF 免）
    let tax = if matches!(side, OrderSide::Sell) && exchange != Etf { notional * STAMP_DUTY_PCT } else { dec!(0) };
    // 过户费：仅沪市，0.001%
    let transfer = if matches!(exchange, Exchange::Sh) { notional * TRANSFER_FEE_PCT } else { dec!(0) };
    CostBreakdown { commission, tax, transfer }
}
```
`Fill` 的 `commission / tax / transfer_fee` 三字段回写到 `Account` 与绩效统计。

### 4.3 绩效指标（PerformanceAnalytics）

#### 4.3.1 净值曲线与收益率序列
- **每日快照**：每个交易日收盘后记录 `{ date, total_value }`，`total_value = cash + Σ position.market_value`（持仓按当日收盘价估值）。
- **日收益率序列** `r_t = total_value_t / total_value_{t-1} - 1`（`f64`）。
- **基准对比**：同步计算基准（如沪深 300 / 自选等权）的日收益率序列，用于超额收益。

#### 4.3.2 指标公式（ADR-027：绩效口径）
所有比率用 `f64`，输入价格用 `Decimal`：
```rust
pub struct PerformanceReport {
    pub total_return: f64,         // (末值/初值 - 1)
    pub annualized_return: f64,    // (1+total_return)^(252/n_days) - 1
    pub sharpe: f64,               // mean(r_t)/(std(r_t)) * sqrt(252)；无风险利率默认 0，可配
    pub max_drawdown: f64,         // max((peak - trough)/peak)；同时返回起止日期
    pub win_rate: f64,             // 盈利交易笔数 / 总平仓笔数
    pub profit_loss_ratio: f64,    // 平均盈利 / 平均亏损（绝对值）
    pub total_trades: usize,
    pub commission_total: Decimal,
    pub tax_total: Decimal,
    pub benchmark_return: f64,     // 基准同期收益
    pub alpha: f64,                // 超额收益（年化）相对基准
    pub beta: f64,                 // 对基准的回归系数
    pub equity_curve: Vec<(NaiveDate, Decimal)>,
}
```
**口径约定（ADR-027）**：
- 无风险利率默认 0；夏普年化用 `sqrt(252)`（日频）；高频策略可配 `sqrt(252*N)` 其中 N 为每日 bar 数。
- `max_drawdown` 基于**日 total_value 序列**（非逐笔），与主流回测平台一致。
- 胜率/盈亏比按**完整平仓周期**统计（一笔开仓+平仓算一笔交易），而非逐笔 Fill。
- 仓位为零的"空仓日"仍计入持有期（影响年化分母）。

### 4.4 运行记录 schema（持久化到 persistence）
回测配置 + 结果落 PostgreSQL，供多次对比（ADR-017 链接 persistence 写入）：
```sql
CREATE TABLE backtest_runs (
    run_id          TEXT PRIMARY KEY,          -- ULID
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    strategy_tag    TEXT NOT NULL,             -- swing/t0/limitup
    universe        TEXT[] NOT NULL,           -- [symbol...]
    periods         TEXT[] NOT NULL,
    range_start     TIMESTAMPTZ NOT NULL,
    range_end       TIMESTAMPTZ NOT NULL,
    adjustment      TEXT NOT NULL,
    config_json     JSONB NOT NULL,            -- 策略参数、费用参数、撮合配置
    status          TEXT NOT NULL,             -- pending/running/done/failed
    error           TEXT,
    -- 结果（status=done 时填充）
    total_return    DOUBLE PRECISION,
    annualized_return DOUBLE PRECISION,
    sharpe          DOUBLE PRECISION,
    max_drawdown    DOUBLE PRECISION,
    win_rate        DOUBLE PRECISION,
    profit_loss_ratio DOUBLE PRECISION,
    total_trades    INT,
    commission_total NUMERIC(18,4),
    tax_total       NUMERIC(18,4),
    benchmark_return DOUBLE PRECISION,
    equity_curve_json JSONB,                   -- 完整曲线单独存 JSONB（或 Parquet）
    trades_json     JSONB                      -- 逐笔交易明细（可选，量大时落 Parquet）
);
```
> 注：equity_curve 与 trades 在数据量大时改落 Parquet（`data/backtest/run_id=<id>/`），表中只存汇总指标——与 Phase 0 Parquet 布局风格一致。

### 4.5 gRPC `BacktestService`
```protobuf
service BacktestService {
  rpc SubmitBacktest(BacktestRequest) returns (BacktestJob);     // 异步，立即返回 run_id
  rpc GetBacktestStatus(BacktestJobId) returns (BacktestStatus);  // pending/running/done/failed + 进度
  rpc GetBacktestResult(BacktestJobId) returns (BacktestResult);  // 完整 PerformanceReport
  rpc ListBacktestRuns(ListFilter) returns (BacktestRunSummary);  // 历史运行对比
}

message BacktestRequest {
  string strategy_tag = 1;            // StrategyStyle 枚举字符串
  repeated string universe = 2;
  repeated string periods = 3;
  string range_start = 4;             // RFC3339
  string range_end = 5;
  string adjustment = 6;
  string strategy_config_json = 7;    // 策略参数
  string matcher_config_json = 8;     // 撮合/费用/滑点配置
  string benchmark = 9;               // 基准 symbol 或 "equal_weight_universe"
}
```

### 4.6 回测路径的已知局限（诚实披露）
- **打板策略**：依赖实时秒级 Snapshot（封板检测、尾盘买），回测路径只有分钟/日 Bar，**回测结果仅作信号规则正确性验证，不能作为收益预期**。打板的收益预期在 Phase 3 实时纸面模拟中验证。
- **T0 做T**：日内多笔对冲在分钟 Bar 粒度下可回测，但成交价用 OHLC 模型近似，与实时盘口撮合有偏差。
- **市场冲击**：默认 `volume_cap=off`，假定订单不移动盘口（watchlist 小单假设）。

---

## 5. tg-signal-engine 详细设计

### 5.1 定位与约束（ADR-010）
`tg-signal-engine` 是**规则驱动的候选产生者**：
- 实现 `tg-engine` 的 `Strategy` trait，但**不直接下单**——`OrderSink` 被替换为只收集 `Signal` 的 `SignalCollector`（不投递 `OrderIntent`）。
- 产出 `Signal` 事件经 `SignalService` 发布，由 Phase 3 `decision-agent` 消费拍板。
- 每条 `Signal` 必带 `reason: Vec<String>` 审计理由（ADR-028，可解释性）。
- 信号强度 `strength` / 置信度 `confidence` 是规则引擎产出的归一化分数（0.0~1.0），供 agent 排序与风控参考。

### 5.2 规则引擎（RuleEngine）
规则引擎把"指标值 + 因子值 + 市场状态"组合成布尔/打分表达式，触发 `Signal`。

```rust
/// 单个原子条件（可解释、可序列化）
pub enum Condition {
    IndicatorCross { indicator: String, fast_param: String, slow_param: String, direction: CrossDirection },
    IndicatorThreshold { indicator: String, param: String, op: CmpOp, value: f64 },
    FactorRank { factor: String, top_pct: f64 },          // 横截面排名前 N%
    FactorThreshold { factor: String, op: CmpOp, value: f64 },
    PriceRelative { ref_field: PriceRef, op: CmpOp, pct: f64 }, // 如 close > pre_close*1.095
    MarketState { is_call_auction: bool, near_close_minutes: Option<i32> },
}

pub enum LogicOp { And, Or, Not }

/// 规则 = 条件树（AND/OR/NOT 组合）
pub struct Rule {
    pub id: String,
    pub style: StrategyStyle,
    pub direction: SignalDirection,        // 触发时的方向（Long/CloseLong/...）
    pub condition: ConditionTree,
    pub strength_fn: StrengthFn,           // 由命中的条件子集计算 strength
    pub confidence_fn: ConfidenceFn,       // 由历史命中率/条件数计算 confidence
}

pub struct RuleEngine {
    rules: Vec<Rule>,
    indicator_client: Arc<dyn IndicatorRpc>,   // gRPC 调 tg-indicators
    factor_source: Arc<dyn FactorSource>,      // 链接 tg-factor-engine 读横截面/时序因子
}

impl RuleEngine {
    /// 输入：当前 Bar/Snapshot + 横截面快照；输出：命中的 Signal 列表（带 reason）
    pub fn evaluate(&self, ctx: &EvalContext) -> Vec<Signal>;
}
```
**reason 编码（ADR-028）**：每个 `Signal.reason` 是触发条件的**人类可读字符串列表**，采用固定结构化格式以便审计与 agent 理解，例如：
```
["indicator:MACD golden cross (fast=12, slow=26)",
 "indicator:RSI(14)=28.3 < 30 (oversold)",
 "factor:momentum_20d rank top 8% (rank=3/45)"]
```
格式为 `"<类型>:<名称> <描述> (<参数/数值>)"`，便于 grep、统计与 LLM 解析。

### 5.3 三套策略原型（ADR-014）的具体信号规则

每个原型是一个 `Strategy` 实现 + 关联的 `Rule` 集合。下表给出**典型信号条件**（实现期可配，参数存 `strategy_config_json`）。

#### 5.3.1 波段策略（SwingStyle）
- **驱动频率**：日 K（主）+ 分钟 K（精确入场）
- **持仓周期**：1-5 天
- **典型开仓信号（Long）**：
  - MACD 金叉（DIF 上穿 DEA）
  - AND RSI(14) ∈ (30, 50)（不超买、刚启动）
  - AND 成交量较前 5 日均量放大 ≥ 1.5 倍
  - AND 动量因子 `momentum_20d` 横截面排名前 20%
- **典型平仓信号（CloseLong）**：
  - MACD 死叉，OR RSI(14) > 70（超买），OR 触发固定 ATR 止损（`stop = entry_price - 2*ATR(14)`），OR 持仓达 5 日时间止盈/止损
- **`reason` 示例**：`["indicator:MACD golden cross", "indicator:RSI(14)=42.1", "factor:momentum_20d rank 12%", "volume:1.8x avg5"]`
- **回测可行性**：✅ 日/分钟 Bar 即可，本 Phase 完整可回测验证。

#### 5.3.2 T0 做T 策略（T0Style）
- **驱动频率**：分钟 K + 实时快照（Phase 3 完整验证，本 Phase 用分钟 Bar 回测近似）
- **前提**：已有昨日持仓（T0 是对存量做日内高抛低吸）
- **典型卖出信号（日内高抛）**：分钟 RSI(6) > 80 AND 价格相对日内 VWAP 上偏 ≥ 1% → 卖出 `available_quantity` 的一部分
- **典型回买信号（日内低吸）**：分钟 RSI(6) < 20 AND 价格相对日内 VWAP 下偏 ≥ 1% → 回买（不超过当日卖出量，确保净持仓不下降到负）
- **A 股 T+1 落地**：卖出只能用 `Position.available_quantity`（昨日持仓），回买的当日新仓 T+1 锁定——规则引擎从 `Portfolio` 读持仓可用量约束建议数量。
- **`reason` 示例**：`["indicator:RSI(1m)=83.2 > 80", "price:1.2% above VWAP", "position:available=500 (yesterday)"]`
- **回测可行性**：⚠️ 分钟 Bar 可验证信号规则正确性，但成交价用 OHLC 近似；实时盘口撮合效果待 Phase 3。

#### 5.3.3 打板策略（LimitUpStyle）
- **驱动频率**：实时秒级快照（**本 Phase 信号规则定义完整，回测仅验证规则逻辑**）
- **开仓信号（涨停封板检测）**：
  - 当前价 == 涨停价（`pre_close * (1 + limit_up_pct(board))`）
  - AND 买一档挂单量（`bid_volume[0]`）持续 ≥ 阈值（如 10 万手），即封单坚实
  - AND 封板时长 ≥ N 分钟（避免"炸板"）
  - AND 换手率在合理区间（避免一字无量板买不到）
  - AND 该股非 ST、流通市值在配置区间内
  - → 触发时间窗：**尾盘 14:55-15:00 集合竞价**（ADR-014：尾盘买），次日开盘卖出
- **平仓信号（次日卖出）**：
  - 次日开盘价高于昨日涨停价 → 开盘即卖（市价/限价）
  - 次日高开低走跌破前日涨停价 → 止损卖
- **`reason` 示例**：`["price:sealed at limit_up (9.99%)", "orderbook:bid1=18.5万手 (solid seal, 12min)", "turnover:3.2% in range", "timing:14:57 call auction"]`
- **回测可行性**：❌ 秒级 Snapshot 历史数据不可得，**本 Phase 只能定义与单元测试规则逻辑**（用构造的 Snapshot fixture），收益预期完全留待 Phase 3 实时验证。回测路径下用日 K 不可信，故打板策略**不纳入回测绩效统计**，仅在 signal-engine 内做规则单元测试。

### 5.4 横截面选股打分
对每套策略，规则引擎结合 `CrossSection` 视图做全 universe 打分：
```rust
impl SignalEngine {
    fn score_universe(&self, style: StrategyStyle, cs: &dyn CrossSection) -> Vec<ScoredSymbol> {
        // 1. 对 universe 内每个 symbol 跑该 style 的规则
        // 2. 命中的 Signal 按 strength * confidence 排序
        // 3. 取 top-K（K 由策略配置，如波段取 top-3，打板不限）
        // 4. 过滤已持仓、黑名单、ST
    }
}
```
横截面排名类条件（如 `FactorRank{top_pct:0.1}`）直接依赖 `tg-factor-engine` 提供的横截面排名（`FactorValue.rank`）。

### 5.5 信号发布
```rust
/// signal-engine 的 Strategy 实现：on_bar/on_snapshot 内调用规则引擎，命中则发 Signal
pub struct SignalStrategy {
    engine: RuleEngine,
    collector: Arc<SignalCollector>,   // 缓冲 Signal，由 SignalService 订阅端拉取
}

#[async_trait]
impl Strategy for SignalStrategy {
    async fn on_bar(&mut self, bar: &Bar, ctx: &mut StrategyContext<'_>) -> Result<()> {
        let signals = self.engine.evaluate(&EvalContext::from_bar(bar, ctx.cross_section));
        for s in signals { self.collector.publish(s).await; }
        Ok(())
    }
    // on_snapshot 同理（打板/T0 用）
    fn style(&self) -> StrategyStyle { /* ... */ }
}
```
- `SignalCollector` 内部用 `tokio::sync::broadcast` 或 `mpsc`，`SignalService::SubscribeSignals` 从它读流推给订阅者。
- **signal-engine 不持有 OrderSink 的真实实现**——它注入一个 `NoopSink`（submit 直接丢弃），强制保证不直接下单（ADR-010 的代码级兜底）。

### 5.6 gRPC `SignalService`
```protobuf
service SignalService {
  rpc SubscribeSignals(SignalFilter) returns (stream Signal);   // 实时推送命中信号
  rpc QuerySignals(SignalQuery) returns (SignalPage);           // 按时间/symbol/style 查历史信号
}

message SignalFilter {
  repeated string styles = 1;        // StrategyStyle 过滤，空=全部
  repeated string symbols = 2;       // 空=全 universe
  double min_strength = 3;
}

message SignalQuery {
  string range_start = 1;
  string range_end = 2;
  repeated string symbols = 3;
  string style = 4;
  int32 limit = 5;
}
```
`Signal` proto message 与 contracts §2.5 字段一一对应（`id/symbol/exchange/direction/strength/confidence/style/reason/suggested_quantity/ts/trading_date`）。

---

## 6. 接口定义汇总

### 6.1 tg-engine 对外 trait（library crate 公共 API）
| trait | 方法 | 实现方 |
|---|---|---|
| `Strategy` | on_init / on_bar / on_snapshot / on_timer / on_fill / on_shutdown / style | 策略开发者（signal-engine、未来用户策略） |
| `ExecutionHandler` | submit / cancel / snapshot_positions / snapshot_account / fill_channel | tg-backtest(回测) / tg-mock-order-engine(Phase3) / tg-broker-gateway(未来) |
| `DataFeed` | next_event / peek_next_ts | tg-backtest(回放) / tg-mock-order-engine(实时, Phase3) |
| `Clock` | now / trading_date | HistoricalClock / WallClock |
| `Portfolio` | account / position / positions / open_orders / apply_fill | engine 内部默认实现 |
| `CrossSection` | latest_bar / latest_snapshot / universe | engine 内部默认实现 |
| `OrderSink` | submit / cancel | engine 默认转发 ExecutionHandler；Phase3 装饰风控 |

### 6.2 gRPC service（proto 定义在 tg-contracts/proto/tg/）
| service | 所属模块 | Phase |
|---|---|---|
| `BacktestService` | tg-backtest | 2 |
| `SignalService` | tg-signal-engine | 2 |

（`IndicatorService` / `FactorService` 为 Phase 1，本 Phase 作为消费方调用；proto 名见 contracts §3。）

---

## 7. 错误处理与可观测性

### 7.1 错误分层
- **trait 内部错误**：用各 crate 的具名 `thiserror` 错误（如 `EngineError::FeedExhausted` / `MatcherError::LimitHitRejected` / `SignalError::RuleConfigInvalid`）。
- **回测/信号服务 binary 边界**：`anyhow::Result` 聚合。
- **gRPC 边界**：映射到 tonic `Status`：
  - `InvalidArgument`：策略配置 / 撮合配置非法
  - `NotFound`：run_id / signal_id 不存在
  - `FailedPrecondition`：回测所需的 Bar 数据在 persistence 中缺失
  - `Internal`：其他未分类错误

### 7.2 可观测性
- **结构化日志（tracing）**：每个事件循环迭代记录 `event_kind / symbol / seq / ts / strategy_style`；每次订单提交记录 `client_order_id / side / qty / order_id`；每次成交经 `fill_channel` 记录 `order_id / fill_price / fill_qty`。
- **指标（Prometheus，本期预留）**：
  - `tg_engine_events_processed_total{kind}`
  - `tg_backtest_runs_active` / `tg_backtest_run_duration_seconds`
  - `tg_signals_emitted_total{style,direction}`
  - `tg_matcher_rejections_total{reason}`（涨跌停/资金/T+1 等）
- **回测可复现性**：每个 `backtest_runs` 记录含完整 `config_json` + `matcher_config_json`，相同配置 + 相同数据 → 相同结果（确定性，ADR-025）。

---

## 8. 测试策略

### 8.1 单元测试（确定性）
- **撮合规则对拍**：构造固定 OHLC Bar，验证市价/限价单的成交价与数量、涨跌停拒绝、T+1 拒绝、手数校验、费用计算——每个规则独立 case。
- **绩效计算对拍**：用已知净值序列（如 `[100, 105, 99, 110]`）手算 total_return / sharpe / max_drawdown / win_rate，与 `PerformanceAnalytics` 输出比对（黄金值回归）。
- **规则引擎**：构造命中/未命中 fixture，验证 `Signal.reason` 字符串格式（ADR-028）与 strength/confidence 计算。
- **打板规则逻辑**：用构造的 `Snapshot`（涨停价 + 大封单 + 尾盘时间）验证封板检测条件，**不依赖真实历史秒级数据**。
- **事件循环确定性**：同一组 Event 输入，多次运行得到完全相同的 Fill 序列与最终 Account（验证 ADR-025 的 tie-break 稳定性）。

### 8.2 集成测试
- **mock DataFeed 跑完整回测**：注入内存版 `BacktestReplay`（喂预设 Bar 序列）+ `HistoricalMatcher` + 一个确定性 `Strategy`（如"每根 Bar 固定买入 100 股"），验证端到端 Fill 流、Account 演进、绩效报告字段完整。
- **跨模块集成**：signal-engine 链接 mock indicators（返回固定 MACD/RSI 值）+ mock factor-engine，验证从 Bar 输入到 `Signal` 发布的完整链路。
- **确定性策略黄金值回归**：一组固定的历史 Bar fixture（手工构造或从 persistence 取一小段冻结），跑波段策略，把首次产出的绩效报告作为黄金值存档；后续改动若导致绩效偏离则测试失败——保护回测稳定性不被无察觉破坏。

### 8.3 冒烟测试（手动 / `#[ignore]`）
- 从真实 persistence（Phase 0 落库的 watchlist 数据）拉一段历史，跑 `BacktestService::SubmitBacktest` 端到端，验证 gRPC + 落库 + 绩效报告——本地按需运行，CI 默认跳过。

### 8.4 不纳入测试
- 打板策略的**收益**回测（数据不可得，§4.6）；仅测试规则触发逻辑。
- 实时墙钟驱动（Phase 3）。

---

## 9. 验收标准（Definition of Done）

1. `tg-engine` crate 编译通过，公开 `Strategy` / `ExecutionHandler` / `DataFeed` / `Clock` / `Portfolio` / `CrossSection` / `OrderSink` trait，且 `Engine::run` 事件循环不依赖任何具体策略/撮合/数据源实现。
2. 同一份 `Strategy` 实现（确定性买入策略）在回测装配（`BacktestReplay` + `HistoricalMatcher` + `HistoricalClock`）下能跑通并产出 `PerformanceReport`，且多次运行结果逐字节一致（确定性，ADR-025）。
3. `HistoricalMatcher` 正确实现 A 股规则：手数、涨跌停、T+1、集合竞价、佣金/印花税/过户费——单元测试覆盖各分支，费用用 `Decimal` 精确。
4. 绩效指标（total_return / annualized / sharpe / max_drawdown / win_rate / profit_loss_ratio / alpha / beta）对手算黄金值对拍通过（ADR-027）。
5. `BacktestService` 三个 RPC（Submit / GetStatus / GetResult）可用：提交异步返回 run_id，完成状态正确，结果含完整 `PerformanceReport`，落 `backtest_runs` 表可对比。
6. `tg-signal-engine` 实现三套策略原型（波段 / T0 / 打板）的规则，每条 `Signal` 带 `reason` 审计理由且格式符合 ADR-028。
7. signal-engine 注入 `NoopSink`，**代码级保证不直接下单**（grep 校验无真实 `OrderIntent` 投递路径）。
8. `SignalService::SubscribeSignals` 流式推送命中的 `Signal`，`QuerySignals` 按时间/symbol/style 查询历史。
9. 波段策略可端到端回测验证（日/分钟 Bar）；T0 策略在分钟 Bar 下信号规则可回测；打板策略规则逻辑通过单元测试（构造 Snapshot fixture），其收益回测明确标注"不可信，待 Phase 3"。
10. 单元 + 集成（mock DataFeed / mock indicators / mock factor）测试全绿；确定性策略黄金值回归测试入库。

---

## 10. 依赖的 ADR

### 已有 ADR
- **ADR-001** 交易模式：纸面模拟（本 Phase 历史撮合器是回测路径的纸面撮合）。
- **ADR-006** 回测/模拟共用引擎：`tg-engine` + 四 trait 是本 Phase 核心。
- **ADR-009** mock/实盘切换：`ExecutionHandler` 由本 Phase 定义，回测实现 + Phase 3 模拟实现 + 未来实盘实现共用。
- **ADR-010** 决策权归属：signal-engine 仅产候选、不直接下单（代码级 `NoopSink` 兜底）。
- **ADR-013** 标的品种：A 股 + ETF，撮合器按 `InstrumentType` 差异化费用与 T+1。
- **ADR-014** 策略风格：三套原型（波段/T0/打板），signal-engine 分别实现。
- **ADR-017** persistence 共享库 crate：backtest/signal-engine 链接 persistence 读写。

### 新增 ADR（本 Phase 立）

- **ADR-025 事件循环调度模型（确定性优先级）**
  事件按 `(timestamp, kind_priority, seq)` 稳定排序消费；同时间戳下处理顺序固定为 `Fill < Bar < Snapshot < Timer`，`seq` 作稳定 tie-break。目的：保证回测逐事件可复现，避免策略用过期持仓下单。

- **ADR-026 历史撮合器精度：保守 OHLC 四点模型**
  回测撮合按 Bar 的 OHLC 判定成交（市价单用 open，限价单用 min/max(open, limit)），不做 tick 级或盘口模拟。理由：A 股免费历史数据最高到分钟 K，tick 不可得；OHLC 模型在低频小单回测中误差可接受，高频策略（打板）收益预期留待 Phase 3 实时验证。

- **ADR-027 绩效指标口径**
  日收益率基于日 `total_value` 快照；夏普年化用 `sqrt(252)`（日频，无风险利率默认 0 可配）；`max_drawdown` 基于日序列；胜率/盈亏比按完整平仓周期统计；空仓日计入持有期。所有比率 `f64`，输入价格 `Decimal`。

- **ADR-028 Signal.reason 编码规范**
  每条 `Signal.reason` 为结构化可读字符串列表，格式 `"<类型>:<名称> <描述> (<参数/数值>)"`（如 `"indicator:RSI(14)=28.3 < 30"`）。目的：审计可解释、可 grep 统计、便于 LLM 解析。

- **ADR-029 signal-engine 的 OrderSink 替换为 NoopSink**
  signal-engine 装配 engine 时注入 `NoopSink`（submit 直接丢弃），代码级强制保证信号引擎不直接下单；真实下单路径唯一由 Phase 3 decision-agent → mock-order-engine 承担。这是 ADR-010 的代码兜底。

---

## 11. 后续 / 延期项

- **参数寻优**：网格 / 贝叶斯优化——本 Phase 仅留 `backtest_runs.config_json` 与对比查询接口，寻优算法延期至 Phase 2.5 或独立子项目。
- **滑点模型增强**：当前 OHLC 模型假定小单不冲击盘口，未来可引入 `volume_cap` 与 VWAP 滑点（数据充足后）。
- **打板/T0 的实时收益验证**：依赖 Phase 3 实时墙钟 DataFeed + mock-order-engine。
- **回测加速**：当 universe / 时间范围增大时，回放器可改为多核并行归并（当前单线程归并足以覆盖 watchlist 规模）。
- **信号回测**：把 signal-engine 产出的历史 `Signal` 作为输入，回测"若严格按信号下单"的收益——本质是一类特殊策略，可作为 Phase 2.5 的 `tg-signal-backtest`。
- **proto v2 演进**：当 `Signal` / `PerformanceReport` 字段需要破坏性变更时，按 contracts 的 v2 流程。

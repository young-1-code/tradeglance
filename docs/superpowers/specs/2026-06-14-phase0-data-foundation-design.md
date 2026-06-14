# Phase 0 数据地基 — 详细设计 Spec

> **子项目**：`tg-contracts` + `tg-market-data` + `tg-persistence`
> **状态**：设计完成，待用户评审
> **日期**：2026-06-14
> **上游文档**：`2026-06-14-tradeglance-architecture-design.md`（v0.6）
> **相关 ADR**：ADR-011 / 012 / 013 / 016 / 017 / 018

---

## 1. 概述与范围

### 1.1 目标
为整个 TradeGlance 系统提供可靠的行情数据底座：把 A 股股票与 ETF 的历史 K 线、实时秒级快照、标的元数据、复权因子按规范 schema 采入 PostgreSQL + Parquet/DuckDB，并提供统一查询接口。

### 1.2 In Scope
- 定义数据领域类型与 gRPC 控制面接口（`tg-contracts`）
- 存储层：PostgreSQL schema + Parquet 分区布局 + Repository trait（`tg-persistence`）
- 采集服务：Rust 主服务 + Python FastAPI sidecar（`tg-market-data`）
- 三种数据流：启动全量、日内增量、盘中实时
- 容错：限频、退避重试、断点续采

### 1.3 Out of Scope（YAGNI，留给后续 Phase）
- 指标计算、因子计算、信号（Phase 1/2）
- 订单/持仓/账户表（Phase 3，本 spec 仅预留 schema 扩展位）
- 全市场扫描、Level-2 tick、消息总线（按需演进）
- watchlist 的 Web UI 管理（Phase 4；本期用配置文件）
- 数据源备份/多源切换（先只用 akshare）

---

## 2. 模块形态与依赖

| 模块 | 形态 | 语言 | 角色 |
|---|---|---|---|
| `tg-contracts` | crate + proto | Rust / protobuf | 定义数据类型 + gRPC 控制面 |
| `tg-market-data` | 服务 + Python sidecar 子组件 | Rust + Python | **写入者**：采集、清洗、落库 |
| `tg-persistence` | 共享库 crate | Rust | **存储 + 访问层**：被所有模块链接 |

依赖关系：
```
tg-market-data ──链接──▶ tg-persistence ──链接──▶ tg-contracts
tg-market-data ──HTTP──▶ collector-python (FastAPI sidecar)
tg-market-data ──链接──▶ tg-contracts
（后续消费者 signal-engine/backtest 等链接 tg-persistence 读取）
```

---

## 3. 数据模型（tg-contracts）

### 3.1 枚举与常量
```rust
enum Exchange { SH, SZ, BJ }
enum InstrumentType { Stock, Etf }
enum Board { MainBoard, Star, ChiNext, Bj }   // 决定涨跌停幅度
enum BarPeriod { Daily, Min1, Min5 }
enum Adjustment { None, PreAdjust, PostAdjust }

// A 股规则常量
const LOT_SIZE: i64 = 100;                      // 最小交易单位
fn limit_up_pct(board: Board) -> f64 {          // 涨停幅度
    match board { MainBoard => 0.10, Star|ChiNext => 0.20, Bj => 0.30 }
}
// 交易时段（CST）：09:30-11:30, 13:00-15:00；集合竞价 09:15-09:25 / 14:57-15:00
```

### 3.2 领域类型
```rust
struct Instrument {
    symbol: String,        // 6 位代码，如 "000001"、"600519"、"159915"
    exchange: Exchange,
    instrument_type: InstrumentType,
    name: String,
    list_date: NaiveDate,
    delist_date: Option<NaiveDate>,
    is_st: bool,
    board: Board,
}

struct Bar {
    symbol: String,
    exchange: Exchange,
    period: BarPeriod,
    ts: DateTime<Utc>,     // UTC instant；K 线结束时间戳
    trading_date: NaiveDate, // CST 交易日（用于分区/查询）
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    volume: i64,           // 股
    amount: Decimal,       // 元
    // 存储为不复权原始值；复权在查询时换算
}

struct Snapshot {          // 实时秒级快照
    symbol: String,
    exchange: Exchange,
    ts: DateTime<Utc>,
    trading_date: NaiveDate,
    last: Decimal,
    open: Decimal, high: Decimal, low: Decimal,
    pre_close: Decimal,
    volume: i64,
    amount: Decimal,
    bid_price: [Decimal; 5], bid_volume: [i64; 5],   // 五档买盘
    ask_price: [Decimal; 5], ask_volume: [i64; 5],   // 五档卖盘
}

struct AdjustmentFactor {
    symbol: String,
    ex_date: NaiveDate,    // 除权除息日
    factor: Decimal,       // 累积复权因子
}

struct TradingCalendar {
    date: NaiveDate,
    is_trading_day: bool,
}
```

### 3.3 价格/时间约定
- 价格、金额用 `rust_decimal::Decimal`；存储映射 Arrow `Decimal128(18,4)` / Postgres `NUMERIC(18,4)`。
- 时间戳内部存 UTC（`DateTime<Utc>` / epoch millis）；`trading_date` 为 CST 交易日，用于分区与按日查询。

---

## 4. 存储设计（tg-persistence）

### 4.1 PostgreSQL Schema（元数据/状态）
```sql
CREATE TABLE instruments (
    symbol        VARCHAR(10) PRIMARY KEY,
    exchange      TEXT NOT NULL,
    instrument_type TEXT NOT NULL,
    name          TEXT NOT NULL,
    list_date     DATE,
    delist_date   DATE,
    is_st         BOOLEAN NOT NULL DEFAULT false,
    board         TEXT NOT NULL
);

CREATE TABLE watchlist (
    id            BIGSERIAL PRIMARY KEY,
    symbol        VARCHAR(10) NOT NULL REFERENCES instruments(symbol),
    strategy_tags TEXT[] NOT NULL DEFAULT '{}',   -- ['swing','t0','limitup']
    added_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (symbol)
);

CREATE TABLE trading_calendar (
    date          DATE PRIMARY KEY,
    is_trading_day BOOLEAN NOT NULL
);

CREATE TABLE adjustment_factors (
    symbol        VARCHAR(10) NOT NULL,
    ex_date       DATE NOT NULL,
    factor        NUMERIC(18,8) NOT NULL,
    PRIMARY KEY (symbol, ex_date)
);

CREATE TABLE fetch_state (
    symbol           VARCHAR(10) NOT NULL,
    period           TEXT NOT NULL,            -- daily/minute1/minute5
    last_fetched_ts  TIMESTAMPTZ,              -- 断点续采游标
    last_sync_at     TIMESTAMPTZ,
    status           TEXT NOT NULL,            -- idle/running/failed
    last_error       TEXT,
    PRIMARY KEY (symbol, period)
);

CREATE TABLE latest_snapshots (           -- 盘中实时快照最新值（O(1) 查询）
    symbol        VARCHAR(10) PRIMARY KEY,
    ts            TIMESTAMPTZ NOT NULL,
    trading_date  DATE NOT NULL,
    last          NUMERIC(18,4) NOT NULL,
    open          NUMERIC(18,4) NOT NULL,
    high          NUMERIC(18,4) NOT NULL,
    low           NUMERIC(18,4) NOT NULL,
    pre_close     NUMERIC(18,4) NOT NULL,
    volume        BIGINT NOT NULL,
    amount        NUMERIC(18,4) NOT NULL,
    bid_price     NUMERIC(18,4)[5],
    bid_volume    BIGINT[5],
    ask_price     NUMERIC(18,4)[5],
    ask_volume    BIGINT[5]
);
```
迁移用 `sqlx migrate` 版本化管理。

### 4.2 Parquet 布局（时序行情，DuckDB 可查）
```
data/
  bars/
    daily/   symbol=<SYM>/year=<YYYY>/part.parquet
    minute1/ symbol=<SYM>/year=<YYYY>/part.parquet
    minute5/ symbol=<SYM>/year=<YYYY>/part.parquet
  snapshots/
    symbol=<SYM>/date=<YYYYMMDD>/part.parquet
```
- 分区键：标的 + 周期 + 年（分钟加日期）。watchlist 规模下每个分片很小，DuckDB 谓词下推查询飞快。
- 写入策略：market-data 为**唯一 Parquet 写入者**；写入时先写临时文件再原子 rename，避免半截文件。
- 其余模块用 DuckDB **只读**挂载查询。

### 4.3 Repository Trait（Rust，屏蔽数据库细节）
```rust
#[async_trait]
trait BarRepo {
    async fn write_bars(&self, bars: &[Bar]) -> Result<()>;
    async fn query_bars(&self, q: BarQuery) -> Result<Vec<Bar>>;
}
struct BarQuery {
    symbol: String, period: BarPeriod,
    range: Range<DateTime<Utc>>,
    adjustment: Adjustment,   // 查询时按需换算
}

trait SnapshotRepo {
    async fn write_snapshot(&self, s: &Snapshot) -> Result<()>;
    async fn get_latest(&self, symbol: &str) -> Result<Option<Snapshot>>;
}

trait InstrumentRepo { /* list / get / upsert / watchlist 管理 */ }
trait CalendarRepo  { /* is_trading_day / range */ }
trait FactorRepo    { /* upsert / query factors */ }
```

### 4.4 复权换算策略
- **底层只存不复权原始 Bar + 复权因子**（单一事实来源）。
- `query_bars(adjustment=PreAdjust)`：读原始 Bar + 对应因子，在查询层换算 OHLC（volume 反向调整）。
- 实时下单/撮合用 `Adjustment::None` 原始价，避免与实时盘口不一致。

### 4.5 并发模型
- PostgreSQL：多读多写，依赖 MVCC。
- Parquet：market-data 单写；其余 DuckDB 只读。无锁冲突。

---

## 5. tg-market-data 架构

### 5.1 Rust 主服务职责
- 加载 `watchlist.yaml` → upsert watchlist
- **调度器**：驱动三种数据流（见 5.3）
- 调 sidecar 取数 → **清洗/校验**（涨跌停合理性、停牌、除权除息、缺失检测）→ **落库**
- gRPC 控制面（见 6.1）
- 限频 + 退避重试 + 写 `fetch_state`

### 5.2 Python sidecar（collector-python，FastAPI）
职责：**只取数**，不做业务逻辑。封装 akshare，返回标准化 JSON。
```
GET /health
GET /instruments?type=stock|etf         # 全市场股票/ETF 列表
GET /calendar?start=&end=               # 交易日历
GET /bars?symbol=&period=&start=&end=   # 历史 K（不复权）
GET /snapshot?symbols=a,b,c             # 实时快照（批量）
GET /adjust_factors?symbol=             # 复权因子
```
- 返回 JSON：数值价格用字符串（保精度）；时间用 ISO8601 UTC。
- sidecar 本身无状态、可水平扩展；限频在 Rust 侧统一管控。

### 5.3 三种数据流
| 模式 | 触发 | 动作 |
|---|---|---|
| **启动全量** | 首启 / 新增 watchlist 标的 | 拉 日K 5 年 + 分钟K（akshare 可得范围）+ instruments + calendar + adjust_factors；写 `fetch_state` 游标 |
| **日内增量** | 每个交易日收盘后（cron） | 增量拉当日 日K/分钟K；同步元数据/复权因子/日历；更新 `fetch_state` |
| **盘中实时** | 交易时段内每 3-5 秒 | 轮询 watchlist 实时快照 → 累积写 snapshots Parquet + upsert `latest_snapshots` 表（供下游 O(1) 取最新价） |

### 5.4 容错与续采
- **限频**：sidecar 调用全局限速（令牌桶），避免触发 akshare/上游封禁。
- **退避重试**：超时/5xx 指数退避 + 抖动；连续失败达阈值标记 `fetch_state.status=failed` 并告警。
- **断点续采**：基于 `fetch_state.last_fetched_ts`，重启后从断点继续，不重不漏。

---

## 6. 接口定义

### 6.1 gRPC 控制面（tg-market-data → 其他服务）
```protobuf
service MarketDataControl {
  rpc TriggerFullSync(FullSyncRequest) returns (SyncJob);     // 指定 watchlist 子集
  rpc TriggerIncrementalSync(Empty) returns (SyncJob);
  rpc GetSyncStatus(Empty) returns (SyncStatusReport);        // 各 symbol/period 进度
  rpc UpdateWatchlist(WatchlistDelta) returns (Watchlist);    // 增删标的
  rpc GetWatchlist(Empty) returns (Watchlist);
}
```
> 注：行情**查询**不经过 market-data，消费者直接链接 `tg-persistence` 库读写。

### 6.2 Python sidecar HTTP API
见 5.2。请求/响应 schema 在 spec 附录用 JSON Schema 固化（实现期补充）。

### 6.3 persistence 库 API
见 4.3 Repository trait。

---

## 7. 错误处理与可观测性
- **结构化日志**（`tracing`）：每次采集记录 symbol/period/耗时/结果/错误。
- **指标**（Prometheus，本期预留接口）：采集成功率、延迟、sidecar 调用 QPS、`fetch_state.failed` 计数。
- **健康检查**：market-data 暴露 `/health`（含 sidecar 连通性 + DB 连通性）。
- 错误分层：sidecar 不可用 → 退避重试 → 标记 failed；数据校验失败 → 隔离该 bar + 记录，不阻断其余。

---

## 8. 测试策略
- **单元测试**（确定性，不依赖网络）：
  - 复权换算正确性（已知因子 → 期望 OHLC）
  - 数据清洗/校验（涨跌停越界、停牌、除权）
  - Parquet 写/读往返 + 分区裁剪
- **集成测试**：用 **mock sidecar**（固定 JSON fixture）驱动 market-data 全流程，验证落库 + 断点续采 + 限频退避。
- **冒烟测试**（手动/CI 可选跳过）：真实 akshare 拉一只标的一小段历史，验证端到端连通（标记 `#[ignore]`，本地按需跑）。
- akshare 是外部不稳定依赖，**所有自动化测试都走 mock sidecar**，保证可复现。

---

## 9. 验收标准（Definition of Done）
1. `watchlist.yaml` 配置若干标的，启动后日K/分钟K/元数据/复权因子完整落库
2. `BarRepo::query_bars` 能按 标的+周期+时间范围+复权方式 返回正确 Bar（前复权值与主流行情软件一致）
3. 盘中交易时段能持续轮询实时快照并累积到 snapshots Parquet
4. 重启后基于 `fetch_state` 增量续采，不重不漏
5. akshare 限频/超时能自动退避重试，连续失败标记 failed 且不崩溃
6. `/health` 正确反映 sidecar + DB 连通性
7. 单元 + 集成（mock sidecar）测试全绿

---

## 10. 依赖的 ADR（来自上游架构文档）
- ADR-011 数据接入：Python sidecar
- ADR-012 标的范围：watchlist
- ADR-013 标的品种：A股股票 + ETF
- ADR-016 历史深度：日K 5年 + 分钟K 取可得
- ADR-017 persistence：共享库 crate
- ADR-018 sidecar 协议：HTTP/FastAPI

---

## 11. 后续 / 延期项
- 数据源多源/备份（akshare → 备选源切换）
- watchlist Web 管理 UI（Phase 4）
- 全市场扫描、Level-2、消息总线（按需）
- 历史数据归档/压缩策略（数据量增长后）

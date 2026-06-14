# Phase 1 分析能力 — 详细设计 Spec

> **子项目**：`tg-indicators`（C++20）+ `tg-factor-engine`（Rust）
> **状态**：设计完成，待用户评审
> **日期**：2026-06-14
> **上游文档**：`2026-06-14-tradeglance-architecture-design.md`（v0.6）
> **契约权威**：`2026-06-14-tg-contracts-design.md`（所有共享类型引用本文，不在本 spec 重复定义）
> **依赖前置**：Phase 0（`tg-contracts` / `tg-market-data` / `tg-persistence`）已就绪
> **相关 ADR**：ADR-005 / 007 / 008 / 016 / 017，**新增 ADR-021 ~ ADR-024**

---

## 1. 概述与范围

### 1.1 目标
在 Phase 0 数据地基之上，提供系统的**分析能力层**：
- `tg-indicators`：把 11 个技术指标（ADX/ATR/布林带/CCI/EMA/MACD/OBV/RSI/SMA/KDJ/威廉%R）以 C++20 实现为独立 gRPC 服务，对齐 `tg-contracts` 的 `Bar` 类型，支撑后续信号引擎与因子引擎。
- `tg-factor-engine`：用 Rust 实现量化因子的**计算 + 库管理 + 有效性评估**全链路——横截面选股排名、时序因子值、IC/IR/衰减/分层回测评估，结果落 `tg-persistence`，并通过 gRPC 暴露查询。

Phase 1 结束即可演示：输入一段历史 K 线 → 算出指标 + 因子值 + 因子评估报告。

### 1.2 In Scope
- `tg-indicators`：C++20 工程骨架、11 个指标的参数化实现、`IndicatorService`（`Compute` / `BatchCompute`）、CMake + proto 生成、性能基线。
- `tg-factor-engine`：`Factor` trait 与声明式注册、7 大因子族（动量/反转/波动率/成交量/资金流/规模/价值）的首批因子、横截面与时序两套计算路径、IC/IR/decay/分层评估算法、因子元数据注册表、因子值落 Parquet + 元数据入 PG、`FactorService`（`ComputeFactor` / `EvaluateFactor` / `QueryFactorValues`）。
- 与 `tg-persistence` 的集成（读 Bar、写因子值/评估）。
- 通过 gRPC 调用 `tg-indicators` 复用指标（如 RSI 因子）。

### 1.3 Out of Scope（YAGNI，留给后续 Phase）
- 生成买卖信号（Phase 2 `tg-signal-engine` 的职责）。
- 策略编排、回测、绩效归因（Phase 2）。
- LLM 决策、下单（Phase 3）。
- 可视化前端（Phase 4）。
- GPU/FPGA 加速、流式增量指标（先做批量一次性计算）。
- 自定义因子 DSL/脚本引擎（本期因子以 Rust trait 静态注册为主）。
- 全市场因子全量重算的分布式调度（watchlist 规模下单机足够）。

---

## 2. 模块形态与依赖

| 模块 | 形态 | 语言 | 角色 |
|---|---|---|---|
| `tg-indicators` | 独立 gRPC 服务（进程） | C++20 + grpc++/protobuf | **指标计算提供方**：纯数值计算，无状态 |
| `tg-factor-engine` | 独立 gRPC 服务（进程） + 共享库 crate | Rust | **因子计算 + 评估 + 库管理**：读 Bar、写因子值、跨进程调用 indicators |

依赖关系：
```
                  tg-contracts (proto + 类型)
                  ▲            ▲
        ┌─────────┘            │
        │ gRPC stub            │ 链接
        │                      │
┌───────────────┐   gRPC   ┌───┴──────────────┐
│ tg-indicators │ ◀────────│ tg-factor-engine │
│   (C++20)     │          │     (Rust)       │
└───────────────┘          └────────┬─────────┘
                                    │ 链接（ADR-017）
                                    ▼
                            tg-persistence (PG + Parquet)
                                    │
                                    ▼
                            tg-market-data 写入的 Bar/快照
```

- `tg-indicators` 只依赖 `tg-contracts` 的 proto（CMake 生成 C++ stub），不读存储、不依赖其他业务模块。**无状态**：每次 `Compute` 都带完整 `bars`。
- `tg-factor-engine` 同时是 `tg-persistence` 的链接消费者（ADR-017，读 Bar、写因子值）和 `tg-indicators` 的 gRPC 客户端（部分因子复用指标，ADR-005 + ADR-022）。
- 两服务都纳入 Phase 4 `tg-infra` 的 docker-compose 编排；本期 spec 给出容器化与配置约定。

---

## 3. tg-indicators 详细设计

### 3.1 定位与改造说明
旧 `quantization-mcp/`（C++ 指标 + MCP 原型）**已被本项目废弃删除**，本 spec 不直接复用其代码。但其算法实现可作为**算法参考**（公式、周期递推关系）。`tg-indicators` **重新实现为干净的 C++20 gRPC 服务**：
- 抛弃 MCP/CLI 入口，统一以 gRPC 暴露。
- 输入/输出严格对齐 `tg-contracts` 的 `IndicatorRequest` / `IndicatorResult`。
- 仅保留数值计算核心，移除任何数据源/网络采集逻辑（旧原型里 `network_data_source` 等概念一并丢弃）。

### 3.2 工程骨架（CMake）
```
cpp/tg-indicators/
├── CMakeLists.txt
├── proto/                       # 软链/复制自 tg-contracts/proto/tg（或 CMake 直接引用）
└── src/
    ├── main.cpp                 # gRPC server 启动
    ├── service.cpp/.h           # IndicatorService 实现
    ├── bar_codec.h              # proto Bar → 内部 OHLCV 视图（零拷贝）
    └── indicators/
        ├── indicator_base.h     # IIndicator 接口
        ├── sma.cpp  ema.cpp  rsi.cpp  macd.cpp
        ├── adx.cpp  atr.cpp  bollinger.cpp
        ├── cci.cpp  obv.cpp  kdj.cpp  williams_r.cpp
        └── registry.cpp         # 工厂注册：indicator 名 → 构造函数
```
- 构建：CMake ≥ 3.20，编译器要求 C++20（gcc 12+ / clang 15+）。
- 依赖：`grpc++`、`protobuf`、`abseil`（grpc 传递依赖）。无第三方数值库——指标用纯标准库实现，便于审计与跨平台。
- proto 生成：CMake 调 `protobuf::protoc` + `grpc_cpp_plugin` 产出 stub；proto 源文件**单点维护于 `tg-contracts/proto/tg/`**，CMake 以相对路径引用，不在本仓库复制副本（避免接口漂移，遵循契约先行）。

### 3.3 指标接口（C++ 内部）
```cpp
// 内部接口，与服务层解耦；便于单元测试直接对拍
struct OHLCV {
    int64_t ts_millis;            // 与 proto Bar.ts 对齐
    double open, high, low, close;
    int64_t volume;
    double amount;
};

class IIndicator {
public:
    virtual ~IIndicator() = default;
    // 用 params 配置（如 period），返回命名子序列
    virtual void configure(const std::unordered_map<std::string,double>& params) = 0;
    virtual std::unordered_map<std::string, std::vector<double>>
        compute(gsl::span<const OHLCV> bars) = 0;
    // 校验：参数范围、最小输入长度（如 period=14 需 ≥15 根）
    virtual void validate(gsl::span<const OHLCV> bars) const /* throws */ = 0;
};
```
- `registry`：启动时静态注册 11 个指标；服务层按 `IndicatorRequest.indicator` 字符串查表派发。
- 配置默认值：每指标提供默认参数（如 RSI period=14、MACD fast=12/slow=26/signal=9、布林 period=20/std=2.0），`params` 未提供时用默认。

### 3.4 各指标参数与输出 series 命名

> 输出 `IndicatorResult.series` 的 key 严格按下表命名；多输出指标必须返回全部子序列且长度对齐 `ts`（缺失前导用 `NaN` 填充，与契约 `f64` 兼容）。`ts` 直接取输入 `bars` 的 `ts` 序列。

| 指标 (`indicator` key) | 参数（默认） | 输出 series key | 备注 |
|---|---|---|---|
| `SMA` | `period=20` | `sma` | 单序列；前 `period-1` 个为 NaN |
| `EMA` | `period=12`, `smoothing=2.0` | `ema` | 单序列；首值用 SMA 种子 |
| `RSI` | `period=14` | `rsi` | Wilder 平滑；值域 [0,100] |
| `MACD` | `fast=12`, `slow=26`, `signal=9` | `dif`, `dea`, `hist` | `hist = (dif-dea)*2`（A股惯例） |
| `ADX` | `period=14` | `adx`, `plus_di`, `minus_di` | +DI/-DI 伴随输出，便于信号引擎 |
| `ATR` | `period=14` | `atr` | Wilder 递推；TR 用 high/low/close |
| `BOLL` | `period=20`, `std_dev=2.0` | `upper`, `mid`, `lower` | mid=SMA；upper/lower=mid±k·σ |
| `CCI` | `period=20`, `constant=0.015` | `cci` | 典型价 (H+L+C)/3 |
| `OBV` | —（无参数） | `obv` | 累积量能；起始 0 |
| `KDJ` | `k_period=9`, `d_period=3`, `j_smooth=3` | `k`, `d`, `j` | `J = 3K - 2D`，A股 9,3,3 惯例 |
| `WILLR` | `period=14` | `willr` | Williams %R；值域 [-100, 0] |

**通用约定**：
- 所有序列长度 = 输入 `bars.len()`；前导不充分位置用 `NaN`（C++ `std::numeric_limits<double>::quiet_NaN()`），便于下游对齐。
- 价格输入用 `Bar.close` 为主（RSI/EMA/MACD/CCI 典型价等按上表规则），volume/amount 用于 OBV。
- 输入不足（`bars.len() < period`）→ 服务层返回 `INVALID_ARGUMENT`（见 §6），不返回部分序列。

### 3.5 服务层与协议
```cpp
// service.cpp 伪码
class IndicatorServiceImpl final : public IndicatorService::Service {
  grpc::Status Compute(grpc::ServerContext*,
                       const IndicatorRequest* req,
                       IndicatorResult* out) override {
    auto bars = decode_bars(req->bars());          // proto Bar → OHLCV
    auto it = registry_.find(req->indicator());
    if (it == registry_.end()) return status(NOT_FOUND, "unknown indicator");
    auto ind = it->second();
    ind->configure(req->params());
    ind->validate(bars);                            // 失败 → INVALID_ARGUMENT
    auto series = ind->compute(bars);
    *out = encode_result(req->indicator(), bars, series);
    return grpc::Status::OK;
  }
  grpc::Status BatchCompute(ServerReaderWriter<IndicatorResult, IndicatorRequest>* stream) override {
    IndicatorRequest req;
    while (stream->Read(&req)) {
      IndicatorResult out;
      auto s = Compute(ctx, &req, &out);
      if (!s.ok()) { stream->Write(make_error_result(req, s)); continue; }
      stream->Write(out);
    }
    return Status::OK;
  }
};
```
- `BatchCompute`：双向流，**保序回写**（请求 i 的响应对应回写第 i 条）；单条失败不中断流，错误以状态码 + 空结果 series 表达。
- 无状态：服务可在多副本间负载均衡（Phase 4 编排）。

### 3.6 性能与并发
- 单次 `Compute` 在 watchlist 规模（单标的日 K 5 年 ~1200 根 + 分钟 K）下目标 < 5 ms（不含 gRPC 序列化）。
- 服务内部用线程池（grpc 默认 sync server，每 RPC 一线程）；纯计算无锁，无须共享状态。
- 内存：避免每请求 malloc；`compute` 内部 reserve 预留容量；输入 OHLCV 用 `gsl::span` 零拷贝视图。
- 数值用 `double`（指标本身是统计量，f64 精度足够；价格精度由 `tg-contracts` 在 Bar 层用 `Decimal` 保证，进入指标层前转 `double`）。

---

## 4. tg-factor-engine 详细设计

### 4.1 Factor trait 与声明式注册
```rust
/// 输入：单标的历史 Bar（时序因子）+ 横截面快照（横截面因子所需的全 universe 同期数据）。
/// 输出：FactorValue（引用 tg-contracts，禁止在此重新定义）。
#[async_trait]
pub trait Factor: Send + Sync {
    /// 因子元信息（注册表用）
    fn meta(&self) -> &FactorMeta;

    /// 时序计算：单标的历史因子值序列。
    /// history 为该标的过去 N 根 Bar；返回与输入等长的因子值（前导不足用 NaN）。
    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>>;

    /// 横截面计算：某时刻全 universe 因子值（排名选股用）。
    /// 默认实现：对每个 symbol 取 history 末根算因子值。可被子因子覆写以做截面标准化。
    async fn compute_cross_section(
        &self,
        universe: &[(String, &[Bar])],   // (symbol, 该标的的历史窗口)
    ) -> Result<Vec<(String, f64)>>;     // (symbol, 原始值；rank 在引擎层统一算)
}
```
- **声明式注册**：用 `inventory` crate（或自建 `register_factor!` 宏）启动时收集所有 `Factor` 实例 → `FactorRegistry`。新增因子只需实现 trait + `register_factor!(Box::new(Momentum20d))`，不改引擎。
- 因子值类型：原始值 `value: f64`；横截面排名 `rank: Option<u32>` 在引擎层统一计算后填入 `FactorValue`（契约 §2.4）。

### 4.2 因子元数据注册表（FactorMeta / 落 PG）
```rust
pub struct FactorMeta {
    pub name: String,            // 唯一键，如 "momentum_20d"
    pub category: FactorCategory,// 动量/反转/波动率/成交量/资金流/规模/价值
    pub logic: String,           // 人类可读公式与意图
    pub data_dep: Vec<DataDep>,  // 数据依赖：如 [Bar(Daily, 20), Snapshot(bid_ask_imbalance)]
    pub params: HashMap<String, f64>,
    pub direction: FactorDirection, // 正向（值大→看多）/反向
    pub enabled: bool,
}
pub enum FactorCategory { Momentum, Reversal, Volatility, Volume, MoneyFlow, Size, Value }
pub enum DataDep { BarDaily(usize), BarMinute(BarPeriod, usize), SnapshotQuote }
pub enum FactorDirection { Positive, Negative }
```
- 注册表持久化：`FactorMeta` 写入 PostgreSQL `factor_meta` 表（见 §4.6）；运行时从注册表 + DB 双向校对（启动期不一致则告警）。
- `QueryFactorValues` 暴露元数据列表（按 category 过滤），供可视化与信号引擎使用。

### 4.3 因子族清单（首批）与公式概要

> 公式为概要，实现细节以代码 + 单元对拍为准。所有因子输出 `f64`；除特别说明，输入用前复权 `Bar`（查询层在 `tg-persistence` 完成 `Adjustment::PreAdjust` 换算）。

| 因子名 | 族 | 公式概要 | 数据依赖 | 备注 |
|---|---|---|---|---|
| `momentum_20d` | 动量 | `close[t]/close[t-20] - 1` | Daily 21 | 反映近月涨跌幅 |
| `momentum_60d` | 动量 | `close[t]/close[t-60] - 1` | Daily 61 | 中期动量 |
| `reversal_5d` | 反转 | `-(close[t]/close[t-5] - 1)` | Daily 6 | 短线反转（反向） |
| `volatility_20d` | 波动率 | `std(ret, 20)`，ret=日收益 | Daily 21 | 高波动→风险溢价 |
| `volume_ratio_5d` | 成交量 | `mean(vol,5)/mean(vol,20)` | Daily 20 | 量能放大 |
| `turnover_5d` | 成交量 | `mean(amount,5)/circ_mv` | Daily 5 + 流通市值 | 换手代理 |
| `amount_momentum_5d` | 资金流 | `sum(amount,5)/sum(amount,20)` | Daily 20 | 资金流入强度 |
| `bid_ask_imbalance` | 资金流 | `(Σbid_vol - Σask_vol)/(Σbid_vol + Σask_vol)` | Snapshot 五档 | 需实时快照 |
| `log_market_cap` | 规模 | `ln(总市值)` | Bar close × 总股本 | 小盘效应 |
| `circ_mv` | 规模 | `close × 流通股本` | Bar close × 流通股本 | 流通市值 |
| `pe_ttm` | 价值 | `close × 总股本 / 归母净利润TTM` | 财务快照 | 估值因子，依赖外部财务数据 |
| `rsi_factor_14` | （复用指标） | `50 - RSI(14)`（反向：超买看空） | Daily 15 | 经 gRPC 调 `tg-indicators` |

> `pe_ttm` 与 `circ_mv`/`log_market_cap` 需要标的股本/财务数据。Phase 1 范围内若 `tg-market-data` 尚未提供财务快照，则这两类价值/规模因子标记为 **`enabled=false`（注册但不计算）**，待数据源补齐后启用——不阻塞 Phase 1 主体。

### 4.4 横截面 vs 时序（两条计算路径）

| 路径 | 触发 | 输入 | 输出 | 用途 |
|---|---|---|---|---|
| **横截面（cross-section）** | 每个交易日收盘后（cron） | 某 `trading_date` 全 universe 各标的历史窗口 | `Vec<FactorValue{symbol, value, rank}>` | 选股排名；信号引擎"动量前10%" |
| **时序（timeseries）** | 信号触发 / 单标的查询 | 单标的历史 N 根 Bar | `Vec<f64>` 等长 | 信号引擎"该标的 RSI 因子是否 <30" |

- **横截面标准化（ADR-023）**：原始 `value` 分布差异大（如市值 vs 动量），选股排名前须统一可比。本期采用 **rank-based 标准化**：将原始值在全 universe 内排名后映射到 [0,1]（`rank/(N-1)`），稳健抗极值；`FactorValue.value` 存原始值，`rank` 存排名（0-based）。z-score 标准化作为未来备选，不在本期实现。
- **rank 方向**：依据 `FactorMeta.direction`，正向因子（值大看多）rank 越大约靠前；反向因子先取负再排名。
- 横截面计算以 `trading_date` 为键，按交易日批量执行；同一日内全 universe 并发（tokio 任务池，限并发数）。

### 4.5 因子评估算法（→ FactorEvaluation，引用契约 §2.4）

> 评估是对**历史因子值时序**与**未来收益**的统计分析。所有统计量用 `f64`。

1. **数据准备**
   - 因子值时序：对某因子取过去 T 个交易日、全 universe 的横截面值（来自 §4.4 横截面落库结果）。
   - 未来收益：每个标的在未来 N 日（默认 `horizons=[1,5,10,20]`）的收益率，从 `tg-persistence` 的前复权 Bar 计算。

2. **IC（信息系数）**
   - 每个交易日：横截面因子值与未来收益的 **Spearman 秩相关**（rank IC，稳健抗极值，ADR-021 选定）。
   - `ic_mean` = T 日 rank IC 序列均值；`ic_std` = 该序列标准差。

3. **IR（信息比率）**
   - `ir = ic_mean / ic_std`（与契约 `FactorEvaluation.ir` 一致）。衡量因子稳定性。

4. **因子衰减（decay）**
   - 对 `horizons` 各滞后日分别算 rank IC，得 `decay: Vec<f64>`（长度 = `horizons.len()`）。用于判断因子有效期（短线系统尤其关心 1~5 日衰减）。

5. **分层回测（quantile_returns）**
   - 每个交易日按因子值将 universe 分为 `Q=5` 个分位组（Q1 最小 ~ Q5 最大，方向按 `FactorMeta.direction`）。
   - 每组等权计算未来 N 日收益，T 日取均值 → `quantile_returns: Vec<f64>`（长度 = Q）。
   - 关注 Q5-Q1 多空收益（首尾组差），写入 `FactorEvaluation` 旁注字段（评估报告扩展，不入契约核心结构）。

> 所有评估结果封装为契约 `FactorEvaluation` 并落库（元数据/汇总入 PG，明细 Parquet）。

### 4.6 持久化（落 tg-persistence）

**Parquet 布局（因子值时序，DuckDB 可查）**——沿用 Phase 0 分区风格：
```
data/
  factors/
    factor=<NAME>/date=<YYYYMMDD>/part.parquet
```
- 列：`symbol, ts, trading_date, value(f64), rank(u32 nullable)`。
- 分区键：`factor + date`；watchlist 规模下单分片极小，DuckDB 谓词下推秒查。
- 写入策略：`tg-factor-engine` 为该目录的**唯一写入者**（与 market-data 写 bars 的契约一致，ADR-017）；先写临时文件再原子 rename。

**PostgreSQL（因子元数据 + 评估汇总）**：
```sql
CREATE TABLE factor_meta (
    name        TEXT PRIMARY KEY,
    category    TEXT NOT NULL,
    logic       TEXT NOT NULL,
    data_dep    JSONB NOT NULL,
    params      JSONB NOT NULL,
    direction   TEXT NOT NULL,        -- positive/negative
    enabled     BOOLEAN NOT NULL DEFAULT true,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE factor_evaluation (
    factor          TEXT NOT NULL REFERENCES factor_meta(name),
    as_of_date      DATE NOT NULL,    -- 评估基准日
    window_days     INT NOT NULL,
    ic_mean         DOUBLE PRECISION,
    ic_std          DOUBLE PRECISION,
    ir              DOUBLE PRECISION,
    decay           DOUBLE PRECISION[],  -- 各 horizon 的 IC
    quantile_returns DOUBLE PRECISION[],
    computed_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (factor, as_of_date, window_days)
);
```
- 迁移脚本随 `tg-persistence` 统一管理（`sqlx migrate`）。
- `FactorRepo` trait 在 `tg-persistence` 扩展（Phase 0 已预留接口）：
  ```rust
  trait FactorRepo {
      async fn upsert_factor_values(&self, rows: &[FactorValue]) -> Result<()>;
      async fn query_factor_values(&self, factor: &str, date: NaiveDate) -> Result<Vec<FactorValue>>;
      async fn upsert_evaluation(&self, e: &FactorEvaluation) -> Result<()>;
      async fn upsert_meta(&self, m: &FactorMeta) -> Result<()>;
  }
  ```

### 4.7 指标复用边界（ADR-022）
- **边界原则**：因子优先用纯 Rust 实现（避免跨进程 gRPC 往返）；仅当因子语义**等价于某个指标**且复用能减少分歧实现时，才经 gRPC 调 `tg-indicators`。
- **首批复用**：仅 `rsi_factor_14` 复用 RSI 指标。其他因子（动量/反转/波动率/量能/市值/估值）公式简单，Rust 直接算，不引入网络依赖。
- **客户端**：`tg-factor-engine` 内置 tonic `IndicatorService` client；评估/计算路径若涉及指标复用，统一走 `Compute`（单次），不滥用 `BatchCompute`。
- **失败降级（ADR-022）**：若 `tg-indicators` 不可达，`rsi_factor_14` 标记 `NaN` 并告警，不阻断其余因子计算；不可达阈值（连续失败次数）走配置。短线决策不应因单个指标服务抖动而整体停摆。

---

## 5. 接口定义（proto 完整签名）

> 消息体引用契约：`IndicatorRequest`/`IndicatorResult`/`FactorValue`/`FactorEvaluation`/`Bar` 均在 `tg-contracts/proto/tg/*.proto` 定义，本 spec 不重复。下列为 service RPC 签名。

### 5.1 tg-indicators — `IndicatorService`
```protobuf
service IndicatorService {
  // 单次计算：indicator 名 + params + bars → 命名 series
  rpc Compute(tg.v1.IndicatorRequest) returns (tg.v1.IndicatorResult);

  // 批量流式：保序回写，单条失败以 status 表达
  rpc BatchCompute(stream tg.v1.IndicatorRequest)
      returns (stream tg.v1.IndicatorResult);
}
```
- `IndicatorRequest.indicator` 取值见 §3.4 表（`SMA`/`EMA`/`RSI`/`MACD`/`ADX`/`ATR`/`BOLL`/`CCI`/`OBV`/`KDJ`/`WILLR`）。
- 未知 indicator → `NOT_FOUND`；参数非法/数据不足 → `INVALID_ARGUMENT`。

### 5.2 tg-factor-engine — `FactorService`
```protobuf
service FactorService {
  // 时序计算：单标的某因子历史值（供信号引擎实时取用）
  rpc ComputeFactor(ComputeFactorRequest) returns (ComputeFactorResponse);

  // 触发某因子的评估（异步任务），返回任务 id
  rpc EvaluateFactor(EvaluateFactorRequest) returns (EvaluateFactorJob);

  // 查询因子值（横截面：某日全 universe；或时序：某标的一段）
  rpc QueryFactorValues(FactorQuery) returns (stream tg.v1.FactorValue);

  // 查询因子元数据列表（注册表）
  rpc ListFactors(ListFactorsRequest) returns (ListFactorsResponse);

  // 触发横截面批量计算（cron 入口，通常由 tg-infra 调度）
  rpc RunCrossSection(CrossSectionRequest) returns (CrossSectionJob);
}

message ComputeFactorRequest {
  string factor = 1;
  string symbol = 2;
  tg.v1.Exchange exchange = 3;
  tg.v1.BarPeriod period = 4;
  // 时间范围（落库已有则直接读，缺失则即时算）
  google.protobuf.Timestamp start_ts = 5;
  google.protobuf.Timestamp end_ts = 6;
  map<string, double> params_override = 7;
}
message ComputeFactorResponse {
  repeated google.protobuf.Timestamp ts = 1;
  repeated double values = 2;     // 与 ts 等长，前导不足 NaN
}

message EvaluateFactorRequest {
  string factor = 1;
  google.protobuf.Timestamp as_of_ts = 2;   // 评估基准日（取其日期）
  int32 window_days = 3;                     // 回看窗口
  repeated int32 horizons = 4;               // [1,5,10,20]
  int32 quantiles = 5;                       // 默认 5
}
message EvaluateFactorJob {
  string job_id = 1;                         // ULID
  string status = 2;                         // queued/running/done/failed
}

message FactorQuery {
  oneof scope {
    string cross_section_date = 1;           // "YYYY-MM-DD"：返回该日全 universe
    CrossSectionByTs cross_section_ts = 2;
  }
  string factor = 3;
  // 可选 symbol 过滤（横截面下取子集；时序下指定标的）
  string symbol = 4;
}
message CrossSectionByTs { google.protobuf.Timestamp ts = 1; }

message ListFactorsRequest {
  string category = 1;                       // 可选过滤
  bool enabled_only = 2;
}
message ListFactorsResponse { repeated tg.v1.FactorMeta factors = 1; }

message CrossSectionRequest {
  string factor = 1;
  google.protobuf.Timestamp as_of_ts = 2;
}
message CrossSectionJob { string job_id = 1; string status = 2; }
```
> `FactorMeta` 作为 proto message 在 `tg-contracts` 补充定义（字段对应 §4.2 Rust 结构），不在本 spec 重复。

---

## 6. 错误处理与可观测性

### 6.1 错误模型
- Rust 侧用契约 `TgError` + 具名 `thiserror` 子错误（`FactorError`：`InsufficientData`/`UnknownFactor`/`IndicatorUpstream`/`Storage`）；binary 边界 `anyhow`；gRPC 边界映射 tonic `Status`：
  - `UnknownFactor` → `NOT_FOUND`
  - `InsufficientData`/参数非法 → `INVALID_ARGUMENT`
  - `IndicatorUpstream` 不可达 → `UNAVAILABLE`
  - `Storage` → `INTERNAL`
- C++ 侧用 grpc `Status(code, msg)` 直接返回；不抛异常出服务边界。

### 6.2 可观测性
- **结构化日志（`tracing` / C++ spdlog 或 glog）**：每次 `Compute`/`ComputeFactor`/`EvaluateFactor` 记录 indicator/factor/symbol/数据长度/耗时/结果状态，带 `request_id`。
- **Prometheus 指标**：
  - `tg_indicators_compute_duration_seconds`（histogram，按 indicator label）
  - `tg_indicators_compute_total{indicator,status}`
  - `tg_factor_compute_duration_seconds{factor}`
  - `tg_factor_evaluation_ir`（gauge，最近一次评估，按 factor label，供监控趋势）
  - `tg_indicator_upstream_errors_total`（`tg-factor-engine` 侧，调 indicators 失败计数）
- **健康检查**：两服务 `/health`（gRPC `Health` service + HTTP 端口）；`tg-factor-engine` 健康检查含 DB 连通性 + indicators client 探活。
- **链路追踪**（预留）：tonic/grpc interceptors 注入 trace id（OpenTelemetry），Phase 4 接入。

---

## 7. 测试策略

### 7.1 单元测试（确定性，不依赖网络/外部服务）
**tg-indicators（C++，GoogleTest）**：
- 每个指标用**已知数据集对拍**：手工构造的小型 OHLCV fixture（含 5/20/60 根典型案例），与 Python `pandas`/`talib` 计算结果比对（容差 1e-6）。
- 边界：输入不足（`bars < period`）返回错误；前导 NaN 数量正确；多输出指标（MACD/KDJ/BOLL/ADX）各子序列长度与 key 齐全。
- 参数边界：非正 period、极端 std_dev 等 → `INVALID_ARGUMENT`。

**tg-factor-engine（Rust，`#[test]`）**：
- 单因子时序计算对拍：构造历史 Bar，验证 `momentum_20d`/`reversal_5d`/`volatility_20d` 等与手算/pandas 结果一致。
- 横截面 rank 标准化正确性（已知输入 → 期望 rank，含反向因子方向）。
- 评估算法对拍：固定 fixture（因子值 + 未来收益）→ 期望 IC/IR/decay/quantile_returns（与 pandas/scipy 的 spearmanr 比对）。
- 元数据注册表：启动后 `enabled` 因子集合符合预期。

### 7.2 集成测试（mock 外部依赖）
- **mock indicators gRPC**：`tg-factor-engine` 测试用 in-process tonic server 桩实现 `IndicatorService`，返回固定 RSI 序列，验证 `rsi_factor_14` 经 gRPC 复用路径正确，并验证 indicators 不可达时降级为 NaN + 告警（ADR-022）。
- **mock persistence**：用临时 PG（`sqlx::test` 自动建临时库）+ 临时 Parquet 目录，验证因子值落库 + 查询往返 + 分区布局正确。
- 横截面批量计算端到端：mock 全 universe 历史 Bar → `RunCrossSection` → 验证 `factor=<NAME>/date=<YYYYMMDD>/part.parquet` 生成且 rank 正确。

### 7.3 冒烟测试（`#[ignore]`，手动/CI 可选）
- 真实拉 `tg-market-data` 已落库的 watchlist 行情 → 真实调 `tg-indicators` → 跑一次 `EvaluateFactor`，人工核对 IC/IR 量级合理（标记 `#[ignore]`，需真实数据，本地按需跑）。

---

## 8. 验收标准（Definition of Done）

1. `tg-indicators` 容器化运行，11 个指标均可经 `IndicatorService.Compute` 计算且 series key 与 §3.4 表完全一致；`BatchCompute` 保序回写正确。
2. 11 个指标的单元对拍测试全绿（与 pandas/talib 容差内一致），边界与多输出齐全性测试通过。
3. `tg-factor-engine` 容器化运行，`FactorService` 三个核心 RPC（`ComputeFactor`/`EvaluateFactor`/`QueryFactorValues`）+ `ListFactors`/`RunCrossSection` 可用。
4. 首批 enabled 因子（动量/反转/波动率/成交量/资金流的量价类，至少 6 个）时序与横截面计算结果与手算对拍一致；`rsi_factor_14` 经 gRPC 复用 RSI 指标结果正确。
5. 因子值按 §4.6 Parquet 布局正确落盘，DuckDB 可按 `factor + date` 查询；`factor_meta`/`factor_evaluation` PG 表正确写入。
6. 评估算法 `EvaluateFactor` 对固定 fixture 产出 IC/IR/decay/quantile_returns 且与 scipy 对拍一致。
7. `tg-indicators` 不可达时 `tg-factor-engine` 不崩溃，`rsi_factor_14` 降级 NaN 并发指标告警（ADR-022 验证）。
8. 横截面 rank-based 标准化（ADR-023）正确处理反向因子方向。
9. `/health` 正确反映各服务依赖（DB / indicators client）连通性。
10. 单元 + 集成测试全绿；冒烟测试可手动复现。

---

## 9. 依赖的 ADR

### 已有 ADR（来自上游架构文档）
- **ADR-005** C++ 指标接入：A1 独立 gRPC 服务（非 FFI）——`tg-indicators` 的形态基础。
- **ADR-007** 存储：PostgreSQL + Parquet/DuckDB——因子值列存 + 元数据/评估入 PG。
- **ADR-008** 语言分工：C++20 仅 indicators，其余 Rust。
- **ADR-016** 历史深度：日 K 5 年 + 分钟 K 取可得——决定因子回看窗口上限。
- **ADR-017** persistence 共享库 crate——`tg-factor-engine` 链接而非 RPC 访问存储。

### 新增 ADR（本 spec 提出）
- **ADR-021 因子评估的 IC 选型**：采用 **Spearman rank IC**（秩相关）而非 Pearson IC。
  - 理由：A股因子值常含极值/厚尾，rank IC 稳健抗异常值，且与横截面 rank 选股逻辑一致；短线系统对极端值的容忍度低。`FactorEvaluation.ic_mean/ic_std/ir` 全部基于 rank IC 序列。
- **ADR-022 因子复用 indicators 的边界**：因子优先纯 Rust 实现；仅当语义等价于某指标时经 gRPC 复用（首批仅 `rsi_factor_14`）。indicators 不可达时因子降级 NaN + 告警，不阻断其余因子计算。
  - 理由：避免因子库普遍引入跨进程网络依赖（延迟 + 可用性耦合）；同时避免对同一指标（如 RSI）在两套语言各实现一次导致分歧。
- **ADR-023 横截面标准化方法**：因子横截面统一采用 **rank-based 标准化**（`rank/(N-1)` 映射 [0,1]，方向按 `FactorMeta.direction`），存 `FactorValue.rank`；z-score 等方法作为未来备选。
  - 理由：rank 标准化对极值稳健、分布无关，与选股排名语义天然对齐；watchlist 规模（几十只）下计算开销可忽略。
- **ADR-024 因子值存储分区粒度**：因子 Parquet 按 `factor=<NAME>/date=<YYYYMMDD>/` 分区（而非 symbol 分区）。
  - 理由：横截面选股的查询模式是"某日全 universe 某因子"，按 factor+date 分区使典型查询单分片命中，DuckDB 谓词下推最优；与行情按 symbol 分区（Phase 0）互补——行情主查时序、因子主查截面。

---

## 10. 后续 / 延期项
- 价值/规模因子（`pe_ttm`/`circ_mv`/`log_market_cap`）依赖标的股本与财务数据，待 `tg-market-data` 补齐财务快照后启用。
- 因子评估的滚动窗口增量计算（当前为整窗重算，watchlist 规模下可接受）。
- z-score / 行业中性化 / 市值中性化等高级标准化方法（ADR-023 备选）。
- 自定义因子 DSL 或脚本化注册（避免每次新因子都改 Rust 代码）。
- 指标层 FFI 直链（ADR-005 备选 A2）：若未来 gRPC 序列化成为瓶颈，评估 Rust↔C++ FFI 直链。
- 指标/因子的流式增量计算（当前为批量一次性）。
- 跨语言契约测试（Rust ↔ C++ buf schema registry，与 `tg-contracts` §10 一致）。

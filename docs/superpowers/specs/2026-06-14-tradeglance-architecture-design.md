# TradeGlance — A 股短线交易系统 架构设计方案

> 📌 **本文件是项目的活文档（living doc）**，作为单一事实来源，随讨论实时维护更新。
> 当前状态：📐 **全模块详细设计完成** — 12 模块架构 + tg-contracts + Phase 0-4 详细 spec 全部成文，待启动 Phase 0 编码实现（codex 全自动 + 事后审阅）
> 最后更新：2026-06-14

---

## 0. 修订记录

| 日期 | 版本 | 变更 | 说明 |
|---|---|---|---|
| 2026-06-14 | v0.1 | 初稿 | 锁定宏观约束、确定方案 A、拆解 12 个模块 |
| 2026-06-14 | v0.2 | 重命名 | `tg-paper-execution` → `tg-mock-order-engine`；显式记录 `ExecutionHandler` 为 mock/实盘切换点 |
| 2026-06-14 | v0.3 | 决策 | Q2 定位：decision-agent 为**最终决策者**（信号仅作输入）；signal-engine 改为候选产生者；新增 ADR-010 + 核心数据流；衍生 Q7（入场/出场路径）、Q8（LLM 兜底） |
| 2026-06-14 | v0.4 | 范围细化 | 锁定 Q1/Q3/Q4/Q5/Q6：watchlist + A股&ETF + 三策略风格(波段/T0/打板) + Python sidecar 取数 + 多 provider LLM；新增 ADR-011~015、§2.1 范围细化、各模块描述补充 |
| 2026-06-14 | v0.5 | Phase 0 范围 | 历史深度：日K 5年 + 分钟K 取 akshare 可得；新增 ADR-016；§8 数据范围锁定，进入详细设计 |
| 2026-06-14 | v0.6 | Phase 0 设计决策 | tg-persistence 定为共享库 crate（ADR-017）；Python sidecar 用 HTTP/FastAPI（ADR-018）；Phase 0 详细设计成文 |
| 2026-06-14 | v0.7 | 全模块详细设计 | tg-contracts 权威 spec 成文；Phase 1-4 详细设计 spec 全部成文；新增 ADR-019~037；Q7/Q8 决策落地（ADR-019/020）；跨模块类型对齐（OrderIntent 入 contracts，ExecutionHandler 统一签名） |

---

## 1. 项目概述

**TradeGlance** 是一套面向 A 股的**短线交易系统**，覆盖从市场信号捕捉、因子分析、技术分析、agent 决策到模拟交易的完整链路，并配套后台数据可视化与统计。

按功能拆分为**多个独立模块仓库**，每个模块可独立部署、独立演进，通过 gRPC 契约协作。

### 目标
- 信号 → 分析 → 决策 → 模拟下单的全链路打通
- 回测与模拟共用同一套引擎（避免回测/实盘行为不一致）
- 容器化部署，模块边界清晰，可平滑升级到实盘

---

## 2. 已锁定的约束

| 维度 | 选择 | 架构含义 |
|---|---|---|
| **交易模式** | 纸面模拟（mock order） | 内置撮合引擎，无券商接口；未来可平滑升级实盘 |
| **行情数据** | 免费秒级快照 | 轮询拉取（akshare/tushare 系），不需要低延迟流式推送 |
| **部署** | 个人但容器化 | 服务化边界 + docker-compose，单用户 |
| **语言** | C++20 / Rust | 除指标外全 Rust；详见 [§5 技术栈](#5-技术栈与语言分工) |

### 2.1 交易范围细化（v0.4 锁定）

- **标的范围（ADR-012）**：watchlist（自选股池，几十只），**非全市场扫描**。数据量小，Parquet/DuckDB 轻松承载。
- **标的品种（ADR-013）**：A 股股票 + ETF；**不含指数、可转债**。ETF 规则有差异（免印花税、部分 ETF 支持 T+0）。
- **策略风格（ADR-014，三套）**——signal-engine 需分别实现信号规则：
  - **波段**：持仓 1-5 天，日/分钟 K 驱动
  - **T0 做T**：日内对已有持仓做 T+0（卖出昨日持仓 + 当日回买），分钟/实时驱动；需 mock-order-engine 区分"昨日持仓"与"今日买入"以执行 T+1
  - **打板**：盘中检测涨停封板、尾盘买入、次日卖出，需实时秒级快照
- **数据接入（ADR-011）**：Python sidecar 包装 akshare 等免费源，Rust market-data 经 gRPC/HTTP 调用。
- **LLM 接入（ADR-015）**：多 provider 抽象，OpenAI 兼容协议为主，可配 `base_url + api_key + model`。

---

## 3. 宏观架构（方案 A）

**选定方案：gRPC 同步 + 进程隔离**

```
┌─────────────┐    gRPC    ┌──────────────┐
│ market-data │ ─────────▶ │  indicators   │ (C++20, 复用现有资产)
│   (Rust)    │            │   service     │
└──────┬──────┘            └──────────────┘
       │ 行情写入
       ▼
┌─────────────┐    gRPC    ┌──────────────┐    gRPC    ┌─────────────────────┐
│ persistence │ ◀────────▶ │ signal-engine│ ◀────────▶ │  mock-order-engine  │
│   (Rust)    │            │ factor-engine│            │  (撮合/持仓/虚拟账户) │
└─────────────┘            │   (Rust)     │            │       (Rust)        │
       ▲                    └──────┬───────┘            └──────────┬──────────┘
       │                           │ LLM API                       │ 订单/持仓
       │                    ┌──────▼───────┐                       │
       │                    │decision-agent│                       │
       │                    │   (Rust)     │                       │
       │                    └──────────────┘                       │
       └───────────────────────────────────────────────────────────┘
                              ┌──────────────┐
                              │monitoring-viz│ (Rust/Axum + TS 前端)
                              └──────────────┘

存储: PostgreSQL(元数据/订单/持仓) + Parquet/DuckDB(行情/因子列存)
编排: docker-compose
```

### 3.1 设计原则

1. **一套引擎、两种运行模式**：`tg-engine` 是事件驱动核心，回测时回放历史事件、模拟时消费实时事件——同一套策略代码两种模式。这是避免"回测能赚、实盘亏损"的标准做法。
2. **`ExecutionHandler` 是 mock / 实盘的切换点**：`tg-engine` 定义该 trait，`tg-mock-order-engine` 和未来的 `tg-broker-gateway` 都实现它。从纸面升级实盘只需切换注入对象，业务逻辑不动。
3. **契约先行**：所有跨服务类型与 RPC 接口集中在 `tg-contracts`，防止接口漂移。
4. **演进路径清晰**：架构上把"行情→信号→订单"建模成事件流（即便先走 gRPC 同步），未来换 tick 行情时只换传输层（gRPC → NATS），业务逻辑不动。
5. **YAGNI**：秒级快照 + 纸面模拟阶段，不引入消息总线（NATS/Kafka）、不引入 ClickHouse——按需演进。
6. **决策权归 agent**：核心数据流为 `行情 → 指标/因子 → signal-engine(产生候选) → decision-agent(拍板) → mock-order-engine(执行)`；agent 是唯一拍板者（ADR-010）。

### 3.2 C++ 指标接入方式：A1（独立 gRPC 服务）

- `tg-indicators` 编译为**独立 gRPC 服务**，Rust 调用它。
- 秒级频率下 gRPC 序列化开销可忽略；边界最干净、可独立部署扩展。
- FFI 直链（A2）作为未来性能瓶颈时的优化备选，当前不采用。

---

## 4. 模块划分（12 个模块）

### 4.0 依赖关系总览

```
Phase 0 ─ 地基（一切的前提）
  tg-contracts ────────┐ (proto + 共享类型，所有人依赖)
                        │
  tg-persistence ◀──────┤ (Postgres + Parquet/DuckDB 存储服务 + schema)
                        │
  tg-market-data ◀──────┘ (akshare 采集：历史日/分钟 + 实时秒级快照)

Phase 1 ─ 分析能力
  tg-indicators (C++20) ◀── tg-contracts     (现有资产，改造为 gRPC 服务)
  tg-factor-engine ◀── persistence + indicators (因子计算 + IC/IR 评估)

Phase 2 ─ 引擎与策略
  tg-engine ◀── contracts + persistence       (★ 共享事件驱动核心)
       │
       ├── tg-backtest ◀── engine (历史回放 + 绩效分析)
       └── tg-signal-engine ◀── engine + indicators + factors (指标+因子→信号)

Phase 3 ─ 决策与执行
  tg-decision-agent ◀── signal-engine + LLM API (多 agent 决策)
  tg-mock-order-engine ◀── engine + persistence + market-data (实时撮合 + 虚拟账户)

Phase 4 ─ 可视化与编排
  tg-monitoring-viz ◀── persistence (Axum + TS 前端)
  tg-infra (docker-compose / 配置 / 调度 / 可观测性)
```

### Phase 0 — 数据地基

#### 1. `tg-contracts` — 共享契约层
- **职责**：全系统的单一事实来源，所有跨服务数据类型与 RPC 接口集中定义。
- **核心功能**
  - gRPC proto：行情查询、指标计算、因子查询、信号订阅、订单提交、持仓查询、回测任务等
  - 公共领域类型：`Bar`、`Snapshot`、`Instrument`、`Order`、`Fill`、`Position`、`Signal`、`FactorValue`、`Event`、`Account`
  - 枚举/常量：市场(SH/SZ/BJ)、订单方向/类型/状态、复权类型、信号方向
  - A 股规则常量：T+1、涨跌停幅度(主板±10%/科创创业±20%/北交±30%)、最小交易单位100股、交易时段、集合/连续竞价
  - 时间模型：交易日历、`Timestamp`、交易日/时段判定
- **输入 → 输出**：无运行时输入 → Rust crate（tonic 类型 + 领域类型）+ proto（给 C++ 生成）
- **依赖**：无
- **边界**：不含任何业务逻辑，纯定义。

#### 2. `tg-market-data` — 行情采集
- **职责**：把外部行情源（akshare/tushare）数据标准化后采入系统。
- **核心功能**
  - 历史采集：日 K、分钟 K（1m/5m/15m），全量/增量更新
  - 实时快照采集：交易时段内 3-5 秒轮询，拉实时报价（开高低收/量额/买卖盘）
  - 数据源适配器：抽象 `DataSource` trait，先实现 akshare
  - 复权计算：前复权 / 后复权 / 不复权
  - 数据清洗校验：涨跌停价校验、停牌处理、除权除息修正、缺失/异常检测
  - 标的元数据同步：股票列表、上市/退市、ST 标记、板块、交易日历
  - 限频容错：速率限制 + 指数退避 + 断点续采
- **输入 → 输出**：外部数据源 API + 采集配置 → 标准化数据写入 persistence；gRPC 提供历史/实时查询
- **依赖**：`tg-contracts`、`tg-persistence`
- **数据接入方式（ADR-011）**：akshare 等免费源由 **Python sidecar** 包装成 gRPC/HTTP 服务（market-data 仓库内子组件 `collector-python/`）；Rust 主服务负责调度/限频/清洗/复权/落库。Python 仅做"取数"，不做业务逻辑。
- **边界**：不做指标/因子/策略。

#### 3. `tg-persistence` — 持久化层（共享库 crate）
- **职责**：系统统一存储与数据访问层，屏蔽数据库细节。
- **形态（ADR-017）**：**共享库 crate**（非独立服务），被 market-data（写）和其他模块（读）链接；Postgres 处理并发，Parquet 由 market-data 单写、其余 DuckDB 只读。
- **核心功能**
  - 双存储分工：PostgreSQL（元数据/订单/持仓/账户/信号/因子元数据/回测记录）+ Parquet/DuckDB（时序行情/因子值）
  - Schema 管理：迁移脚本、版本化（sqlx migrate）
  - 行情分区：按 `标的 + 周期 + 年/月` 分区
  - `Repository` trait：抽象数据访问，上层不关心底层库
  - 读写接口：批量写 Bar、按时间范围查、查最新快照、订单/持仓 CRUD
- **输入 → 输出**：其他模块读写请求 → 统一数据访问服务（gRPC + Rust crate）
- **依赖**：`tg-contracts`
- **边界**：不做数据采集，不含业务逻辑。

### Phase 1 — 分析能力

#### 4. `tg-indicators` (C++20) — 技术指标服务
- **职责**：现有资产，技术指标计算引擎，改造为 gRPC 服务。
- **核心功能**
  - 现有指标：ADX、ATR、布林带、CCI、EMA、MACD、OBV、RSI、SMA、KDJ、威廉 %R
  - 输入：Bar 序列（OHLCV）+ 参数；输出：指标值序列
  - 参数化：每指标支持可配置周期
  - 改造：① 加 gRPC 服务端；② 对齐 `tg-contracts` 的 Bar 类型；③（可选）保留 MCP 接口供 decision-agent 调用
- **输入 → 输出**：gRPC `ComputeIndicator(type, params, bars)` → 指标值序列
- **依赖**：`tg-contracts`（proto）
- **边界**：不做因子/信号/策略，纯数值计算。

#### 5. `tg-factor-engine` — 因子引擎
- **职责**：量化因子计算 + 因子库管理 + 因子有效性评估。
- **核心功能**
  - 因子库：动量、反转、波动率、成交量、资金流、规模、价值等因子族
  - `Factor` trait：声明式注册（输入历史 Bar + 横截面数据，输出因子值）
  - 横截面计算：某时刻全市场因子值（选股排名）
  - 时序计算：单标的历史因子值（信号触发）
  - 因子评估：IC（信息系数）、IR（信息比率）、分层回测、因子衰减——结果入 persistence
  - 因子元数据注册表：名称、类别、逻辑、数据依赖
  - 指标复用：部分因子复用 indicators（如 RSI 因子）
- **输入 → 输出**：persistence 行情 + 参数 → 因子值（存库）+ 评估报告 + gRPC 查询
- **依赖**：`tg-contracts`、`tg-persistence`、（可选 `tg-indicators`）
- **边界**：不生成买卖信号，不做策略。

### Phase 2 — 引擎与策略

#### 6. `tg-engine` — 事件驱动核心 ★
- **职责**：策略运行内核，**回测与模拟同一套引擎**。
- **核心功能**
  - `Strategy` trait：`on_bar`、`on_event`、`on_timer` 回调，策略产出订单意图
  - 事件循环：调度 Bar/Snapshot/Timer/Fill 事件给策略
  - `ExecutionHandler` trait：回测接历史撮合器，模拟接 mock-order-engine（**mock/实盘切换点**）
  - `DataFeed` trait：回测接历史回放，模拟接实时
  - 组合视图：策略可读持仓、资金、挂单
  - 时钟抽象：回测=历史时间，模拟=墙钟
  - 横截面快照：某时刻全 universe 数据视图（多标的策略）
- **形态**：library crate（非独立服务），被 backtest 和 mock-order-engine 链接复用
- **依赖**：`tg-contracts`、`tg-persistence`
- **边界**：不含具体策略、不含具体撮合规则（由执行器注入）。

#### 7. `tg-backtest` — 回测与绩效分析
- **职责**：历史回测驱动 + 绩效统计。
- **核心功能**
  - 回测驱动：从 persistence 读历史，按时间回放给 tg-engine
  - 历史撮合器：按 OHLC（或更细）模拟成交
  - A 股规则：T+1、涨跌停、最小 100 股、集合竞价、滑点/手续费/印花税
  - 绩效分析：收益曲线、年化、夏普、最大回撤、胜率、盈亏比、与基准对比
  - 运行记录：配置 + 结果存 persistence，可对比多次
  - 参数寻优（后期）：网格 / 贝叶斯优化
  - gRPC：提交回测任务、查询进度与结果
- **依赖**：`tg-contracts`、`tg-engine`、`tg-persistence`
- **边界**：不做实时运行、不下真实订单。

#### 8. `tg-signal-engine` — 信号引擎
- **职责**：把指标 + 因子编排成结构化买卖信号（规则驱动层）。
- **核心功能**
  - 基于 tg-engine 实现（本身是一个/一组策略）
  - 规则引擎：多指标 + 因子条件组合（例：MACD 金叉 AND RSI<30 AND 动量因子排名前10%）
  - 信号结构：方向（多/空/平）、强度、置信度、**触发原因**（审计可解释）
  - 横截面选股：全市场按因子打分筛选
  - 信号发布：产出 Signal 事件，供 decision-agent 消费或直接进 mock-order-engine
- **与 decision-agent 关系**：signal-engine=**候选产生者**（规则驱动，产出信号候选），decision-agent=**最终决策者**（LLM 驱动，拍板）。signal-engine 不直接触发交易。见 ADR-010。
- **策略原型（ADR-014，三套）**：**波段**（日/分钟K）、**T0做T**（分钟/实时，对已有持仓做T+0）、**打板**（实时秒级，涨停封板检测+尾盘买+次日卖）。为每套原型实现独立信号规则。
- **依赖**：`tg-contracts`、`tg-engine`、`tg-indicators`、`tg-factor-engine`
- **边界**：不做 LLM 决策、不直接下单。

### Phase 3 — 决策与执行

#### 9. `tg-decision-agent` — LLM 多 agent 决策层 ★ 最终决策者
- **职责**：系统的**权威决策层**（ADR-010）。多 agent LLM 融合信号/因子/市场状态，对"是否交易、仓位多少"做最终拍板；signal-engine 的信号仅作为输入候选。产出决策交 mock-order-engine 执行。
- **注意（待澄清 Q7）**：若每笔订单（含止损）都过 LLM，延迟与可靠性成问题。倾向方案：**开仓经 agent，平仓/止损/止盈由规则直接执行**——待确认。
- **核心功能**
  - 多 agent 编排：分析师 agent（解读信号+因子）→ 交易员 agent（决策）→ 风控 agent（否决权）
  - LLM 接入（ADR-015）：**多 provider 抽象**，OpenAI 兼容协议为主（覆盖 deepseek / qwen / glm / moonshot / OpenAI），可配 `base_url + api_key + model`；可经 MCP 调 indicators
  - 上下文组装：prompt = 标的 + 信号 + 因子 + 指标 + 市场状态 + 历史决策
  - 结构化输出：买/卖/持 + 仓位建议 + 理由（JSON schema 约束）
  - 决策全程日志：审计、复盘、未来可微调
  - 规则兜底：LLM 不可用时降级为 signal-engine 规则
- **输入 → 输出**：signal-engine 信号 + 上下文 → 结构化决策（交 mock-order-engine）
- **依赖**：`tg-contracts`、`tg-signal-engine`、`tg-factor-engine`、LLM API、（MCP/indicators）
- **边界**：不直接下单、不采行情。

#### 10. `tg-mock-order-engine` — 模拟下单引擎
- **职责**：实时模拟下单 + 内置撮合 + 虚拟账户 + A 股规则引擎。
- **核心功能**
  - 实时驱动：用 tg-engine 在墙钟运行，消费 market-data 实时快照
  - 内置撮合：收到订单按实时行情模拟成交（涨跌停、滑点、部分成交）
  - A 股规则引擎：T+1（买入当日锁定）、涨跌停板、最小 100 股、集合竞价、资金/持仓校验
  - **T+0 做T 支持（ADR-014）**：区分"昨日持仓"（今日可卖）与"今日买入"（今日不可卖），支持日内卖出昨日持仓后回买，净持仓不为负
  - **打板执行（ADR-014）**：涨停价/封板检测、尾盘限价单逻辑
  - **品种差异化（ADR-013）**：ETF 免印花税、部分 ETF 支持 T+0（跨境/货币/债券 ETF）——按 instrument 类型应用不同规则
  - 虚拟账户：现金、持仓、冻结资金、可用/不可用（T+1）区分
  - 精确成本模型：佣金、印花税（卖出 0.05%）、过户费（沪市）
  - 软风控：单标的仓位上限、总仓位、止损、黑名单（无真实风险）
  - 订单生命周期：提交/部分成交/成交/撤单/拒绝
  - gRPC：提交订单、查询持仓/账户/订单
- **依赖**：`tg-contracts`、`tg-engine`、`tg-persistence`、`tg-market-data`
- **边界**：不接真实券商。未来升级实盘新增 `tg-broker-gateway`（实现同一 `ExecutionHandler` 接口，切换注入即可）。

### Phase 4 — 可视化与编排

#### 11. `tg-monitoring-viz` — 后台可视化
- **职责**：把系统运行状态和交易绩效可视化，Web 访问。
- **核心功能**
  - 后端（Rust/Axum）：REST/gRPC API，从 persistence 读 + 订阅实时事件
  - 交易仪表盘：实时持仓/盈亏、账户净值曲线、今日订单
  - 绩效统计：回测/模拟收益、夏普、回撤、胜率图表
  - 信号/决策监控：实时信号流、agent 决策日志
  - 行情图表：K 线 + 指标叠加（lightweight-charts/ECharts）
  - 因子分析视图：IC/IR、分层收益
  - 告警/通知（后期）
- **前端**：TypeScript（React/Vue + 图表库）
- **依赖**：`tg-persistence`（读）、`tg-contracts`、订阅 signal/order 事件
- **边界**：不含交易逻辑、不采数据。

#### 12. `tg-infra` — 部署编排
- **职责**：把所有服务编排起来跑 + 共享基础设施配置。
- **核心功能**
  - docker-compose：编排所有服务、Postgres 依赖、卷、网络
  - 配置管理：各服务配置（标的范围、轮询频率、数据源凭证）——环境变量/配置文件
  - 定时调度：收盘后采集日 K、每日因子计算、定时回测（cron）
  - 可观测性：结构化日志、Prometheus 指标（后期链路追踪）
  - 健康检查：各服务 liveness/readiness
  - 密钥管理：数据源 token、LLM API key
- **依赖**：所有服务
- **边界**：不含业务功能。

### 4.99 全景速查表

| 模块 | 语言 | 职责一句话 | 关键依赖 | 状态 |
|---|---|---|---|---|
| tg-contracts | proto+Rust | 公共类型 + RPC 定义 | — | ⬜ 待开发 |
| tg-market-data | Rust | 行情采集（历史+实时） | contracts, persistence | ⬜ 待开发 |
| tg-persistence | Rust | 存储层（PG+Parquet/DuckDB） | contracts | ⬜ 待开发 |
| tg-indicators | C++20 | 技术指标计算 | contracts | 🟡 有现成资产待改造 |
| tg-factor-engine | Rust | 因子计算+评估 | contracts, persistence | ⬜ 待开发 |
| tg-engine | Rust | 事件驱动核心（回测/模拟共用） | contracts, persistence | ⬜ 待开发 |
| tg-backtest | Rust | 历史回测+绩效 | engine, persistence | ⬜ 待开发 |
| tg-signal-engine | Rust | 指标+因子→信号 | engine, indicators, factors | ⬜ 待开发 |
| tg-decision-agent | Rust | LLM 多 agent 决策 | signal-engine, LLM | ⬜ 待开发 |
| tg-mock-order-engine | Rust | 实时模拟撮合+虚拟账户 | engine, market-data, persistence | ⬜ 待开发 |
| tg-monitoring-viz | Rust+TS | Web 后台可视化 | persistence | ⬜ 待开发 |
| tg-infra | compose | 部署+调度+可观测 | 全部 | ⬜ 待开发 |

> 状态图例：⬜ 待开发 / 🟡 有资产待改造 / 🚧 进行中 / ✅ 完成

---

## 5. 技术栈与语言分工

| 模块 | 语言 | 理由 |
|---|---|---|
| tg-contracts | protobuf + Rust bindings | IDL 语言无关，Rust 生成类型 |
| tg-market-data | **Rust** | tokio 异步轮询、reqwest、易并发 |
| tg-persistence | **Rust** | sqlx + DuckDB/Parquet 生态成熟 |
| tg-indicators | **C++20** | 现有资产，模板化数值计算 |
| tg-factor-engine | **Rust**（部分 FFI 调 C++） | 向量化运算，polars/ndarray 够用 |
| tg-engine | **Rust** | trait + 异步事件循环 |
| tg-backtest | **Rust** | 复用 engine |
| tg-signal-engine | **Rust** | 编排 indicators + factors |
| tg-decision-agent | **Rust**（调 LLM HTTP API） | C++/Rust 栈；Python 仅作可选研究 notebook |
| tg-mock-order-engine | **Rust** | 复用 engine + 撮合逻辑 |
| tg-monitoring-viz | **Rust(Axum)** + **TypeScript(前端)** | Web 标准栈 |
| tg-infra | docker-compose + 配置 | — |

### 关键库（候选）
- **Rust**：tokio（异步运行时）、tonic（gRPC）、prost（protobuf）、sqlx（Postgres）、duckdb-rs / arrow / parquet（列存）、polars / ndarray（向量化）、axum（Web 后端）、reqwest（HTTP）、tracing（日志/追踪）
- **C++20**：grpc、protobuf、（现有指标实现）
- **前端**：React 或 Vue + lightweight-charts / ECharts

---

## 6. 构建顺序（Phase 0 → 4）

```
Phase 0 数据地基  → contracts + market-data + persistence
Phase 1 分析能力  → indicators(改造) + factor-engine
Phase 2 引擎策略  → engine + backtest + signal-engine
Phase 3 决策执行  → decision-agent + mock-order-engine
Phase 4 可视编排  → monitoring-viz + infra
```

每个 Phase 完成都可演示：
- Phase 0 结束：数据落库可查
- Phase 1 结束：可算指标/因子并评估
- Phase 2 结束：可跑回测
- Phase 3 结束：可跑纸面模拟（信号→决策→模拟下单）
- Phase 4 结束：全链路 Web 可视

---

## 7. 关键设计决策记录（ADR）

### 已决策
- **ADR-001 交易模式**：纸面模拟（非实盘）。→ 避开券商/合规/真金风控最复杂部分。
- **ADR-002 行情数据**：免费秒级快照，轮询拉取。→ 不需要低延迟流式基础设施。
- **ADR-003 部署形态**：个人容器化，服务化边界。→ docker-compose，单用户。
- **ADR-004 宏观架构**：方案 A（gRPC 同步 + 进程隔离），边界设计可向事件总线演进。
- **ADR-005 C++ 指标接入**：A1（独立 gRPC 服务），非 FFI。
- **ADR-006 回测/模拟共用引擎**：`tg-engine` 事件驱动核心 + `ExecutionHandler`/`DataFeed` trait。
- **ADR-007 存储**：PostgreSQL + Parquet/DuckDB（非 ClickHouse/TDengine，YAGNI）。
- **ADR-008 语言分工**：C++20 仅 indicators，其余系统模块 Rust，前端 TypeScript。
- **ADR-009 mock/实盘切换**：`ExecutionHandler` trait 为切换点，mock-order-engine 与未来 broker-gateway 共同实现。
- **ADR-010 决策权归属**：`tg-decision-agent` 是**最终决策者**——signal-engine 产出的信号仅作为 agent 的输入候选，是否交易、仓位大小由 agent 拍板。数据流：`signal-engine → decision-agent → mock-order-engine`。
- **ADR-011 数据接入**：免费行情源由 **Python sidecar** 包装（akshare 等），Rust `tg-market-data` 经 gRPC/HTTP 调用。Python 仅负责"取数"，调度/限频/清洗/复权/落库由 Rust 主服务承担。
- **ADR-012 标的范围**：watchlist（自选股池），非全市场扫描。
- **ADR-013 标的品种**：A 股股票 + ETF；不含指数、可转债。
- **ADR-014 策略风格**：波段 / T0 做T / 打板 三套，signal-engine 分别实现，mock-order-engine 支持 T+0 与打板执行。
- **ADR-015 LLM 接入**：多 provider 抽象层，以 OpenAI 兼容协议为主（覆盖 deepseek / qwen / glm / moonshot / OpenAI），运行时可配 `base_url + api_key + model`。
- **ADR-016 历史数据深度**：日 K 回溯 5 年；分钟 K（1m/5m）取 akshare 免费源可得范围（约 1-2 年）；实时秒级快照从启动起累积。
- **ADR-017 persistence 形态**：`tg-persistence` 为**共享库 crate**（非独立服务）。market-data 链接它写入，其余模块链接它读取；Postgres 处理并发，Parquet 由 market-data 单写、其余 DuckDB 只读。
- **ADR-018 Python sidecar 协议**：collector-python 用 **HTTP/REST (FastAPI)** 暴露取数接口，Rust market-data 经 reqwest 调用。

**Phase 1（分析能力，见 Phase 1 spec）：**
- **ADR-021 因子评估 IC 选型**：采用 Spearman rank IC（稳健抗异常值）。
- **ADR-022 因子复用 indicators 边界**：首批仅 RSI 因子经 gRPC 复用 tg-indicators；不可达降级。
- **ADR-023 横截面标准化方法**：rank-based 标准化（去极值 + 排序归一）。
- **ADR-024 因子值存储分区**：`factor=<NAME>/date=<YYYYMMDD>` 分区（区别于行情 symbol 分区）。

**Phase 2（引擎策略，见 Phase 2 spec）：**
- **ADR-025 事件循环调度模型**：同时间戳按 `Fill < Bar < Snapshot < Timer` 确定性优先级 + `seq` tie-break，逐事件可复现。
- **ADR-026 历史撮合器精度**：保守 OHLC 四点模型（回测）。
- **ADR-027 绩效指标口径**：年化/夏普/最大回撤/胜率/盈亏比统一定义。
- **ADR-028 Signal.reason 编码**：结构化条件码 + 文本，便于审计与统计。
- **ADR-029 signal-engine OrderSink**：注入 `NoopSink`，代码级保证不直接下单（ADR-010 兜底）。

**Phase 3（决策执行，见 Phase 3 spec）：**
- **ADR-019 入场/出场路径（Q7）**：开仓/加仓/减仓/平仓经 decision-agent(LLM)；**硬止损/硬止盈由 mock-order-engine 规则直执**（秒级，不等 LLM）。
- **ADR-020 LLM 兜底（Q8）**：LLM 不可用时**持仓保持 + 不开新仓**，规则止损照常运行；探针恢复后退出降级。
- **ADR-030 LLM provider 抽象**：单一 trait + 单一 OpenAICompatibleClient 实现（覆盖 deepseek/qwen/glm/moonshot/OpenAI）。
- **ADR-031 结构化输出 + 成本精度**：JSON Schema 约束 LLM 输出；成本全程 `Decimal`。
- **ADR-032 撮合假设**：保守部分成交模型（无队列/对手盘深度，宁可少成交不可虚增）。
- **ADR-033 T0 持仓分桶**：按交易日分桶 + FIFO 扣桶，满足 T+1（昨日桶可卖、今日桶锁定）。

**Phase 4（可视编排，见 Phase 4 spec）：**
- **ADR-034 前端框架**：React + Vite + TypeScript。
- **ADR-035 实时事件推送**：SSE（Server-Sent Events）。
- **ADR-036 配置与密钥管理**：分层配置（默认/env/文件）+ Docker secrets。
- **ADR-037 调度实现**：容器内 cron（supercronic / ofelia）。

### 待决策（见 §9）
- 全部关键决策已落地（ADR-001~037）；§9 中 Q1~Q8 全部已决。

---

## 8. 第一个子项目（待启动）

**数据地基**：`tg-contracts` + `tg-market-data` + `tg-persistence`

- **目标**：把 A 股行情（历史日/分钟 K + 实时秒级快照）按规范 schema 可靠采入，落到 PostgreSQL + Parquet/DuckDB，并提供查询接口。
- **数据范围（v0.5 锁定）**：日K 5年 + 分钟K(1m/5m, 取 akshare 可得 ~1-2年) + 实时秒级快照；前复权（回测/信号）+ 不复权原始价（实时下单）；watchlist 配置文件管理；akshare 经 Python sidecar。
- **理由**：一切的前提；三者强耦合须一起设计；边界清晰可演示；风险前置。
- **状态**：🚧 详细设计进行中。

---

## 9. 待解决问题（Open Questions）

| # | 问题 | 影响模块 | 状态 |
|---|---|---|---|
| ~~Q1~~ | ~~akshare Rust 接入方式~~ | ✅ **已决（ADR-011）**：Python sidecar |
| ~~Q2~~ | ~~decision-agent 定位~~ | ✅ **已决（ADR-010）**：最终决策者 |
| ~~Q3~~ | ~~标的范围~~ | ✅ **已决（ADR-012）**：watchlist |
| ~~Q4~~ | ~~策略风格~~ | ✅ **已决（ADR-014）**：波段 + T0 + 打板 |
| ~~Q5~~ | ~~标的品种~~ | ✅ **已决（ADR-013）**：A股股票 + ETF |
| ~~Q6~~ | ~~LLM 提供商~~ | ✅ **已决（ADR-015）**：多 provider 抽象（OpenAI 兼容协议） |
| ~~Q7~~ | ~~入场/出场路径~~ | ✅ **已决（ADR-019）**：开仓/加仓/减仓/平仓经 decision-agent(LLM)；硬止损/硬止盈由 mock-order-engine 规则直执（秒级，不等 LLM） |
| ~~Q8~~ | ~~LLM 不可用兜底~~ | ✅ **已决（ADR-020）**：LLM 不可用时持仓保持 + 不开新仓，规则止损照常运行；探针恢复后退出降级 |

---

## 10. 进度跟踪

| Phase | 模块 | 设计 spec | 编码 |
|---|---|---|---|
| 0 | tg-contracts / tg-market-data / tg-persistence | ✅ 完成 | ⬜ 待启动（codex） |
| 1 | tg-indicators / tg-factor-engine | ✅ 完成 | ⬜ 待启动 |
| 2 | tg-engine / tg-backtest / tg-signal-engine | ✅ 完成 | ⬜ 待启动 |
| 3 | tg-decision-agent / tg-mock-order-engine | ✅ 完成 | ⬜ 待启动 |
| 4 | tg-monitoring-viz / tg-infra | ✅ 完成 | ⬜ 待启动 |

## 11. 详细设计 Spec 索引

| 文档 | 覆盖 | 状态 |
|---|---|---|
| `2026-06-14-tg-contracts-design.md` | tg-contracts（全系统共享类型 + gRPC 服务目录 + 编码规范） | ✅ |
| `2026-06-14-phase0-data-foundation-design.md` | tg-market-data + tg-persistence（数据地基） | ✅ |
| `2026-06-14-phase1-analysis-design.md` | tg-indicators + tg-factor-engine | ✅ |
| `2026-06-14-phase2-engine-strategy-design.md` | tg-engine + tg-backtest + tg-signal-engine | ✅ |
| `2026-06-14-phase3-decision-execution-design.md` | tg-decision-agent + tg-mock-order-engine | ✅ |
| `2026-06-14-phase4-viz-infra-design.md` | tg-monitoring-viz + tg-infra | ✅ |

> 实现顺序遵循 §6 构建顺序：Phase 0 → 4。每 Phase 先用 codex 全自动实现（对照 spec），再人工审阅 + 提交。

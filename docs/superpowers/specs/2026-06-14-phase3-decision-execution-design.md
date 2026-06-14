# Phase 3 决策与执行 — 详细设计 Spec

> **子项目**：`tg-decision-agent` + `tg-mock-order-engine`
> **状态**：设计完成，待用户评审
> **日期**：2026-06-14
> **上游文档**：`2026-06-14-tradeglance-architecture-design.md`（v0.6）
> **依赖文档**：`2026-06-14-tg-contracts-design.md`（权威类型）、`2026-06-14-phase0-data-foundation-design.md`（Phase 0 模板）
> **相关 ADR**：ADR-009 / 010 / 013 / 014 / 015（已有）+ ADR-019 / 020 / 030 / 031 / 032 / 033（本期新增）

---

## 1. 概述与范围

### 1.1 目标
打通系统的"决策 → 执行"半环：把 Phase 2 `tg-signal-engine` 产出的结构化信号（候选）交给 `tg-decision-agent`（多 agent LLM 编排）做最终拍板，产出 `Decision`；再把 `Decision` 翻译成 `Order` 交 `tg-mock-order-engine`（内置撮合 + 虚拟账户 + A 股规则引擎 + T0/打板/品种差异化执行）在墙钟下实时模拟下单，并把订单/成交/持仓/账户全量落 PostgreSQL，完成"信号 → 决策 → 模拟下单"的纸面闭环（架构文档 §6 Phase 3 演示目标）。

本 Phase 同时落地架构文档 §9 延期的两个关键决策：**Q7 入场/出场路径**（ADR-019）与 **Q8 LLM 兜底策略**（ADR-020），并补充 4 项区间决策（ADR-030~033）。

### 1.2 In Scope
- `tg-decision-agent`：多 agent 编排（分析师 / 交易员 / 风控）、LLM 多 provider 抽象层、上下文组装、JSON schema 结构化输出、决策日志、规则兜底、`DecisionService` gRPC。
- `tg-mock-order-engine`：实现 Phase 2 `tg-engine` 的 `ExecutionHandler` trait、内置撮合引擎、A 股规则引擎（T+1 / 涨跌停 / 最小 100 股 / 集合竞价 / 资金持仓校验）、T0 持仓分桶、打板执行、品种差异化（ETF）、虚拟账户状态机、精确成本模型、软风控、订单生命周期、`OrderService` gRPC。
- PostgreSQL schema：`orders` / `fills` / `positions` / `accounts` 完整 DDL（经 `tg-persistence`）。
- Q7 / Q8 决策与 ADR-019 / 020。

### 1.3 Out of Scope（YAGNI，留给后续）
- 真实券商接入（Phase 5+ `tg-broker-gateway`，预留切换路径见 §11）。
- 决策的强化学习/在线微调（先用审计日志积累，离线分析后人工调）。
- 跨账户多策略组合优化（单虚拟账户，多 `strategy_tag` 标记溯源）。
- LLM 工具调用（function calling / MCP 指标查询）的非 OpenAI 兼容协议适配（ADR-015 锁定 OpenAI 兼容为主，工具调用在 Phase 3.5 视必要再加）。
- Web 可视化（Phase 4 `tg-monitoring-viz`，仅预留事件订阅接口）。

---

## 2. 模块形态与依赖

| 模块 | 形态 | 语言 | 角色 |
|---|---|---|---|
| `tg-decision-agent` | gRPC 服务（独立进程） | Rust | **最终决策者**（ADR-010）：消费 Signal，多 agent LLM 拍板产出 `Decision` |
| `tg-mock-order-engine` | gRPC 服务 + 链接 `tg-engine`（library crate） | Rust | **执行者**：消费 `Decision`→`Order`，内置撮合 + 虚拟账户 + A 股规则 |

### 2.1 依赖关系图

```
                       ┌──────────────────────────────────────────────┐
                       │              LLM Provider                    │
                       │  (OpenAI 兼容: deepseek/qwen/glm/moonshot/    │
                       │   OpenAI)  base_url + api_key + model         │
                       └────────────────────▲─────────────────────────┘
                                            │ HTTPS (reqwest)
   ┌──────────────────────┐   Signal(gRPC)  │              ┌──────────────────────┐
   │  tg-signal-engine    │──────────────▶ ┌──────────────┐│   tg-persistence     │
   │  (Phase 2)           │                │tg-decision-  ││   (共享库 crate)     │
   └──────────────────────┘                │   agent      ││  orders/fills/       │
                                           └──────┬───────┘│  positions/accounts  │
                                                  │ Decision│  (PG)                │
                                                  │ (gRPC)  │                      │
                                                  ▼         │                      │
                                           ┌──────────────┐│                      │
   ┌──────────────────────┐  Snapshot      │tg-mock-order-││                      │
   │  tg-market-data      │  (链接persistence)│  engine   │├──────链接写──────────▶│
   │  (Phase 0)           │──────────────▶ │              ││                      │
   └──────────────────────┘                └──────┬───────┘│                      │
                                                  │ 实现    │                      │
                                                  ▼         │                      │
                                           ┌──────────────┐│                      │
                                           │  tg-engine   │├──────链接读──────────▶│
                                           │ ExecutionHandler│                    │
                                           │ DataFeed trait│                      │
                                           └──────────────┘                      │
   ┌──────────────────────┐  Fill(gRPC订阅)│                                      │
   │tg-monitoring-viz     │◀───────────────┘                                      │
   │  (Phase 4, 预留)     │                                                       │
   └──────────────────────┘                                                       │
                                                                                  │
   tg-contracts (Phase 0, 链接) ── 所有模块共享类型 ──────────────────────────────▶│
```

依赖清单：
- `tg-decision-agent` → `tg-contracts`、`tg-signal-engine`（订阅信号）、`tg-factor-engine`（取因子上下文，gRPC）、`tg-persistence`（写决策日志/读历史决策）、LLM HTTP API。
- `tg-mock-order-engine` → `tg-contracts`、`tg-engine`（library，`ExecutionHandler` / `DataFeed` trait）、`tg-persistence`（链接读写）、`tg-market-data`（链接 persistence 读实时快照 + gRPC 触发）。

---

## 3. tg-decision-agent 详细设计

### 3.1 多 agent 编排（Pipeline）

三个 agent 顺序串行，每步有明确产物；任一步失败走 §3.7 兜底。编排器（`DecisionOrchestrator`）为单入口，`Decide` gRPC 调用即一次完整 pipeline。

```
Signal + Context
   │
   ▼
[分析师 agent]  ← prompt: 标的 + 信号(strength/confidence/reason) + 因子值 + 指标值 + 市场状态(大盘/板块) + 历史决策
   │  产出: 分析摘要 JSON {summary, bull_points[], bear_points[], factor_read{}, score}
   ▼
[交易员 agent]  ← prompt: 分析摘要 + 当前持仓 + 账户可用资金 + 风格偏好(StrategyStyle) + 仓位约束
   │  产出: 决策草案 JSON {action: Open/Add/Reduce/Close/Hold, side, target_quantity, rationale}
   ▼
[风控 agent]    ← prompt: 决策草案 + 风控规则集(单标的仓位上限/总仓位/黑名单/ST/涨跌停临近) + 持仓集中度
   │  产出: RiskCheckResult[] + 是否否决 + 调整后 target_quantity
   ▼
Decision (action/side/target_quantity/rationale/risk_checks)
```

- **分析师 agent**：只读，纯解读；输出结构化分析摘要，**不决策**。
- **交易员 agent**：唯一产出 `action` 与 `target_quantity` 的 agent；输入需含风控约束（软约束 prompt 化，硬约束由风控 agent + mock-order-engine 双重把关）。
- **风控 agent**：**否决权**。可降级 `action`（Open→Hold）或削减 `target_quantity`；任何 `passed=false` 的硬规则直接将 `action` 改写为 `Hold` 并记录否决理由。
- 每个 agent 调用 LLM 一次；三步共 3 次 LLM 调用。为保证延迟可控，每步设超时（默认 15s，可配），超时走兜底（§3.7）。

### 3.2 LLM Provider 抽象层（ADR-015 落地，新 ADR-030 接口形态）

```rust
/// 单次 LLM 调用请求。
pub struct LlmRequest {
    pub provider: ProviderId,         // "openai" / "deepseek" / "qwen" / "glm" / "moonshot"
    pub model: String,
    pub system: String,               // 角色 prompt（分析师/交易员/风控）
    pub messages: Vec<ChatMessage>,   // 历史对话（含上下文组装结果）
    pub json_schema: Option<serde_json::Value>,  // 结构化输出约束（ADR-031）
    pub temperature: f64,
    pub max_tokens: u32,
    pub timeout: std::time::Duration,
}

pub struct LlmResponse {
    pub content: String,              // 原始文本或 JSON
    pub finish_reason: FinishReason,  // Stop / Length / ContentFilter / Timeout
    pub usage: TokenUsage,            // prompt/completion tokens（成本追踪）
    pub latency: std::time::Duration,
}

/// 抽象 trait，所有 OpenAI 兼容 provider 共享一个实现（OpenAICompatibleClient），
/// 仅 base_url/api_key/model 不同。
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
    fn provider(&self) -> ProviderId;
}

/// 运行时按配置构造：base_url + api_key + model + 可选请求头（如 GLM 的特定 header）。
pub struct OpenAICompatibleClient { /* reqwest::Client + base_url + api_key */ }
```

- **决策（ADR-030）**：trait 形态为 `async complete(LlmRequest) -> LlmResponse`，**单一 `OpenAICompatibleClient` 实现覆盖全部 5 家 provider**，差异仅在 `base_url` + `api_key` + `model` + 少量 header（不为此抽象多 trait）。Provider 配置由 `decision.toml` 注入。
- LLM 概率/置信度/score/temperature 等 LLM 域数值用 `f64`；金融数值（仓位、价格）绝不混入 LLM 输出原值，必须经风控 agent / mock-order-engine 二次量化（见 ADR-031）。

### 3.3 上下文组装

`ContextBuilder` 在 `Decide` 入口组装 prompt 上下文，**全部从 `tg-contracts` 已定义类型取值**，不在本模块重新定义：

| 上下文段 | 来源 | 类型 |
|---|---|---|
| 标的 | watchlist / 请求参数 | `Instrument`（symbol/exchange/board/instrument_type/is_st） |
| 信号 | signal-engine gRPC `SignalService.SubscribeSignals` | `Signal`（direction/strength/confidence/style/reason/suggested_quantity） |
| 因子 | factor-engine gRPC `FactorService.QueryFactorValues` | `Vec<FactorValue>`（value/rank） |
| 指标 | indicators gRPC `IndicatorService.Compute`（按需）或 factor-engine 缓存 | `IndicatorResult` |
| 市场状态 | 链接 persistence 读 latest_snapshots / 大盘指数快照 | `Snapshot`（指数代理）+ 涨跌停临近判定 |
| 当前持仓 | mock-order-engine gRPC `OrderService.QueryPositions`（只读） | `Vec<Position>` |
| 账户资金 | mock-order-engine gRPC `OrderService.QueryAccount`（只读） | `Account`（cash/frozen_cash/total_value） |
| 历史决策 | persistence 链接读 `decisions` 表（本期本标的最近 N 条） | `Vec<Decision>`（仅 action/rationale/ts） |

组装为结构化 JSON（`system` prompt 描述角色，`user` message 嵌入 JSON 上下文），避免自由文本拼接导致的 prompt 注入；输入 JSON 经字段白名单过滤后再进 prompt。

### 3.4 JSON Schema 结构化输出（ADR-031）

决策（ADR-031）：**用 JSON Schema 约束 LLM 输出**（OpenAI 兼容 `response_format: {type:"json_schema", json_schema:{...}}`，对不支持该字段的 provider 退化为 prompt 内嵌 schema + 服务端严格解析校验）。三个 agent 各有独立 schema：

```jsonc
// 分析师 agent 输出
{
  "type": "object",
  "required": ["summary", "score"],
  "properties": {
    "summary":   {"type": "string", "maxLength": 500},
    "bull_points": {"type": "array", "items": {"type": "string"}},
    "bear_points": {"type": "array", "items": {"type": "string"}},
    "factor_read": {"type": "object"},   // factor_name -> 一句话解读
    "score":     {"type": "number", "minimum": 0, "maximum": 1}  // f64 综合评分
  }
}

// 交易员 agent 输出
{
  "type": "object",
  "required": ["action", "target_quantity", "rationale"],
  "properties": {
    "action":    {"type": "string", "enum": ["Open","Add","Reduce","Close","Hold"]},
    "side":      {"type": "string", "enum": ["Buy","Sell"]},
    "target_quantity": {"type": "integer", "minimum": 0, "multipleOf": 100},  // LOT_SIZE 校验
    "rationale": {"type": "string", "maxLength": 1000}
  }
}

// 风控 agent 输出
{
  "type": "object",
  "required": ["approved", "checks"],
  "properties": {
    "approved":  {"type": "boolean"},
    "checks": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["rule","passed","detail"],
        "properties": {
          "rule":   {"type": "string"},
          "passed": {"type": "boolean"},
          "detail": {"type": "string"}
        }
      }
    },
    "adjusted_quantity": {"type": "integer", "minimum": 0, "multipleOf": 100}
  }
}
```

- 服务端在拿到 LLM 输出后**强制二次校验**：解析失败 / 字段缺失 / `target_quantity` 非 100 整数倍 / `action` 与持仓矛盾（如无持仓却 `Close`）→ 直接降级为 `Hold` 并记审计（不交回 LLM 重试，避免延迟放大与成本失控）。
- 风控 agent 的 `adjusted_quantity` 为软建议；最终 `target_quantity = min(交易员草案, 风控调整)`，且不得使净持仓为负（由 mock-order-engine 二次兜底）。

### 3.5 决策日志（审计 / 复盘）

每次 `Decide` 全程落库到 `decisions`（DDL 见 §5.5）：

| 字段 | 含义 |
|---|---|
| `id` | ULID |
| `signal_id` | 触发本次决策的 Signal（可空，规则兜底时为 NULL） |
| `symbol` / `exchange` | 标的 |
| `action` / `side` / `target_quantity` | 最终决策（`Decision` 字段，与 contracts §2.5 一致） |
| `rationale` | 交易员 agent 理由（审计） |
| `risk_checks` | 风控 agent 结果（JSONB 数组） |
| `analysis` | 分析师 agent 摘要（JSONB，含 score/bull/bear） |
| `pipeline_meta` | 3 次 LLM 调用的 usage/latency/finish_reason（成本追踪） |
| `source` | `llm` / `fallback_rule`（兜底来源标记） |
| `ts` | `DateTime<Utc>` |

> 决策日志为未来微调 / 复盘的核心数据，必须保留原始 LLM 输出（含被风控否决的草案）。

### 3.6 Q7 决策：入场 / 出场路径（→ ADR-019）

**结论（ADR-019）**：

| 动作 | 路径 | 执行者 | 理由 |
|---|---|---|---|
| **开仓（Open）/ 加仓（Add）** | 经 decision-agent（LLM） | `Decision` → mock-order-engine | 开仓是高风险、低频决策，值得 LLM 综合判断；延迟（数秒）可接受 |
| **减仓（Reduce）/ 平仓（Close）** | 经 decision-agent（LLM） | `Decision` → mock-order-engine | 主动止盈/调仓仍需 LLM 判断时机，频率低 |
| **硬止损（StopLoss）/ 硬止盈（TakeProfit）** | **规则直执**，不经 LLM | mock-order-engine 规则引擎 | 止损时效性要求高（秒级），LLM 延迟与不可用风险不可接受 |
| **T0 做T 的日内回买** | 经 decision-agent（LLM） | `Decision` → mock-order-engine | 回买时机是判断题，由 agent 拍板 |
| **打板次日卖出** | 经 decision-agent（LLM） | `Decision` → mock-order-engine | 是否兑现利润需判断 |

**理由**：开仓/调仓/止盈属"判断型"决策，LLM 价值高且频率低，延迟可接受；硬止损属"反应型"决策，**时效 > 智能**，必须在价格触及阈值时秒级触发，由 mock-order-engine 在事件循环里直接挂市价/限价单，**绝不等待 LLM 往返**。止损规则（按 `StrategyStyle` 差异化：波段 -5%、打板 跌破封板价等）在 mock-order-engine 配置，每个 Fill 后刷新持仓成本即重算止损价。

**契约体现**：mock-order-engine 既消费 `Decision`（agent 路径），也独立监听 `DataFeed` 的 Snapshot 事件触发止损（规则路径）。两条路径在订单层汇合，共用同一 `OrderService.SubmitOrder` 与 A 股规则引擎。

### 3.7 Q8 决策：LLM 兜底（→ ADR-020）

**结论（ADR-020）**：LLM 不可用时（超时 / 5xx / 限流 / 内容过滤拒绝 / JSON 解析失败重试耗尽）= **持仓保持 + 不开新仓**；**规则止损仍由 mock-order-engine 正常执行**（完全不依赖 LLM）。

**降级与恢复条件**：
- **降级触发**：单次 `Decide` 内任一 agent 步骤连续失败 ≥ 阈值（默认 2 次重试后仍失败）→ 该次 `Decide` 返回 `Hold`，`decisions.source = fallback_rule`。
- **降级行为**：
  1. 不发起新 `Open` / `Add`（保护资金）。
  2. 不主动 `Reduce` / `Close`（避免 LLM 缺位下乱平仓）。
  3. 已挂单不撤、已持仓不动（被动持仓）。
  4. mock-order-engine 的硬止损规则**照常运行**（ADR-019 已规定止损不经 LLM，故 LLM 挂掉不影响止损）。
- **恢复条件**：探针（每 N 秒发一次轻量 `complete` ping）连续 K 次成功 → 退出降级，恢复正常 LLM 决策。
- **理由**：安全 > 收益。LLM 挂掉时最坏情况是"错过机会"，而非"乱下单"；止损独立保证下行风险有界，系统可恢复。

---

## 4. tg-mock-order-engine 详细设计

### 4.1 ExecutionHandler trait 实现（ADR-009 落地）

Phase 2 `tg-engine` 定义（本 spec 引用其签名，不重新定义）：

```rust
// tg-engine crate（Phase 2 定义，本模块实现）
#[async_trait]
pub trait ExecutionHandler: Send + Sync {
    async fn submit(&self, intent: OrderIntent) -> Result<OrderId, TgError>;
    async fn cancel(&self, order_id: &OrderId) -> Result<(), TgError>;
    async fn snapshot_positions(&self) -> Result<Vec<Position>, TgError>;
    async fn snapshot_account(&self) -> Result<Account, TgError>;
    fn fill_channel(&self) -> tokio::sync::broadcast::Receiver<Fill>;
}

// OrderIntent 权威定义见 tg-contracts §2.3（策略/agent 产出，不含订单 ID）
```

`MockExecutionHandler` 实现该 trait：
- `submit`：构造 `Order`（生成 ULID）→ A 股规则引擎预校验（§4.5）→ 入虚拟账户（冻结资金/持仓）→ 撮合引擎（§4.3）→ Fill 落库 → 广播 Fill。
- `cancel`：撤未成交部分，解冻。
- `snapshot_positions` / `snapshot_account`：读虚拟账户内存状态（同时是落库前的一致性快照）。
- `fill_channel`：`tokio::sync::broadcast` 广播 Fill，供 monitoring-viz / signal-engine 复盘订阅。

> 回测（`tg-backtest`）链接的 `BacktestExecutionHandler` 与本 `MockExecutionHandler` 实现**同一 trait**，差异仅在 DataFeed（历史回放 vs 墙钟实时）——这是 ADR-006/009 的核心一致性保证。

### 4.2 DataFeed：实时快照消费

实现 `tg-engine::DataFeed` trait，`next_event()` 阻塞等待 market-data 写入 `latest_snapshots` 的轮询产物（链接 persistence 读，或经 gRPC 触发拉取）。事件循环每收到一个 `Snapshot` 事件：
1. 更新持仓 `last_price` / `market_value` / `unrealized_pnl`。
2. 触发挂单撮合（§4.3）。
3. 触发硬止损 / 硬止盈规则检查（ADR-019，规则直执）。
4. 触发打板封板检测（ADR-014）。

### 4.3 撮合算法（ADR-032：撮合假设）

**撮合假设（ADR-032）**：纸面模拟不追求微观真实（无队列位置/对手盘深度），采用**保守部分成交模型**——以五档盘口和成交量比例为输入，给出可解释的成交假设，宁可少成交不可虚增。具体规则：

| 场景 | 成交价 | 成交量 |
|---|---|---|
| **市价买** | `ask_price[0]`（卖一） | `min(订单量, ask_volume[0] × fill_ratio)`，`fill_ratio` 默认 0.3（保守，避免吃穿五档） |
| **市价卖** | `bid_price[0]`（买一） | `min(订单量, bid_volume[0] × fill_ratio)` |
| **限价买** | 订单价 ≥ `ask_price[0]`：以 `min(订单价, ask_price[0])` 成交；否则挂单等待 | 同上 fill_ratio |
| **限价卖** | 订单价 ≤ `bid_price[0]`：以 `max(订单价, bid_price[0])` 成交；否则挂单等待 | 同上 fill_ratio |
| **涨跌停（ADR-014 打板）** | 涨停价：买单仅当 `last == 涨停价 AND bid_volume[0] > N × ask_volume[0]`（封板）时排队部分成交；跌停对称 | 限制 |
| **滑点** | 成交价 ± `slippage_bps`（默认 2bps，可配，按品种/波动率） | — |
| **部分成交** | 单 tick 未吃完的订单量继续挂单，下一 tick 继续撮合（生命周期见 §4.9） | — |
| **集合竞价** | 集合竞价时段（§4.5 判定）不撮合，仅挂单，开盘/收盘集合竞价结束时按虚拟开盘价撮合 | — |

撮合引擎在 `MatchEngine::on_snapshot(snap, &mut orders)` 内对每个未终结订单尝试撮合，产出 0..N 个 `Fill`。

### 4.4 A 股规则引擎（前置校验 + 持续校验）

提交订单与每 tick 撮合前均过规则引擎，**任一硬规则 fail → `OrderStatus::Rejected` 并记 `rejection_reason`**：

| 规则 | 判定 | contracts 来源 |
|---|---|---|
| 最小交易单位 | `quantity % LOT_SIZE == 0 && quantity > 0` | `LOT_SIZE=100` |
| 涨跌停价 | `limit_up = pre_close × (1 + limit_up_pct(board))`，`limit_down = pre_close × (1 - limit_up_pct(board))`，订单价 ∈ [limit_down, limit_up]；价格按 A 股规则四舍五入到分 | `limit_up_pct(board)` |
| T+1 卖出（股票） | 卖出量 ≤ `available_quantity`（= total − t1_locked），见 §4.6 持仓分桶 | `Position.available_quantity` |
| T+0 卖出（跨境/货币/债券 ETF） | 同上但 t1_locked = 0（品种差异化 §4.7） | ADR-013/014 |
| 资金校验（买入） | `quantity × price × (1 + 佣金率) + 过户费 ≤ available_cash` | `Account` |
| 持仓校验（卖出） | `quantity ≤ available_quantity` | `Position` |
| 集合竞价限制 | 集合竞价时段仅收限价单，不撮合 | `is_call_auction(ts)` |
| ST / 退市风险 | `is_st == true` 且策略 `strategy_tag` 配置禁 ST → 拒绝 | `Instrument.is_st` |
| 黑名单 | symbol ∈ 配置黑名单 → 拒绝 | 风控配置 |

> 涨跌停价用 `rust_decimal::Decimal` 精确计算并 `round_dp(2)`；禁止 f64 中间值。

### 4.5 T0 持仓分桶算法（ADR-033）

**问题**：T+1 规则要求"今日买入不可卖，昨日持仓可卖"，T0 做T 需要日内卖出昨日持仓后回买。单一 `total_quantity` 无法表达可卖性，必须分桶。

**决策（ADR-033）**：持仓按"交易日分桶"存储，每桶记录 `{trading_date, quantity, avg_cost}`。`Position` 聚合视图（contracts §2.3）由分桶计算得出：

```rust
// 持仓桶（内存状态 + positions 表落库）
pub struct PositionLot {
    pub symbol: String,
    pub trading_date: NaiveDate,    // 该桶买入日（CST）
    pub quantity: i64,               // 该桶剩余股数（LOT_SIZE 倍数）
    pub avg_cost: Decimal,           // 该桶含费用均价
}

// 聚合到 contracts::Position：
//   total_quantity        = Σ lot.quantity
//   t1_locked_quantity    = Σ (lot where lot.trading_date == today).quantity
//   available_quantity    = total − t1_locked
//   avg_cost              = Σ(lot.quantity × lot.avg_cost) / total_quantity
```

**卖出 FIFO 规则**：卖出时**优先消耗最旧的桶**（保证 t1_locked 桶最后被卖），即按 `trading_date` 升序扣减。这天然满足 T+1（今日桶在最后，不可被卖）。

**T0 做T 时序示例**：
1. 昨日持 1000 股（桶：`D-1, 1000`）。
2. 今日开盘 agent 决策卖出 500 → 扣 `D-1` 桶：剩余 `D-1, 500`。现金 +。
3. 今日盘中 agent 决策回买 300 → 新桶：`today, 300`。现金 -。
4. 此时 `total=800`，`t1_locked=300`，`available=500`。今日不可再卖那 300 股，但可再卖 `D-1` 剩余 500。
5. 次日开盘 00:00 CST 翻日：`today` 桶自然变成 `D-1`，全部可卖。

**翻日（rollover）**：每日 00:00 CST（或交易日开盘前）调度任务将所有桶的 `trading_date` 视为历史，今日新建桶；`t1_locked` 自动清零。落库时 `positions` 表更新每桶 `quantity`。

**约束**：净持仓永不为负（卖出量 ≤ available 时已被规则引擎拦截）。

### 4.6 打板执行（ADR-014）

打板策略（`StrategyStyle::LimitUp`）执行规则：
- **封板检测**：当 `Snapshot.last == limit_up_price` 且 `bid_volume[0] > K × ask_volume[0]`（K 默认 5，封单远大于卖单）→ 判定封板。
- **尾盘限价单**：14:55 CST 后若仍封板，agent 决策买入则以涨停价挂限价单（撮合按 §4.3 涨停规则部分成交）。
- **次日卖出**：次日开盘若未一字板，agent 决策卖出兑现；跌破封板价触发硬止损（规则直执，ADR-019）。

打板相关订单 `strategy_tag = LimitUp`，便于事后统计胜率与盈亏比。

### 4.7 品种差异化（ADR-013）

按 `InstrumentType` 应用不同规则集：

| 规则 | 股票 | ETF |
|---|---|---|
| 印花税（卖出 0.05%） | 征收 | **免征** |
| T+1 | 是 | 是（普通 A 股 ETF） |
| T+0 | 否 | **跨境/货币/债券 ETF 支持**（按 instrument 元数据 `supports_t0` 标记，t1_locked=0） |
| 过户费（沪市） | 征收 | 征收 |
| 涨跌停 | 主板±10%/科创创业±20%/北交±30% | 同（ETF 按其底层板块） |

`supports_t0` 标记来自 `Instrument` 扩展（Phase 0 instruments 表预留位，本 Phase 在元数据同步时填充）。规则引擎在 T+1 校验时按此标记切换。

### 4.8 虚拟账户状态机

```rust
pub struct VirtualAccount {
    pub cash: Decimal,                // 可用现金
    pub frozen_cash: Decimal,         // 买单冻结（订单未成交）
    pub lots: HashMap<String, Vec<PositionLot>>,  // symbol -> 桶
    // 派生（contracts::Account 视图）：
    //   total_value     = cash + Σ positions.market_value
    //   positions       = aggregate(lots)
}
```

状态转移（每个事件原子，受 PG 事务保护）：

| 事件 | 现金 | 冻结 | 持仓 |
|---|---|---|---|
| 买单提交（限价） | 不变 | `+qty×price×(1+佣金率)+过户费` | 不变 |
| 买单部分/全部成交 | `−qty×fill_price×(1+佣金率)+过户费` | `−qty×fill_price×(1+佣金率)`（解冻对应部分） | 新增桶（today） |
| 买单撤单 | 不变 | `−qty×price×(1+佣金率)`（解冻剩余） | 不变 |
| 卖单提交 | 不变（卖出不冻结现金） | 不变 | 不变（撮合时扣桶） |
| 卖单成交 | `+qty×fill_price×(1−佣金率−印花税)−过户费` | 不变 | FIFO 扣桶 |
| 翻日（rollover） | 不变 | 不变 | 桶 trading_date 历史化，t1_locked→0 |

### 4.9 成本模型（ADR-031 精度策略）

决策（ADR-031 区间决策）：**成本全程 `Decimal`，税率/费率为编译期 `Decimal` 常量，禁止 f64 中间值**。

```
买入成本  = 成交价 × 数量
买入费用  = 佣金(max(成交金额 × 佣金率, 5元最低)) + 过户费(沪市: 成交金额 × 0.001%)
卖出费用  = 佣金(同上) + 印花税(成交金额 × 0.05%) + 过户费(沪市)
  注: ETF 卖出免印花税
净成交金额(买) = −(成交金额 + 买入费用)
净成交金额(卖) = +(成交金额 − 卖出费用)
```

- 常量来源 `tg-contracts`：`STAMP_DUTY_PCT = 0.0005`、`COMMISSION_MAX_PCT = 0.0003`、`TRANSFER_FEE_PCT = 0.00001`。
- 佣金率 / 最低佣金 / 滑点 bps / fill_ratio 在 `mock-order-engine.toml` 可配（不同券商费率不同）。
- 成本对拍单元测试：用已知费率手算期望值，对比引擎产出的 `Fill.commission/tax/transfer_fee`（§8）。

### 4.10 软风控（无真实风险但保留逻辑）

mock-order-engine 内置软风控（与 decision-agent 风控 agent 形成双重把关）：
- 单标的仓位上限（占 total_value 比例，默认 20%）。
- 总仓位上限（默认 80%，留 20% 现金）。
- 单标的硬止损（按 `StrategyStyle` 差异化，规则直执 ADR-019）。
- 黑名单（symbol 列表）。
- 软风控命中 → 拒单 `OrderStatus::Rejected`，记 `rejection_reason = "risk: single_position_cap"` 等。

> 软风控不替代硬 A 股规则（§4.4），是策略层风险约束。规则引擎与软风控共享 `RiskCheckResult` 类型（contracts §2.5）。

### 4.11 订单生命周期

```
New ──撮合──▶ PartiallyFilled ──继续──▶ Filled
 │                                  ▲
 │                                  │
 └──cancel──▶ Cancelled             │
 │                                  │
 └──reject(规则/风控)──▶ Rejected    │
                                       │
 (任何 PartiallyFilled 都已部分成交，剩余量可 cancel→Cancelled) 
```

每个状态转移落 `orders.status` 并广播事件；`filled_quantity` / `avg_fill_price` 在 `Order` 上累加更新（contracts §2.3）。

---

## 5. PostgreSQL Schema（经 tg-persistence）

> 全部落 `tg-persistence` crate（ADR-017），sqlx migrate 版本化。价格 `NUMERIC(18,4)`，时间 `TIMESTAMPTZ`，交易日 `DATE`（CST），ID 为 ULID 字符串。枚举存 TEXT（与 contracts 枚举名一致），应用层校验。

### 5.1 orders

```sql
CREATE TABLE orders (
    id               VARCHAR(26) PRIMARY KEY,            -- ULID
    client_order_id  TEXT NOT NULL,                       -- 幂等键
    symbol           VARCHAR(10) NOT NULL,
    exchange         TEXT NOT NULL,                       -- SH/SZ/BJ
    side             TEXT NOT NULL,                       -- Buy/Sell
    order_type       TEXT NOT NULL,                       -- Limit/Market
    price            NUMERIC(18,4),                       -- 限价必填；市价 NULL
    quantity         BIGINT NOT NULL,                     -- LOT_SIZE 倍数
    time_in_force    TEXT NOT NULL DEFAULT 'Day',
    strategy_tag     TEXT NOT NULL,                       -- Swing/T0/LimitUp
    source           TEXT NOT NULL,                       -- agent / rule_stoploss / rule_takeprofit
    status           TEXT NOT NULL,                       -- New/PartiallyFilled/Filled/Cancelled/Rejected
    filled_quantity  BIGINT NOT NULL DEFAULT 0,
    avg_fill_price   NUMERIC(18,4) NOT NULL DEFAULT 0,
    rejection_reason TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (client_order_id)
);
CREATE INDEX idx_orders_symbol_ts    ON orders (symbol, created_at);
CREATE INDEX idx_orders_status       ON orders (status) WHERE status IN ('New','PartiallyFilled');
CREATE INDEX idx_orders_strategy     ON orders (strategy_tag, created_at);
```

### 5.2 fills

```sql
CREATE TABLE fills (
    fill_id          VARCHAR(26) PRIMARY KEY,             -- ULID
    order_id         VARCHAR(26) NOT NULL REFERENCES orders(id),
    symbol           VARCHAR(10) NOT NULL,
    exchange         TEXT NOT NULL,
    side             TEXT NOT NULL,
    price            NUMERIC(18,4) NOT NULL,
    quantity         BIGINT NOT NULL,
    commission       NUMERIC(18,4) NOT NULL,
    tax              NUMERIC(18,4) NOT NULL,              -- 印花税（ETF=0）
    transfer_fee     NUMERIC(18,4) NOT NULL,              -- 过户费（深市=0）
    ts               TIMESTAMPTZ NOT NULL DEFAULT now(),
    trading_date     DATE NOT NULL                        -- CST 交易日
);
CREATE INDEX idx_fills_order      ON fills (order_id);
CREATE INDEX idx_fills_symbol_ts  ON fills (symbol, ts);
CREATE INDEX idx_fills_tdate      ON fills (trading_date);
```

### 5.3 positions（持仓桶，T0 分桶落库，ADR-033）

```sql
CREATE TABLE positions (
    symbol           VARCHAR(10) NOT NULL,
    trading_date     DATE NOT NULL,                       -- 该桶买入日（CST）
    quantity         BIGINT NOT NULL,                     -- 该桶剩余股数
    avg_cost         NUMERIC(18,4) NOT NULL,              -- 含费用均价
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (symbol, trading_date)
);
CREATE INDEX idx_positions_symbol ON positions (symbol);
```

> 应用层按 §4.5 聚合为 `contracts::Position`（total / t1_locked / available / avg_cost）。`trading_date == today` 的桶构成 t1_locked。

### 5.4 accounts（虚拟账户快照）

```sql
CREATE TABLE accounts (
    snapshot_id      BIGSERIAL PRIMARY KEY,
    ts               TIMESTAMPTZ NOT NULL DEFAULT now(),
    trading_date     DATE NOT NULL,
    cash             NUMERIC(18,4) NOT NULL,
    frozen_cash      NUMERIC(18,4) NOT NULL,
    total_value      NUMERIC(18,4) NOT NULL,              -- cash + Σ 持仓市值
    unrealized_pnl   NUMERIC(18,4) NOT NULL
);
CREATE INDEX idx_accounts_tdate ON accounts (trading_date, ts);
```

> 每个 Fill 后写一行快照（或按 tick 批量），供净值曲线复盘。

### 5.5 decisions（决策日志，§3.5）

```sql
CREATE TABLE decisions (
    id               VARCHAR(26) PRIMARY KEY,             -- ULID
    signal_id        VARCHAR(26),                         -- 触发决策的 Signal（可空）
    symbol           VARCHAR(10) NOT NULL,
    exchange         TEXT NOT NULL,
    action           TEXT NOT NULL,                       -- Open/Add/Reduce/Close/Hold
    side             TEXT NOT NULL,                       -- Buy/Sell
    target_quantity  BIGINT NOT NULL,
    rationale        TEXT NOT NULL,
    risk_checks      JSONB NOT NULL DEFAULT '[]',         -- RiskCheckResult[]
    analysis         JSONB,                               -- 分析师 agent 摘要
    pipeline_meta    JSONB,                               -- LLM usage/latency/finish_reason
    source           TEXT NOT NULL,                       -- llm / fallback_rule
    ts               TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_decisions_symbol_ts ON decisions (symbol, ts);
CREATE INDEX idx_decisions_source   ON decisions (source, ts);
```

---

## 6. 接口定义（proto，contracts §3 对齐）

### 6.1 DecisionService（tg-decision-agent）

```protobuf
service DecisionService {
  // 同步决策：传入 Signal + 上下文 hint，返回最终 Decision。
  rpc Decide(DecideRequest) returns (DecideResponse);

  // 流式订阅：每当 decision-agent 产出 Decision（含 Decide 调用与规则兜底）即推送。
  rpc SubscribeDecisions(SubscribeDecisionsRequest) returns (stream DecisionEvent);

  // 决策日志查询（复盘用）。
  rpc QueryDecisions(QueryDecisionsRequest) returns (QueryDecisionsResponse);
}

message DecideRequest {
  string signal_id = 1;                 // 触发决策的 Signal（可空则纯规则）
  string symbol = 2;
  string exchange = 3;
  tg.StrategyStyle style_hint = 4;      // 风格提示，影响仓位约束
  bool force_llm = 5;                   // 强制走 LLM（测试用，跳过降级缓存）
}

message DecideResponse {
  tg.Decision decision = 1;
  string source = 2;                    // llm / fallback_rule
  LlmPipelineMeta meta = 3;             // 3 次 LLM 调用统计
}

message DecisionEvent {
  tg.Decision decision = 1;
  string source = 2;
}

message QueryDecisionsRequest {
  string symbol = 1;
  google.protobuf.Timestamp from_ts = 2;
  google.protobuf.Timestamp to_ts = 3;
  uint32 limit = 4;
}
message QueryDecisionsResponse { repeated tg.Decision decisions = 1; }

message LlmPipelineMeta {
  uint32 total_calls = 1;
  uint64 prompt_tokens = 2;
  uint64 completion_tokens = 3;
  uint32 latency_ms_total = 4;
  repeated string finish_reasons = 5;   // 每步的 finish_reason
}
```

### 6.2 OrderService（tg-mock-order-engine）

```protobuf
service OrderService {
  // 提交订单（agent 路径与规则止损路径共用）。
  rpc SubmitOrder(SubmitOrderRequest) returns (SubmitOrderResponse);
  rpc CancelOrder(CancelOrderRequest) returns (CancelOrderResponse);
  rpc GetOrder(GetOrderRequest) returns (GetOrderResponse);
  rpc QueryPositions(QueryPositionsRequest) returns (QueryPositionsResponse);
  rpc QueryAccount(QueryAccountRequest) returns (QueryAccountResponse);

  // 流式订阅 Fill 事件（monitoring-viz / signal-engine 复盘）。
  rpc SubscribeFills(SubscribeFillsRequest) returns (stream tg.Fill);
}

message SubmitOrderRequest {
  string client_order_id = 1;           // 幂等键
  string symbol = 2;
  string exchange = 3;
  tg.OrderSide side = 4;
  tg.OrderType order_type = 5;
  string price = 6;                     // Decimal 字符串；市价空
  int64 quantity = 7;
  tg.TimeInForce time_in_force = 8;
  tg.StrategyStyle strategy_tag = 9;
  string source = 10;                   // agent / rule_stoploss / rule_takeprofit
}
message SubmitOrderResponse {
  tg.Order order = 1;                   // 含 ULID id 与初始 status
}
message CancelOrderRequest  { string order_id = 1; }
message CancelOrderResponse { tg.Order order = 1; }
message GetOrderRequest     { string order_id = 1; }
message GetOrderResponse    { tg.Order order = 1; }
message QueryPositionsRequest  { string symbol = 1; }   // 空则返回全部
message QueryPositionsResponse { repeated tg.Position positions = 1; }
message QueryAccountRequest    {}
message QueryAccountResponse   { tg.Account account = 1; }
```

> proto 字段名 snake_case，message PascalCase，与 contracts §5 编码规范一致。`tg.*` 引用 `tg-contracts` 已定义的领域类型（Decision / Order / Fill / Position / Account / 枚举）。

---

## 7. 错误处理与可观测性

### 7.1 错误分层
- **decision-agent**：LLM 超时/5xx/限流 → ADR-020 兜底（Hold，不阻断）。JSON 解析失败 → 强制 Hold + 记审计（不回 LLM）。下游 gRPC（signal/factor/indicators）不可用 → 该 `Decide` 走 `fallback_rule`。
- **mock-order-engine**：规则/风控拒绝 → `OrderStatus::Rejected` + `rejection_reason`（不抛异常，正常返回）。资金/持仓不足 → Rejected（细节记 `rejection_reason`）。落库失败 → 重试 + 告警，内存状态以 PG 为准重启时重放。
- **跨边界**：tonic `Status` code 映射 contracts §4 `TgError`（`InvalidArgument`→`Validation`、`NotFound`→`NotFound`、`ResourceExhausted`→`RateLimited`、`FailedPrecondition`→`InvalidOrder`/`RiskRejected`）。

### 7.2 可观测性
- **结构化日志**（tracing）：每次 `Decide` 带 `decision_id/signal_id/symbol/source/latency`；每次 `submit/cancel/fill` 带 `order_id/symbol/quantity/price`。
- **Prometheus 指标**（本 Phase 预留 + 落地关键项）：
  - `tg_decision_llm_calls_total{provider,model,finish_reason}` / `tg_decision_llm_latency_ms` / `tg_decision_fallback_total`（降级计数，ADR-020 监控核心）。
  - `tg_order_submitted_total{strategy_tag,source,side}` / `tg_order_rejected_total{reason}` / `tg_fill_total`。
  - `tg_account_total_value`（gauge，净值曲线源）。
- **健康检查**：`/health` 反映 LLM provider 连通性（探针）+ DB 连通性 + 是否处降级态。
- **审计**：`decisions` 表是系统级审计事实源，永不删（仅归档）；每条含原始 LLM 输出与被否决的草案。

---

## 8. 测试策略

### 8.1 单元测试（确定性，无网络）
- **撮合算法**（§4.3）：构造固定 `Snapshot`（含五档）+ `Order`，断言成交价/量/滑点；覆盖涨跌停、限价未触、部分成交、集合竞价不撮合。
- **A 股规则引擎**（§4.4）：涨跌停价计算（主板/科创/北交）、T+1 卖出拦截、最小 100 股、ST 拒绝、资金不足拒绝、集合竞价限单。
- **T0 持仓分桶**（§4.5）：买入建桶、FIFO 卖出扣桶、T+1 锁定判定、翻日后 t1_locked 清零、净持仓不为负。
- **成本模型对拍**（§4.9）：已知费率手算股票买卖成本（含印花税/过户费）与 ETF 买卖成本（免印花税），对比 `Fill` 字段。
- **LLM 输出解析**：喂固定 LLM 文本 fixture（合法 JSON / 缺字段 / 非 100 倍数 / action 矛盾），断言降级行为。
- **降级状态机**（ADR-020）：模拟 LLM 连续失败，断言 `fallback_rule` 触发；模拟探针恢复，断言退出降级。

### 8.2 集成测试（mock LLM + mock DataFeed）
- **mock LLM**：实现 `LlmClient` trait 的 fake，按脚本返回三步 agent 的 JSON（分析师/交易员/风控），驱动完整 `Decide` pipeline，断言产出 `Decision` 落 `decisions` 表。
- **mock DataFeed**：回放固定 `Snapshot` 序列（含一次涨停封板 + 一次跌破止损价），驱动 mock-order-engine 完整事件循环，断言：开仓经 agent、止损规则直执（ADR-019）、T0 日内回买、ETF 免印花税、订单生命周期状态转移、虚拟账户现金流。
- **端到端**：mock signal-engine 推一个 Signal → decision-agent.Decide → mock-order-engine.SubmitOrder → 撮合 Fill → 落库 → 账户快照更新，全链路断言。

### 8.3 冒烟测试（`#[ignore]`，本地/CI 可选）
- 真实 LLM provider（任一 OpenAI 兼容）跑一次完整 `Decide`，验证 prompt 组装与 JSON schema 解析（标记 `#[ignore]`，需配置真实 `base_url+api_key`，不进默认 CI）。
- 真实 akshare 快照（Phase 0 冒烟）拉一只标的 1 分钟，驱动 mock-order-engine 撮合一单，验证端到端连通。

---

## 9. 验收标准（Definition of Done）

1. `tg-decision-agent` 实现 `DecisionService`（Decide / SubscribeDecisions / QueryDecisions），多 agent pipeline 三步顺序执行，每步 LLM 调用带超时与重试。
2. LLM 多 provider 抽象层（`OpenAICompatibleClient`）可经配置切换 deepseek/qwen/glm/moonshot/OpenAI，仅 `base_url+api_key+model` 差异（ADR-015/030）。
3. JSON Schema 结构化输出落地（三 agent 各一 schema），服务端强制二次校验，非法输出降级 Hold 并记审计。
4. 决策全程日志落 `decisions` 表，含原始分析 / 风控结果 / LLM usage / source 标记。
5. Q7 决策落地（ADR-019）：开仓/加仓/减仓/平仓经 agent；硬止损/硬止盈由 mock-order-engine 规则直执，秒级触发不等待 LLM。
6. Q8 决策落地（ADR-020）：LLM 不可用时持仓保持 + 不开新仓（`fallback_rule`），止损规则照常；探针恢复后退出降级。
7. `tg-mock-order-engine` 实现 `tg-engine::ExecutionHandler` trait（`submit/cancel/snapshot_positions/snapshot_account/fill_channel`），与回测执行器同 trait。
8. A 股规则引擎完整：涨跌停（按板块）/ T+1 / 最小 100 股 / 集合竞价 / 资金持仓校验 / ST / 黑名单。
9. T0 持仓分桶（ADR-033）正确：昨日持仓可卖、今日买入锁定、FIFO 扣桶、净持仓不为负、翻日清零 t1_locked。
10. 打板执行（ADR-014）：封板检测、尾盘限价单、次日卖出、跌破封板价止损。
11. 品种差异化（ADR-013）：ETF 免印花税、跨境/货币/债券 ETF 支持 T+0（元数据 `supports_t0` 标记）。
12. 成本模型全程 `Decimal`，印花税/过户费/佣金按 contracts 常量计算，对拍单元测试通过。
13. `orders` / `fills` / `positions` / `accounts` / `decisions` 五表落库，sqlx migrate 版本化，价格 `NUMERIC(18,4)`。
14. `OrderService` 完整（SubmitOrder/CancelOrder/GetOrder/QueryPositions/QueryAccount/SubscribeFills），订单生命周期状态机正确。
15. 单元（撮合/T+1/T0/成本对拍/LLM 解析/降级）+ 集成（mock LLM + mock DataFeed 端到端）测试全绿；冒烟测试 `#[ignore]` 可手动跑通。

---

## 10. 依赖的 ADR

### 已有（来自上游架构文档）
- **ADR-006** 回测/模拟共用引擎（mock-order-engine 链接 tg-engine library）。
- **ADR-009** mock/实盘切换点（实现 `ExecutionHandler` trait，未来 broker-gateway 同接口）。
- **ADR-010** 决策权归 decision-agent（signal 仅候选，agent 拍板）。
- **ADR-013** 标的品种：A 股股票 + ETF，ETF 规则差异化。
- **ADR-014** 策略风格：波段/T0/打板，mock-order-engine 支持 T+0 与打板执行。
- **ADR-015** LLM 多 provider 抽象（OpenAI 兼容协议为主）。
- **ADR-017** persistence 共享库 crate（本 Phase 链接读写）。

### 本期新增
- **ADR-019 入场/出场路径（Q7 决策）**：开仓/加仓/减仓/平仓经 decision-agent（LLM，判断型，延迟可接受）；硬止损/硬止盈由 mock-order-engine 规则直执（反应型，秒级，不等待 LLM）。两条路径在订单层汇合共用 `OrderService`。
- **ADR-020 LLM 兜底（Q8 决策）**：LLM 不可用 = 持仓保持 + 不开新仓（`source=fallback_rule`），止损规则照常运行；探针连续成功后恢复。安全 > 收益，下行风险有界可恢复。
- **ADR-030 LLM provider 抽象接口形态**：单一 trait `LlmClient::complete(LlmRequest)→LlmResponse`，单一 `OpenAICompatibleClient` 实现覆盖 5 家 provider，差异仅在 `base_url/api_key/model/header`。不为此抽象多 trait，YAGNI。
- **ADR-031 结构化输出约束方式 + 成本精度策略**：①用 JSON Schema 约束 LLM 输出（OpenAI `response_format=json_schema`，不支持的 provider 退化 prompt 内嵌 + 服务端严格解析），服务端二次校验非法即降级 Hold；②成本全程 `rust_decimal::Decimal`，税率/费率为编译期常量，禁 f64 中间值，对拍单元测试守护。
- **ADR-032 撮合假设**：纸面模拟采用保守部分成交模型（`fill_ratio` 默认 0.3 + `slippage_bps` 默认 2bps，可配），以五档盘口为输入，宁可少成交不可虚增；不模拟队列位置/对手盘深度（YAGNI，纸面阶段）。
- **ADR-033 T0 持仓分桶算法**：持仓按交易日分桶（`{trading_date, quantity, avg_cost}`），卖出 FIFO 扣最旧桶（天然满足 T+1），翻日历史化使 t1_locked 清零；聚合到 contracts `Position` 视图（total/t1_locked/available/avg_cost）。

---

## 11. 后续 / 延期项

- **实盘切换路径**：未来 `tg-broker-gateway` 实现同一 `ExecutionHandler` trait（ADR-009），切换注入对象即从纸面升级实盘，业务逻辑（agent pipeline / 策略 / 风控）不动。实盘需额外补：真实队列位置/对手盘、券商专属错误码、断网重连、真实 T+1 资金清算延迟。
- **LLM 工具调用**：decision-agent 经 MCP/function calling 调 indicators 实时算指标（ADR-015 提及），Phase 3.5 视必要再加；当前用 factor-engine 缓存值。
- **决策微调**：积累 `decisions` 表后，离线分析 LLM 决策与实际盈亏的相关性，调 prompt / 风控阈值（本期只采集，不在线学习）。
- **多策略组合优化**：单虚拟账户多 `strategy_tag` 溯源（本期），未来可演进多账户隔离 + 组合层风控。
- **撮合精细化**：纸面 `fill_ratio`/`slippage_bps` 模型够用即可；若回测与模拟出现显著偏差，再考虑 Level-2 队列模型（ADR-032 延期）。

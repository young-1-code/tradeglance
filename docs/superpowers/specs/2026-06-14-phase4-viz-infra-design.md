# Phase 4 可视化与编排 — 详细设计 Spec

> **子项目**：`tg-monitoring-viz` + `tg-infra`
> **状态**：设计完成，待用户评审
> **日期**：2026-06-14
> **上游文档**：`2026-06-14-tradeglance-architecture-design.md`（v0.6）
> **契约来源**：`2026-06-14-tg-contracts-design.md`（权威类型，本文不重复定义）
> **相关 ADR**：ADR-003 / 007 / 011 / 017 / 018（既有）+ ADR-034 / 035 / 036 / 037（新增）

---

## 1. 概述与范围

### 1.1 目标
Phase 4 是 TradeGlance 的收口阶段，交付两件事：
1. **可视化后台**（`tg-monitoring-viz`）：把 Phase 0~3 产出的行情、因子、信号、决策、订单、持仓、账户、绩效数据通过 Web 界面呈现，让用户能"看盘 + 看决策 + 看绩效"。后端 Rust/Axum 提供 REST API 与实时推送，前端 TypeScript 渲染图表。
2. **部署编排**（`tg-infra`）：用一份 docker-compose 把所有服务（market-data / indicators / factor-engine / backtest / signal-engine / decision-agent / mock-order-engine / monitoring-viz）+ Python sidecar + Postgres + Parquet 卷 + 可观测性栈编排起来，开箱即跑；并承担配置、定时调度、健康检查、密钥管理。

Phase 4 结束后系统可演示：`docker compose up` → 全服务健康 → Web 访问仪表盘 → 实时看到信号/决策/订单流 → 回测绩效可查。

### 1.2 In Scope
- `tg-monitoring-viz` 后端（Axum）：REST 端点清单（持仓/账户/订单/绩效/信号/决策/行情/因子/健康）+ 实时推送通道（SSE，ADR-035）
- `tg-monitoring-viz` 前端（React + Vite + TypeScript，ADR-034）：五类视图（仪表盘 / 绩效 / 信号决策 / 行情 / 因子）
- 前端数据来源：**只走后端 REST/SSE，不直连数据库**（硬约束）
- `tg-infra` docker-compose：全服务编排 + 依赖拓扑 + 网络/卷/端口
- 配置管理方案：分层配置（compose env / 各服务 config 文件 / 密钥）
- 定时调度：cron 清单（收盘后采日K、每日因子计算、定时回测）
- 可观测性栈：结构化日志聚合 + Prometheus 指标 + Grafana 看板
- 健康检查：各服务 liveness/readiness，对接 `/health`
- 密钥管理：数据源 token、LLM API key（env / docker secrets，ADR-036）

### 1.3 Out of Scope（YAGNI / 延期）
- 告警/通知（邮件/IM 推送）—— 延期（§10）
- 分布式链路追踪（OpenTelemetry）—— 延期（§10）
- 多用户、登录、RBAC、审计日志 —— 单用户个人系统，延期（§10）
- 前端移动端 / 原生 App —— 仅 Web
- 实盘券商网关 —— 属未来 Phase 5（`tg-broker-gateway`）
- Kubernetes 编排 —— 个人容器化以 docker-compose 为准（ADR-003），不上 K8s
- 参数寻优可视化 —— Phase 2 回测功能延伸，按需

---

## 2. 模块形态与依赖

### 2.1 模块形态表

| 模块 | 形态 | 语言 | 角色 |
|---|---|---|---|
| `tg-monitoring-viz` | 后端服务 + 前端 SPA（同 repo，置于 `apps/tg-monitoring-viz`） | Rust (Axum) + TypeScript (React) | **只读消费者**：从 persistence 读 + 订阅事件；无交易逻辑 |
| `tg-infra` | 编排产物（置于 `apps/tg-infra`）：docker-compose + 配置模板 + 调度 sidecar + 监控栈 | YAML + Shell + 少量 Python | **编排者**：把所有服务跑起来 + 共享基础设施 |

### 2.2 依赖关系图

```
                         ┌──────────────────────────┐
                         │      tg-infra (compose)   │
                         │  编排全部 + Postgres + 卷  │
                         │  + Prometheus + Grafana   │
                         │  + 调度 sidecar           │
                         └───────────┬──────────────┘
                                     │ 编排/拉起
        ┌────────────────────────────┼────────────────────────────┐
        ▼                            ▼                            ▼
┌────────────────┐         ┌──────────────────┐         ┌──────────────────┐
│ tg-monitoring- │         │  全部业务服务     │         │  共享基础设施     │
 │     viz        │         │ market-data/...   │         │ Postgres         │
│  ┌──────────┐  │         │ mock-order-engine │         │ Parquet 卷       │
│  │ 后端Axum │  │         │ signal-engine     │         │ collector-python │
│  └────┬─────┘  │         │ decision-agent ...│         │ prometheus       │
│       │        │         └─────────┬─────────┘         │ grafana          │
│  ┌────▼─────┐  │                   │                   │ loki (日志)      │
│  │ 前端 SPA │  │                   │                   └──────────────────┘
│  └──────────┘  │                   │
└───────┬────────┘                   │
        │                            │
        │  链接 tg-persistence（只读） │ 事件总线（SSE 适配）
        ▼                            ▼
┌─────────────────────────────────────────────┐
│           tg-persistence (共享库 crate)       │
│   Postgres + Parquet/DuckDB（ADR-007/017）    │
└─────────────────────────────────────────────┘
```

### 2.3 关键依赖说明
- **monitoring-viz 后端**链接 `tg-persistence` crate 以只读方式读取行情/订单/持仓/绩效（ADR-017），同时链接 `tg-contracts` 复用 `Bar/Snapshot/Order/Fill/Position/Account/Signal/Decision/FactorEvaluation` 等权威类型——**禁止重复定义**。
- **实时数据**：monitoring-viz 不直接订阅信号/决策/订单引擎的内部事件流，而是经一个轻量事件总线（内存广播或 Redis pub/sub，本期采用 SSE 适配器轮询 + Postgres 事件表，见 ADR-035）拿到增量，避免与业务服务进程耦合。
- **前端**只与后端 REST/SSE 通信，**不直连 Postgres / Parquet**（硬约束）。
- **tg-infra** 本身不含业务代码，是纯编排与配置产物；它是唯一"知道全部服务清单"的模块。

---

## 3. tg-monitoring-viz 详细设计

### 3.1 后端（Axum）

#### 3.1.1 架构分层
```
apps/tg-monitoring-viz/
├── backend/                     # Rust Axum 后端
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs              # 启动 axum + 注入 persistence repo
│   │   ├── routes/              # REST 路由（按资源分文件）
│   │   ├── sse/                 # SSE 实时推送
│   │   ├── dto/                 # 出参 DTO（从 contracts 类型映射）
│   │   └── error.rs             # ApiError → HTTP status
│   └── tests/                   # 集成测试（persistence fixture）
└── frontend/                    # TypeScript React SPA
    ├── package.json
    └── src/ ...
```

#### 3.1.2 数据来源
- **读路径**：链接 `tg-persistence` crate，直接调 `BarRepo / SnapshotRepo / OrderRepo / PositionRepo / AccountRepo / SignalRepo / DecisionRepo / FactorRepo / BacktestRepo`（Phase 0~3 已定义）。
- **实时推送路径**：SSE 端点（§3.1.4）。后端维护一个"事件轮询器"周期性查 Postgres 中的 `signals / decisions / orders / fills` 表的最新行（按 `created_at` 增量游标），变更推给订阅的 SSE 客户端。这避免引入消息总线（ADR-002 YAGNI），又满足 Web 实时性需求（ADR-035）。

#### 3.1.3 REST 端点清单（MonitoringApi，contracts §3）

> 命名遵循 contracts spec §3 的 `MonitoringApi`（REST 为主）。返回类型引用 contracts 权威类型，DTO 只做序列化适配。

| 方法 | 路径 | 含义 | 返回（contracts 类型） |
|---|---|---|---|
| GET | `/health` | 健康检查（DB 连通性 + 子系统状态） | `{ status, db, deps }` |
| GET | `/api/v1/account` | 当前账户总览 | `Account` |
| GET | `/api/v1/positions` | 实时持仓列表（含浮盈） | `Vec<Position>` |
| GET | `/api/v1/positions/:symbol` | 单标的持仓详情 | `Position` |
| GET | `/api/v1/orders?date=&status=&symbol=` | 订单查询（默认今日） | `Vec<Order>` |
| GET | `/api/v1/orders/:id` | 订单详情（含 fills） | `Order + Vec<Fill>` |
| GET | `/api/v1/fills?date=&symbol=` | 成交流水 | `Vec<Fill>` |
| GET | `/api/v1/equity-curve?from=&to=&mode=backtest\|live` | 账户净值曲线（按日） | `{ ts[], equity[], benchmark[] }` |
| GET | `/api/v1/performance/:run_id` | 绩效统计（单次回测/模拟） | `PerformanceReport`（收益/夏普/回撤/胜率/盈亏比） |
| GET | `/api/v1/performance` | 绩效报告列表（对比多次） | `Vec<{ run_id, config, summary }>` |
| GET | `/api/v1/signals?from=&to=&symbol=&style=` | 信号历史 | `Vec<Signal>` |
| GET | `/api/v1/signals/:id` | 信号详情（含 reason） | `Signal` |
| GET | `/api/v1/decisions?from=&to=&symbol=` | 决策历史（含 rationale / risk_checks） | `Vec<Decision>` |
| GET | `/api/v1/decisions/:id` | 决策详情 | `Decision` |
| GET | `/api/v1/bars?symbol=&period=&from=&to=&adjustment=` | K 线（直接走 persistence `BarRepo`） | `Vec<Bar>` |
| GET | `/api/v1/snapshot/:symbol` | 最新实时快照 | `Snapshot` |
| GET | `/api/v1/indicators?symbol=&period=&indicator=&params=` | 指标序列（代理调 indicators 服务或后端复算） | `IndicatorResult` |
| GET | `/api/v1/factors/evaluation?factor=&from=&to=` | 因子评估（IC/IR/分层） | `FactorEvaluation` |
| GET | `/api/v1/factors/values?factor=&date=` | 横截面因子值（含 rank） | `Vec<FactorValue>` |
| GET | `/api/v1/watchlist` | 自选股池（透传 persistence） | `{ items: [{ symbol, strategy_tags }] }` |

约定：
- 时间参数统一 ISO8601 UTC；分页用 `?limit=&cursor=`（游标为 `created_at + id`）。
- 价格字段返回字符串（保 Decimal 精度，前端再 parse）。
- 错误统一 JSON：`{ "code": "NOT_FOUND", "message": "..." }`，HTTP 状态码与 contracts §4 的 `TgError` 映射（NotFound→404，Validation→400，Upstream→502，RateLimited→429）。

#### 3.1.4 实时推送方式：SSE（ADR-035）
提供聚合 SSE 端点：
```
GET /api/v1/events/stream?topics=signals,decisions,orders,fills,account
Content-Type: text/event-stream
```
- 每个事件以 SSE 帧下发：`event: signal\ndata: { ...Signal json... }\n\n`。
- 后端维护一个共享的"事件多路复用器"（tokio broadcast channel），轮询器（默认 2s 间隔）从 persistence 增量拉新行，按 topic 广播；客户端用 `Last-Event-ID` 头做断线续传（游标=行 id）。
- 客户端可订阅子集 topics，降低无用推送。
- 选 SSE 而非 WebSocket 的理由：推送是单向（服务端→前端），SSE 自带断线重连与 `Last-Event-ID`、走 HTTP/2 更省、Axum 实现极简，符合 YAGNI（ADR-035）。

### 3.2 前端（React + TypeScript，ADR-034）

#### 3.2.1 技术栈
| 维度 | 选型 | 理由 |
|---|---|---|
| 框架 | React 18 + TypeScript | 生态最大、图表库适配最广 |
| 构建 | Vite | 冷启动快、HMR 体验好 |
| 路由 | React Router | 标配 |
| 状态管理 | TanStack Query (React Query) | 服务端状态缓存/重试/失效天然契合只读 dashboard；无需 Redux |
| 全局 UI 状态 | Zustand（轻量） | 仅主题/侧栏等少量本地状态 |
| 图表（金融） | lightweight-charts | K 线 + 指标叠加，TradingView 出品，专业 |
| 图表（统计） | ECharts | 净值曲线、夏普/回撤、IC/IR 分层、热力图 |
| 表格 | TanStack Table | 订单/信号/决策列表 |
| 实时 | 原生 EventSource (SSE) | 浏览器内置，免依赖 |
| UI 组件 | Ant Design 或 MUI（择一，默认 AntD） | 国内场景 AntD 表格/表单成熟 |
| 测试 | Vitest + React Testing Library + Playwright（E2E 可选） | 与 Vite 同构 |

#### 3.2.2 页面与组件

| 页面 | 路由 | 关键组件 | 数据来源（REST/SSE） |
|---|---|---|---|
| ① 交易仪表盘 | `/dashboard` | 持仓卡片表（实时浮盈）、账户净值小图、今日订单表、市场状态栏 | `/account` `/positions` `/orders?date=today` `/equity-curve` + SSE `account` `fills` |
| ② 绩效统计 | `/performance` | 净值曲线（含基准对比）、回撤图、关键指标卡（年化/夏普/最大回撤/胜率/盈亏比）、月度收益热力图、回测对比表 | `/performance` `/performance/:run_id` `/equity-curve?mode=backtest` |
| ③ 信号/决策监控 | `/signals` `/decisions` | 实时信号流（SSE 推送，新行高亮）、信号详情抽屉（reason 列表）、决策日志表、决策详情（rationale + risk_checks 树） | `/signals` `/decisions` + SSE `signals` `decisions` |
| ④ 行情图表 | `/chart/:symbol` | K 线主图（lightweight-charts，可切换日/1m/5m + 复权）、指标叠加（MACD/RSI/布林/KDJ）、五档盘口、基本面信息条 | `/bars` `/snapshot/:symbol` `/indicators` |
| ⑤ 因子分析 | `/factors` | 因子选择器、IC 时序图、IR 柱状图、分层收益曲线（quantile_returns）、因子衰减曲线（decay）、横截面排名表 | `/factors/evaluation` `/factors/values` |
| 设置（轻量） | `/settings` | watchlist 查看（只读）、主题切换、SSE 连接状态指示 | `/watchlist` |

#### 3.2.3 状态管理与数据流
- 所有 REST 数据经 **TanStack Query**：`useQuery(['positions'])` / `useInfiniteQuery(['signals'])` 等；缓存 5~30s，持仓/账户等高频数据用 SSE 事件触发 `queryClient.invalidateQueries`。
- SSE 用单个全局 `EventSource` 连接（在 App 顶层建立），事件分发到各页面的 query 失效或本地 store。
- 不引入 Redux——只读后台无复杂客户端状态。

#### 3.2.4 前端构建产物与托管
- `npm run build` 产出静态资源到 `frontend/dist/`。
- 后端 Axum 用 `tower-http::services::ServeDir` 托管 `dist/`，实现"单端口同时提供 API 与 SPA"（简化部署，符合单用户容器化）。
- 开发期 Vite dev server 通过 proxy 转发 `/api` 与 `/api/v1/events` 到 Axum（:8080）。

### 3.3 数据流总览（前后端 + persistence + 事件）
```
Postgres/Parquet ──读──▶ Axum 路由 ──REST JSON──▶ React Query 缓存 ──▶ React 组件
        ▲                     │
        │ 增量轮询(2s)         └──SSE──▶ EventSource ──▶ invalidateQueries
        │
  业务服务写入（signal/decision/order/fill 表）
```

---

## 4. tg-infra 详细设计

### 4.1 docker-compose 服务清单与依赖拓扑

#### 4.1.1 服务清单

| 服务（compose service） | 来源 | 端口（host:container） | 依赖（depends_on with condition: service_healthy） | 说明 |
|---|---|---|---|---|
| `postgres` | 官方镜像 `postgres:16` | `5432:5432` | — | 元数据/订单/持仓/账户/信号/决策/绩效 |
| `collector-python` | `market-data/collector-python` 镜像 | `8001:8000` | `postgres`（可选，sidecar 无状态） | Python FastAPI sidecar（ADR-011/018）取数 |
| `market-data` | `tg-market-data` 镜像 | `50051:50051`（gRPC） `/health` | `postgres` `collector-python` | 行情采集 Rust 主服务 |
| `indicators` | `cpp/tg-indicators` 镜像 | `50052:50051`（gRPC） `/health` | — | C++20 指标 gRPC 服务（ADR-005） |
| `factor-engine` | `tg-factor-engine` 镜像 | `50053:50051` `/health` | `postgres` `indicators` | 因子计算 |
| `backtest` | `tg-backtest` 镜像 | `50054:50051` `/health` | `postgres` | 回测驱动 |
| `signal-engine` | `tg-signal-engine` 镜像 | `50055:50051` `/health` | `postgres` `indicators` `factor-engine` | 信号产生 |
| `decision-agent` | `tg-decision-agent` 镜像 | `50056:50051` `/health` | `signal-engine` | LLM 多 agent 决策（调外部 LLM HTTP） |
| `mock-order-engine` | `tg-mock-order-engine` 镜像 | `50057:50051` `/health` | `postgres` `market-data` | 模拟撮合 + 虚拟账户 |
| `monitoring-viz` | `tg-monitoring-viz` 镜像 | `8080:8080`（HTTP） `/health` | `postgres` | Axum 后端 + 前端 SPA（本 Phase） |
| `scheduler` | `tg-infra/scheduler` 镜像（轻量 Python/curl + cron） | — | 上述被调度服务 | 定时触发（§4.3） |
| `prometheus` | 官方 `prom/prometheus` | `9090:9090` | 各服务（scrape） | 指标采集 |
| `grafana` | 官方 `grafana/grafana` | `3000:3000` | `prometheus` `loki` | 看板 |
| `loki` | 官方 `grafana/loki` | `3100:3100` | — | 结构化日志聚合（promtail 同机采集） |
| `promtail` | 官方 `grafana/promtail` | — | `loki` | 收集各服务 stdout 日志 |

#### 4.1.2 依赖拓扑（启动顺序）
```
postgres
   ├──▶ collector-python ──▶ market-data ──┐
   ├──▶ indicators ──────▶ factor-engine ──┤
   ├──▶ backtest                          ──┤
   ├──▶ signal-engine ──▶ decision-agent ──┤
   ├──▶ mock-order-engine                  ──┤
   └──▶ monitoring-viz ───────────────────── ┘
                              │
        scheduler（依赖被触发服务 healthy）    │
        prometheus ◀─── scrape 全部         │
        promtail ──▶ loki ──▶ grafana      │
```
- 用 `depends_on.condition: service_healthy` + 各服务 `/health` 实现有序启动。
- `signal-engine` 真正开始消费需 `factor-engine + indicators + postgres` 就绪；`decision-agent` 需 `signal-engine` 就绪。

#### 4.1.3 网络与卷
- **网络**：单个自定义桥接网络 `tg-net`，所有服务接入；只对宿主机暴露必要端口（Postgres 默认仅内网，monitoring-viz:8080 / grafana:3000 / prometheus:9090 对外）。
- **卷**：
  - `pg-data`（Postgres 数据）
  - `parquet-data`（Parquet 行情/因子/snapshots，挂载到 `market-data`（读写）与所有 DuckDB 只读消费者（只读挂载））
  - `grafana-data`（看板与设置）
  - `prometheus-data`
  - `loki-data`

### 4.2 配置管理方案（ADR-036）

#### 4.2.1 分层
| 层 | 内容 | 形式 |
|---|---|---|
| L1 编排层 | 服务名/端口/依赖/资源限制 | `docker-compose.yml` |
| L2 环境层（非敏感） | 标的范围、轮询频率、日志级别、PG 连接串 | `apps/tg-infra/env/.env.<service>` + compose `env_file` |
| L3 服务配置层 | 各服务结构化配置（watchlist、策略参数、因子清单、LLM provider） | 各服务 `config/*.yaml`，挂载到容器；用 `tg-contracts` 定义的 schema 校验 |
| L4 密钥层 | 数据源 token、LLM api_key、PG 密码、Grafana admin 密码 | **Docker secrets**（文件 `/run/secrets/xxx`）或 compose `secrets`；禁止明文写 env |

#### 4.2.2 配置文件示例清单（落在 `apps/tg-infra/config/`）
- `watchlist.yaml`（标的 + strategy_tags，market-data 与 signal-engine 共用）
- `market-data.yaml`（轮询频率、限频桶大小、复权策略默认值）
- `factor-engine.yaml`（因子注册清单 + 计算窗口）
- `signal-engine.yaml`（三套策略原型参数：swing / t0 / limitup）
- `decision-agent.yaml`（LLM provider base_url + model + prompt 模板路径；api_key 走 secret）
- `mock-order-engine.yaml`（初始资金、手续费率、风控阈值、T+0 标的白名单）
- `monitoring-viz.yaml`（默认时间范围、刷新间隔、特性开关）

#### 4.2.3 校验
- 启动时各服务用 `tg-contracts` 的 serde 反序列化 + 额外校验逻辑验证配置；失败则容器 `/health` 返回 unhealthy，compose 依赖链阻断下游启动。

### 4.3 定时调度清单（scheduler sidecar，ADR-037）

| cron（CST） | 任务 | 触发方式 | 说明 |
|---|---|---|---|
| `00 15 * * 1-5` | 收盘后采日K/分钟K增量 | HTTP 调 market-data gRPC 控制面 `TriggerIncrementalSync`（经 sidecar 适配或直接 gRPC） | 每个交易日 15:00 收盘后跑 |
| `30 15 * * 1-5` | 每日因子计算（横截面） | 调 factor-engine 触发接口 | 依赖采数完成 |
| `00 16 * * 1-5` | 每日因子评估（IC/IR/分层刷新） | 调 factor-engine `EvaluateFactor` | 写回 persistence |
| `30 16 * * 1-5` | 定时回测（昨日信号复跑/周策略复评） | 调 backtest `SubmitBacktest` | 异步任务，结果入 persistence 供前端查 |
| `00 18 * * 5` | 周报：绩效汇总 | 调 backtest + 写通知表 | 延期项前置占位 |

实现选型（ADR-037）：scheduler 用一个**极简容器**（基于 `supercronic` 或 `ofelia`，支持 compose 内 cron + 日志到 stdout 供 loki 采集），内部用 `curl`/`grpcurl` 触发各服务 HTTP/gRPC 控制面。不引入外部 Airflow（YAGNI，单用户）。

### 4.4 可观测性栈

| 维度 | 实现 |
|---|---|
| **结构化日志** | 各服务用 `tracing` 输出 JSON 到 stdout；`promtail` 采集 → `loki` → Grafana 查询；统一字段：`service / level / symbol / request_id / ts` |
| **指标** | 各服务暴露 Prometheus `/metrics`（Rust 用 `metrics-exporter-prometheus`，C++ 用 prometheus cpp client）；`prometheus` 抓取；Grafana 看板（系统总览 + 各服务 RED 指标 + 业务指标：采集成功率/信号数/订单数/延迟） |
| **链路追踪** | 延期（§10），架构预留：tracing 字段带 `trace_id`，未来接 OpenTelemetry collector |
| **看板** | Grafana 预置两个 dashboard：① 基础设施（容器 CPU/内存/日志速率/HTTP 错误率）；② 业务（持仓数/今日订单/信号 QPS/回测任务状态/采集成功率） |

### 4.5 健康检查
- 每个服务暴露 `GET /health`（HTTP），返回 `{ status: "ok"|"degraded", checks: { db, deps... } }`。
- compose 中每个服务配 `healthcheck`：`test: ["CMD", "curl", "-f", "http://localhost:<port>/health"]`，`interval: 15s`，`timeout: 3s`，`retries: 5`。
- Rust 服务用 `tower-http` + `axum` 路由实现 `/health`，检查项：DB 连接、关键依赖（market-data 检查 collector-python 连通；mock-order-engine 检查 market-data）。
- C++ indicators 的 `/health` 用一个简易 HTTP 端点（与 gRPC 共存，cpp-httplib 实现）或暴露 gRPC Health Checking Protocol（prometheus 抓取 + compose 用 grpc_health_probe 二进制作 healthcheck）。
- monitoring-viz 后端 `/health` 检查 persistence（DB + Parquet 目录可读）。

### 4.6 密钥管理（ADR-036）
- 用 **Docker secrets**（compose `secrets:` + 各服务 `source:`）。密钥以文件挂载到 `/run/secrets/<name>`，服务启动时读取。优于 env 明文：不会被 `docker inspect` 直接泄露，不被子进程 env 继承。
- 密钥清单：
  - `db-password`（Postgres）
  - `akshare-token`（若需；akshare 多为匿名，预留）
  - `llm-api-key`（decision-agent）
  - `grafana-admin-password`
- 本地开发：`.env`（gitignore）+ compose `env_file`；生产/个人部署：`docker secret create` 或 compose secrets + 外部 secret 文件。
- 不引入 Vault（YAGNI，单用户）。

---

## 5. 接口定义

### 5.1 MonitoringApi REST 端点（contracts §3）
见 §3.1.3 端点表。命名前缀 `/api/v1/`，与 contracts spec §3 的 `MonitoringApi` 一致（REST 为主，gRPC 可选，本期不实现 gRPC 形式）。

### 5.2 docker-compose 服务/端口/卷/网络概要

```yaml
# apps/tg-infra/docker-compose.yml（概要，非最终实现）
name: tradeglance

networks:
  tg-net: { driver: bridge }

volumes:
  pg-data:
  parquet-data:
  grafana-data:
  prometheus-data:
  loki-data:

secrets:
  db-password:      { file: ./secrets/db-password.txt }
  llm-api-key:      { file: ./secrets/llm-api-key.txt }
  grafana-admin-password: { file: ./secrets/grafana-admin.txt }

services:
  postgres:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD_FILE: /run/secrets/db-password
    secrets: [db-password]
    volumes: [pg-data:/var/lib/postgresql/data]
    healthcheck: { test: ["CMD-SHELL","pg_isready -U tg"], interval: 10s }
    networks: [tg-net]

  collector-python:
    build: ../../market-data/collector-python
    healthcheck: { test: ["CMD","curl","-f","http://localhost:8000/health"], ... }
    networks: [tg-net]

  market-data:
    build: ../../crates/tg-market-data
    env_file: [./env/market-data.env]
    volumes:
      - parquet-data:/data           # 读写
      - ./config:/config:ro
    depends_on:
      postgres: { condition: service_healthy }
      collector-python: { condition: service_healthy }
    ports: ["50051:50051"]
    networks: [tg-net]

  indicators:
    build: ../../cpp/tg-indicators
    ports: ["50052:50051"]
    networks: [tg-net]

  factor-engine:
    build: ../../crates/tg-factor-engine
    depends_on:
      postgres: { condition: service_healthy }
      indicators: { condition: service_healthy }
    ports: ["50053:50051"]
    volumes: [parquet-data:/data:ro]
    networks: [tg-net]

  backtest:
    build: ../../crates/tg-backtest
    depends_on: [postgres]
    ports: ["50054:50051"]
    volumes: [parquet-data:/data:ro]
    networks: [tg-net]

  signal-engine:
    build: ../../crates/tg-signal-engine
    depends_on:
      postgres: { condition: service_healthy }
      indicators: { condition: service_healthy }
      factor-engine: { condition: service_healthy }
    ports: ["50055:50051"]
    networks: [tg-net]

  decision-agent:
    build: ../../crates/tg-decision-agent
    secrets: [llm-api-key]
    depends_on: [signal-engine]
    ports: ["50056:50051"]
    networks: [tg-net]

  mock-order-engine:
    build: ../../crates/tg-mock-order-engine
    depends_on:
      postgres: { condition: service_healthy }
      market-data: { condition: service_healthy }
    ports: ["50057:50051"]
    networks: [tg-net]

  monitoring-viz:
    build: ../../apps/tg-monitoring-viz
    depends_on: [postgres]
    ports: ["8080:8080"]
    volumes: [parquet-data:/data:ro]
    networks: [tg-net]

  scheduler:
    build: ./scheduler
    depends_on: [market-data, factor-engine, backtest]
    networks: [tg-net]

  prometheus: { image: prom/prometheus, ports: ["9090:9090"], ... }
  loki:       { image: grafana/loki, ports: ["3100:3100"], ... }
  promtail:   { image: grafana/promtail, ... }
  grafana:    { image: grafana/grafana, ports: ["3000:3000"], secrets: [grafana-admin-password], ... }
```

> 说明：实际端口分配、镜像名、healthcheck 细节在实现期固化；本表为"概要契约"，确保覆盖全部 8 业务服务 + sidecar + 基础设施。

---

## 6. 错误处理与可观测性

### 6.1 错误处理
- **后端**：业务错误用 contracts §4 的 `TgError`；Axum 层用 `ApiError` 实现 `IntoResponse`，映射 HTTP 状态：
  - `NotFound` → 404
  - `Validation` → 400
  - `RateLimited` → 429
  - `Upstream` → 502
  - `Other` → 500
- **前端**：TanStack Query 统一 `onError` → toast 提示；SSE 断线由浏览器自动重连（指数退避），UI 顶栏显示连接状态点。
- **可观测**：
  - 后端每个请求中间件注入 `tracing` span（`request_id` / `route` / `latency`）。
  - Prometheus 指标：`http_requests_total{route,status}`、`http_request_duration_seconds`、`sse_clients`、`persistence_query_duration_seconds`。
  - 日志到 stdout → promtail → loki。

### 6.2 可观测性补充
- Grafana 看板预置告警规则（仅看板内，不推送，延期项）：采集失败率 > 阈值、SSE 客户端骤降、Postgres 连接数高。
- `monitoring-viz` 后端 `/health` 聚合 Postgres 连通 + Parquet 目录可读 + SSE 轮询器存活三项。

---

## 7. 测试策略

### 7.1 前端组件测试
- 用 Vitest + React Testing Library：每个页面/关键组件写渲染 + 数据 mock 测试（mock fetch / EventSource）。
- 覆盖：持仓表渲染、净值曲线数据→图表 props 转换、SSE 事件触发 query 失效、错误态展示。
- E2E（可选，Playwright）：`docker compose up` 后访问 `/dashboard` 截图回归。

### 7.2 后端 API 集成测试
- 用 Axum `TestServer` + **persistence fixture**（测试专用 Postgres 容器 + 预置 Parquet fixture 数据）。
- 每个 REST 端点写集成测试：
  - 预置 `Order/Fill/Position/Signal/Decision/FactorValue` 数据
  - 调端点 → 校验 JSON shape 与 contracts 类型一致
  - 校验分页、时间过滤、错误码
- SSE 端点测试：启动客户端 → 后端轮询器检测到新行 → 客户端收到事件帧 + `Last-Event-ID` 续传。

### 7.3 compose 冒烟测试
- 脚本 `apps/tg-infra/scripts/smoke.sh`：
  1. `docker compose up -d --build`
  2. 轮询所有服务 `/health` 直至全 healthy（超时 5min 失败）
  3. 调 `monitoring-viz` `/api/v1/account` `/api/v1/watchlist` 验证非 500
  4. 触发一次增量同步，等待 `fetch_state` 反映
  5. 验证 grafana/prometheus 可达
  6. `docker compose down -v` 清理
- CI（GitHub Actions / 本地）跑冒烟，确保编排可重复构建。

### 7.4 测试数据策略
- 集成测试不依赖真实 akshare：collector-python 在测试模式下用 fixture 路由（Phase 0 已确立 mock sidecar 模式）。
- 回测/绩效测试用预置历史 Parquet fixture（小规模，几只标的 × 数月）。

---

## 8. 验收标准（Definition of Done）

1. `docker compose up` 一条命令拉起全部服务（含 Postgres、collector-python、8 个业务服务、monitoring-viz、scheduler、prometheus、grafana、loki、promtail），无手动干预。
2. 启动后 5 分钟内所有服务 `/health` 返回 healthy，`docker compose ps` 全部 `healthy`。
3. 浏览器访问 `http://<host>:8080` 可加载前端 SPA，五类视图（仪表盘/绩效/信号决策/行情/因子）均可渲染（无 JS 致命错误）。
4. 前端通过 REST 取数正确（持仓/订单/绩效/信号/决策/行情/因子各至少一个端点验证通过），**不直连数据库**（抓包校验仅 8080 流量）。
5. SSE `/api/v1/events/stream` 能实时推送 signal/decision/order/fill 增量，前端实时刷新；断线重连后用 `Last-Event-ID` 续传无丢事件。
6. cron 调度在指定时间成功触发增量采数、因子计算、定时回测（日志可见 + persistence 落库可见）。
7. Prometheus 成功抓取全部服务 `/metrics`；Grafana 预置看板有数据；Loki 收到全部服务结构化日志。
8. 密钥经 Docker secrets 注入，`docker inspect` 不显示明文；服务能正确读取 LLM api_key / db-password。
9. 前端组件测试 + 后端集成测试 + compose 冒烟测试全绿。
10. 监控可视化：仪表盘能反映一次完整链路（信号→决策→订单→成交→持仓变化→净值变化）的实时演进。

---

## 9. 依赖的 ADR

### 9.1 既有 ADR
- **ADR-003** 部署形态：个人容器化，docker-compose，单用户（→ tg-infra 形态）
- **ADR-007** 存储：PostgreSQL + Parquet/DuckDB（→ monitoring-viz 经 persistence 读，compose 起 Postgres + 卷）
- **ADR-011** 数据接入：Python sidecar（→ compose 含 collector-python 服务）
- **ADR-017** persistence 共享库 crate（→ monitoring-viz 链接 crate 只读，不经 RPC 查行情）
- **ADR-018** sidecar 协议：HTTP/FastAPI（→ collector-python 服务形态）

### 9.2 新增 ADR（ADR-034 ~ ADR-037）

#### ADR-034 前端框架选型：React + Vite + TypeScript
- **决策**：前端采用 React 18 + TypeScript + Vite 构建，搭配 TanStack Query（服务端状态）、Zustand（本地 UI 状态）、lightweight-charts（K 线）+ ECharts（统计图）、Ant Design（组件库）。
- **理由**：React 生态最大，金融图表库（lightweight-charts）适配最佳；TanStack Query 天然契合只读 dashboard 的服务端缓存与失效；Vite 冷启动快。Vue 亦可行但生态略小，不选 Svelte 系（图表库适配少）。
- **影响**：`apps/tg-monitoring-viz/frontend` 为标准 Vite 工程；构建产物由 Axum `ServeDir` 托管。

#### ADR-035 实时事件推送方式：SSE（Server-Sent Events）
- **决策**：monitoring-viz 向前端推送实时信号/决策/订单/成交用 SSE（`text/event-stream`），而非 WebSocket / 长轮询。后端用轮询 persistence 事件表 + tokio broadcast channel 适配（不引消息总线）。
- **理由**：推送单向（服务端→前端），SSE 原生支持断线重连与 `Last-Event-ID` 续传，走 HTTP/2 更省，Axum 实现极简；WebSocket 的双向能力在本场景冗余；引入消息总线（NATS/Redis Streams）违反 ADR-002 YAGNI。
- **影响**：前端用浏览器原生 `EventSource`；后端实现 `/api/v1/events/stream` + 增量轮询器（默认 2s）；延迟可接受（秒级快照场景）。

#### ADR-036 配置与密钥管理方案：分层配置 + Docker secrets
- **决策**：四层配置（compose / env / 各服务 yaml / secrets）；非敏感配置走 `env_file` + 挂载 yaml；密钥（DB 密码、LLM api_key、Grafana 密码）走 **Docker secrets**（文件挂载 `/run/secrets/`），服务读文件而非 env。
- **理由**：分层使配置可审计、可版本化（yaml 入 git，secrets 不入）；Docker secrets 比 env 明文更安全（不进 `docker inspect`、不被子进程 env 继承）；单用户场景不引 Vault（YAGNI）。
- **影响**：`apps/tg-infra/env/`、`apps/tg-infra/config/`、`apps/tg-infra/secrets/`（gitignore）三目录；各服务启动需支持"从文件读密钥"。

#### ADR-037 调度实现选型：容器内 cron（supercronic / ofelia）
- **决策**：定时任务用一个轻量 scheduler 容器承载，内部用 `supercronic`（或 `ofelia`）跑 cron，通过 HTTP/gRPC 调各服务控制面触发（增量采数、因子计算、定时回测）。
- **理由**：单用户 + 任务量小，不引 Airflow / Prefect / K8s CronJob；supercronic 把 cron 输出到 stdout，自然进 loki；compose 原生支持，零外部依赖。
- **影响**：`apps/tg-infra/scheduler/` 目录；cron 表见 §4.3；调度失败由可观测性栈反映（日志 + Grafana 看板）。

---

## 10. 后续 / 延期项

- **告警/通知**：邮件 / IM（钉钉/飞书/Telegram）推送——告警规则已在 Grafana 预置，仅缺通知通道；下期接入。
- **分布式链路追踪**：OpenTelemetry collector + Jaeger/Tempo——架构已预留 `trace_id`，跨服务追踪待业务量上升后接入。
- **多用户 / 登录 / RBAC / 审计**：当前单用户个人系统，前端无鉴权；未来若多人协作再加 OAuth + 角色权限。
- **参数寻优可视化**：Phase 2 回测的网格/贝叶斯寻优结果，在 `/performance` 增加寻优热力图。
- **watchlist Web 管理 UI**：当前 `/settings` 只读，下期支持增删标的（调 `MarketDataControl.UpdateWatchlist`）。
- **Kubernetes / 远程部署**：当前 docker-compose（ADR-003），未来上云再写 Helm chart。
- **实时事件总线升级**：若 SSE 轮询延迟不满足（例如未来接 tick 行情），演进为 Redis Streams / NATS JetStream（架构设计原则 4 预留路径）。

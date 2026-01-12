# API 文档

Quantization MCP Server API 完整参考文档。

## 协议

服务器使用 JSON-RPC 2.0 协议，通过 stdin/stdout 进行通信。

### 请求格式

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "method_name",
  "params": {
    "param1": "value1",
    "param2": "value2"
  }
}
```

### 响应格式

**成功响应:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "data": "..."
  }
}
```

**错误响应:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -1,
    "message": "Error description"
  }
}
```

---

## 方法列表

### 1. list_indicators

列出所有可用的技术指标。

**请求:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "list_indicators",
  "params": {}
}
```

**响应:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "indicators": [
      {
        "name": "sma_20",
        "display_name": "SMA",
        "min_data_points": 20
      },
      {
        "name": "rsi_14",
        "display_name": "RSI",
        "min_data_points": 15
      }
    ]
  }
}
```

**字段说明:**
- `name`: 指标唯一标识符
- `display_name`: 指标显示名称
- `min_data_points`: 计算该指标所需的最少数据点数

---

### 2. calculate_indicator

计算指定的技术指标。

**请求:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "calculate_indicator",
  "params": {
    "indicator": "rsi_14",
    "symbol": "AAPL",
    "interval": "1d",
    "count": 100
  }
}
```

**参数:**
- `indicator` (必需): 指标名称
- `symbol` (必需): 股票代码
- `interval` (可选): 时间间隔，默认 "1d"
  - 支持: "1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w", "1M"
- `count` (可选): 数据点数量，默认 100

**响应:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "indicator": "rsi_14",
    "symbol": "AAPL",
    "interval": "1d",
    "values": [
      {
        "timestamp": 1704067200,
        "value": 65.32
      },
      {
        "timestamp": 1704153600,
        "value": 68.45
      }
    ]
  }
}
```

**字段说明:**
- `timestamp`: Unix时间戳（秒）
- `value`: 指标值

---

### 3. fetch_data

获取原始市场数据（OHLCV）。

**请求:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "fetch_data",
  "params": {
    "symbol": "AAPL",
    "interval": "1d",
    "count": 50
  }
}
```

**参数:**
- `symbol` (必需): 股票代码
- `interval` (可选): 时间间隔，默认 "1d"
- `count` (可选): 数据点数量，默认 100

**响应:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "symbol": "AAPL",
    "interval": "1d",
    "data": [
      {
        "timestamp": 1704067200,
        "open": 150.25,
        "high": 152.30,
        "low": 149.80,
        "close": 151.50,
        "volume": 50000000
      }
    ]
  }
}
```

**字段说明:**
- `timestamp`: Unix时间戳（秒）
- `open`: 开盘价
- `high`: 最高价
- `low`: 最低价
- `close`: 收盘价
- `volume`: 成交量

---

## 可用指标列表

### 移动平均类
| 指标名称 | 说明 | 周期 |
|---------|------|------|
| `sma_5` | 5日简单移动平均 | 5 |
| `sma_10` | 10日简单移动平均 | 10 |
| `sma_20` | 20日简单移动平均 | 20 |
| `sma_50` | 50日简单移动平均 | 50 |
| `sma_200` | 200日简单移动平均 | 200 |
| `ema_5` | 5日指数移动平均 | 5 |
| `ema_10` | 10日指数移动平均 | 10 |
| `ema_12` | 12日指数移动平均 | 12 |
| `ema_20` | 20日指数移动平均 | 20 |
| `ema_26` | 26日指数移动平均 | 26 |
| `ema_50` | 50日指数移动平均 | 50 |

### 动量指标
| 指标名称 | 说明 | 参数 |
|---------|------|------|
| `rsi_14` | 14日相对强弱指标 | period=14 |
| `rsi_9` | 9日相对强弱指标 | period=9 |
| `macd` | 标准MACD | fast=12, slow=26, signal=9 |
| `macd_fast` | 快速MACD | fast=5, slow=13, signal=5 |

### 波动率指标
| 指标名称 | 说明 | 参数 |
|---------|------|------|
| `bb_20` | 20日布林带（2倍标准差） | period=20, std=2.0 |
| `bb_20_3std` | 20日布林带（3倍标准差） | period=20, std=3.0 |
| `atr_14` | 14日平均真实波幅 | period=14 |

### 随机指标
| 指标名称 | 说明 | 参数 |
|---------|------|------|
| `stoch_14_3` | 标准随机指标 | k=14, d=3 |
| `stoch_5_3` | 快速随机指标 | k=5, d=3 |

### 趋势指标
| 指标名称 | 说明 | 参数 |
|---------|------|------|
| `adx_14` | 14日平均趋向指标 | period=14 |
| `cci_20` | 20日顺势指标 | period=20 |
| `cci_14` | 14日顺势指标 | period=14 |

### 其他指标
| 指标名称 | 说明 | 参数 |
|---------|------|------|
| `williams_r_14` | 14日威廉指标 | period=14 |
| `obv` | 能量潮 | - |

---

## 错误代码

| 错误码 | 说明 |
|-------|------|
| -1 | 通用错误 |
| -32700 | JSON解析错误 |
| -32600 | 无效请求 |
| -32601 | 方法不存在 |
| -32602 | 无效参数 |
| -32603 | 内部错误 |

---

## 使用示例

### cURL 示例

```bash
# 通过管道发送请求
echo '{"jsonrpc":"2.0","id":1,"method":"list_indicators","params":{}}' | ./quantization-mcp
```

### Python 示例

```python
import json
import subprocess

# 启动服务器
process = subprocess.Popen(
    ['./quantization-mcp'],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    text=True
)

# 发送请求
request = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "calculate_indicator",
    "params": {
        "indicator": "rsi_14",
        "symbol": "AAPL",
        "interval": "1d",
        "count": 100
    }
}

process.stdin.write(json.dumps(request) + '\n')
process.stdin.flush()

# 读取响应
response = json.loads(process.stdout.readline())
print(response)
```

### Node.js 示例

```javascript
const { spawn } = require('child_process');

// 启动服务器
const server = spawn('./quantization-mcp');

// 发送请求
const request = {
  jsonrpc: '2.0',
  id: 1,
  method: 'calculate_indicator',
  params: {
    indicator: 'rsi_14',
    symbol: 'AAPL',
    interval: '1d',
    count: 100
  }
};

server.stdin.write(JSON.stringify(request) + '\n');

// 读取响应
server.stdout.on('data', (data) => {
  const response = JSON.parse(data.toString());
  console.log(response);
});
```

---

## 性能考虑

### 批量请求

为了提高性能，可以连续发送多个请求：

```json
{"jsonrpc":"2.0","id":1,"method":"calculate_indicator","params":{"indicator":"rsi_14","symbol":"AAPL"}}
{"jsonrpc":"2.0","id":2,"method":"calculate_indicator","params":{"indicator":"macd","symbol":"AAPL"}}
{"jsonrpc":"2.0","id":3,"method":"calculate_indicator","params":{"indicator":"bb_20","symbol":"AAPL"}}
```

服务器会按顺序处理并返回响应。

### 数据缓存

建议在客户端实现缓存机制，避免重复请求相同的数据。

### 连接复用

保持服务器进程运行，复用连接可以减少启动开销。

---

## 扩展开发

### 添加自定义指标

1. 实现 `IIndicator` 接口
2. 在 `main.cpp` 中注册指标
3. 重新编译

示例代码见 `docs/DEVELOPMENT.md`

### 自定义数据源

1. 实现 `IMarketDataSource` 接口
2. 在 `main.cpp` 中替换数据源
3. 重新编译

---

## 限制和注意事项

1. **数据量限制**: 单次请求建议不超过10000个数据点
2. **并发限制**: 当前版本不支持并发请求
3. **内存使用**: 大量数据可能占用较多内存
4. **网络超时**: 默认30秒超时
5. **数据精度**: 使用双精度浮点数（double）

---

## 版本历史

### v1.0.0 (2024-01-12)
- 初始版本
- 支持11种技术指标
- 网络数据源实现
- MCP协议支持

---

## 支持和反馈

- GitHub Issues: https://github.com/your-repo/quantization-mcp/issues
- 文档: https://github.com/your-repo/quantization-mcp/docs

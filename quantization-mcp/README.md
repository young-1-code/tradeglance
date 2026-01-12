# Quantization MCP Server

基于C++23实现的股票技术指标MCP服务器，提供主流技术指标计算功能。

## 特性

- ✅ **C++23标准**: 使用最新的C++特性，包括`std::expected`
- ✅ **模块化设计**: 每个技术指标独立文件实现
- ✅ **统一数据接口**: 支持多种数据源，当前实现网络获取
- ✅ **MCP协议**: 通过stdin/stdout进行JSON-RPC通信
- ✅ **丰富的指标**: 支持10+种主流技术指标

## 支持的技术指标

### 移动平均类
- **SMA** (Simple Moving Average) - 简单移动平均
- **EMA** (Exponential Moving Average) - 指数移动平均

### 动量指标
- **RSI** (Relative Strength Index) - 相对强弱指标
- **MACD** (Moving Average Convergence Divergence) - 指数平滑异同移动平均线
- **Stochastic Oscillator** - 随机指标
- **Williams %R** - 威廉指标

### 波动率指标
- **Bollinger Bands** - 布林带
- **ATR** (Average True Range) - 平均真实波幅

### 趋势指标
- **ADX** (Average Directional Index) - 平均趋向指标
- **CCI** (Commodity Channel Index) - 顺势指标

### 成交量指标
- **OBV** (On-Balance Volume) - 能量潮

## 项目结构

```
quantization-mcp/
├── include/
│   ├── market_data_source.hpp      # 统一数据源接口
│   ├── network_data_source.hpp     # 网络数据源实现
│   ├── mcp_server.hpp              # MCP服务器
│   └── indicators/
│       ├── indicator_base.hpp      # 指标基类
│       ├── sma.hpp                 # 简单移动平均
│       ├── ema.hpp                 # 指数移动平均
│       ├── rsi.hpp                 # 相对强弱指标
│       ├── macd.hpp                # MACD
│       ├── bollinger_bands.hpp     # 布林带
│       ├── stochastic.hpp          # 随机指标
│       ├── atr.hpp                 # 平均真实波幅
│       ├── adx.hpp                 # 平均趋向指标
│       ├── cci.hpp                 # 顺势指标
│       ├── williams_r.hpp          # 威廉指标
│       └── obv.hpp                 # 能量潮
├── src/
│   ├── main.cpp                    # 主程序入口
│   ├── mcp_server.cpp              # MCP服务器实现
│   ├── network_data_source.cpp     # 网络数据源实现
│   └── indicators/                 # 各指标实现
│       ├── sma.cpp
│       ├── ema.cpp
│       ├── rsi.cpp
│       ├── macd.cpp
│       ├── bollinger_bands.cpp
│       ├── stochastic.cpp
│       ├── atr.cpp
│       ├── adx.cpp
│       ├── cci.cpp
│       ├── williams_r.cpp
│       └── obv.cpp
└── CMakeLists.txt
```

## 依赖项

- **C++23编译器**: GCC 13+, Clang 16+, 或 MSVC 2022+
- **CMake**: 3.20+
- **libcurl**: HTTP客户端库
- **nlohmann/json**: JSON解析库

## 编译安装

### Ubuntu/Debian

```bash
# 安装依赖
sudo apt update
sudo apt install -y build-essential cmake libcurl4-openssl-dev nlohmann-json3-dev

# 编译
mkdir build && cd build
cmake ..
make -j$(nproc)

# 安装（可选）
sudo make install
```

### macOS

```bash
# 安装依赖
brew install cmake curl nlohmann-json

# 编译
mkdir build && cd build
cmake ..
make -j$(sysctl -n hw.ncpu)
```

### 使用vcpkg（跨平台）

```bash
# 安装vcpkg依赖
vcpkg install curl nlohmann-json

# 编译
mkdir build && cd build
cmake .. -DCMAKE_TOOLCHAIN_FILE=[vcpkg root]/scripts/buildsystems/vcpkg.cmake
cmake --build . --config Release
```

## 使用方法

### 启动服务器

```bash
# 使用默认API端点
./quantization-mcp

# 指定API端点
./quantization-mcp http://your-api-endpoint.com/api

# 或使用环境变量
export MARKET_DATA_API=http://your-api-endpoint.com/api
./quantization-mcp
```

### MCP协议示例

服务器通过stdin接收JSON-RPC请求，通过stdout返回响应。

#### 1. 列出所有可用指标

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

#### 2. 计算技术指标

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

#### 3. 获取原始行情数据

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

## 数据源接口

### 当前实现：网络数据源

`NetworkDataSource` 通过HTTP API获取行情数据。

**API端点格式:**

- 获取历史数据: `GET /ohlcv?symbol={symbol}&interval={interval}&start={start}&end={end}`
- 获取最新数据: `GET /ohlcv/latest?symbol={symbol}&interval={interval}&count={count}`

**响应格式:**
```json
{
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
```

### 扩展其他数据源

实现 `IMarketDataSource` 接口即可添加新的数据源：

```cpp
class CustomDataSource : public IMarketDataSource {
public:
    std::expected<std::vector<OHLCV>, DataSourceError>
    fetch_ohlcv(...) override {
        // 自定义实现
    }

    std::expected<std::vector<OHLCV>, DataSourceError>
    fetch_latest(...) override {
        // 自定义实现
    }
};
```

支持的数据源类型：
- 数据库（MySQL, PostgreSQL, MongoDB等）
- 本地文件（CSV, Parquet等）
- 消息队列（Kafka, RabbitMQ等）
- WebSocket实时数据流
- 其他API服务

## 添加新的技术指标

1. 在 `include/indicators/` 创建头文件
2. 在 `src/indicators/` 创建实现文件
3. 继承 `IIndicator` 接口
4. 在 `CMakeLists.txt` 添加源文件
5. 在 `main.cpp` 注册指标

**示例:**

```cpp
// include/indicators/my_indicator.hpp
#pragma once
#include "indicator_base.hpp"

namespace quantization::indicators {

class MyIndicator : public IIndicator {
public:
    explicit MyIndicator(size_t period);

    std::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "MyIndicator"; }
    size_t min_data_points() const override { return period_; }

private:
    size_t period_;
};

} // namespace quantization::indicators
```

## 性能优化

- 使用 `-O3` 编译优化
- 启用LTO（链接时优化）
- 考虑使用并行计算（OpenMP, TBB）
- 缓存计算结果

## 许可证

MIT License

## 贡献

欢迎提交Issue和Pull Request！

## 联系方式

- GitHub: [quantization-mcp](https://github.com/your-repo/quantization-mcp)

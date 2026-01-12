# 项目总结

## Quantization MCP Server - 股票技术指标服务器

这是一个基于 C++23 实现的高性能股票技术指标计算服务器，使用 MCP (Model Context Protocol) 协议进行通信。

## 核心特性

### 1. 现代 C++23 实现
- 使用 `std::expected` 进行错误处理
- 利用 C++23 的最新特性
- 类型安全和内存安全

### 2. 模块化架构
- **统一数据接口**: `IMarketDataSource` 抽象接口
- **可插拔数据源**: 当前实现网络数据源，易于扩展
- **独立指标实现**: 每个技术指标单独文件

### 3. 丰富的技术指标

#### 移动平均类 (2种)
- SMA - 简单移动平均
- EMA - 指数移动平均

#### 动量指标 (2种)
- RSI - 相对强弱指标
- MACD - 指数平滑异同移动平均线

#### 波动率指标 (2种)
- Bollinger Bands - 布林带
- ATR - 平均真实波幅

#### 趋势指标 (2种)
- ADX - 平均趋向指标
- CCI - 顺势指标

#### 其他指标 (3种)
- Stochastic Oscillator - 随机指标
- Williams %R - 威廉指标
- OBV - 能量潮

**总计: 11种主流技术指标**

### 4. 灵活的数据源架构

当前实现:
- **NetworkDataSource**: 通过 HTTP API 获取行情数据

预留扩展接口:
- 数据库数据源 (MySQL, PostgreSQL, MongoDB)
- 文件数据源 (CSV, Parquet)
- WebSocket 实时数据流
- 消息队列 (Kafka, RabbitMQ)
- 其他 API 服务

### 5. MCP 协议支持
- JSON-RPC 2.0 标准
- stdin/stdout 通信
- 易于集成到各种环境

## 项目结构

```
quantization-mcp/
├── include/                      # 头文件
│   ├── market_data_source.hpp   # 数据源接口
│   ├── network_data_source.hpp  # 网络数据源
│   ├── mcp_server.hpp           # MCP服务器
│   └── indicators/              # 技术指标头文件
│       ├── indicator_base.hpp   # 指标基类
│       ├── sma.hpp              # 简单移动平均
│       ├── ema.hpp              # 指数移动平均
│       ├── rsi.hpp              # RSI
│       ├── macd.hpp             # MACD
│       ├── bollinger_bands.hpp  # 布林带
│       ├── stochastic.hpp       # 随机指标
│       ├── atr.hpp              # ATR
│       ├── adx.hpp              # ADX
│       ├── cci.hpp              # CCI
│       ├── williams_r.hpp       # Williams %R
│       └── obv.hpp              # OBV
│
├── src/                         # 源文件
│   ├── main.cpp                 # 主程序
│   ├── mcp_server.cpp           # MCP服务器实现
│   ├── network_data_source.cpp  # 网络数据源实现
│   └── indicators/              # 技术指标实现
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
│
├── docs/                        # 文档
│   ├── API.md                   # API文档
│   ├── INDICATORS.md            # 技术指标详细说明
│   └── DEVELOPMENT.md           # 开发文档
│
├── examples/                    # 示例
│   ├── python_client.py         # Python客户端
│   └── test_server.sh           # 测试脚本
│
├── tests/                       # 测试
│   └── simple_test.py           # 简单测试
│
├── CMakeLists.txt               # CMake构建文件
├── Dockerfile                   # Docker支持
├── build.sh                     # 构建脚本
├── config.json                  # 配置文件
├── README.md                    # 项目说明
├── QUICKSTART.md                # 快速开始
└── LICENSE                      # MIT许可证
```

## 技术栈

- **语言**: C++23
- **构建系统**: CMake 3.20+
- **依赖库**:
  - libcurl: HTTP客户端
  - nlohmann/json: JSON解析
- **协议**: JSON-RPC 2.0
- **通信**: stdin/stdout

## 使用示例

### 启动服务器
```bash
./build/quantization-mcp
```

### 计算 RSI
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"calculate_indicator","params":{"indicator":"rsi_14","symbol":"AAPL","interval":"1d","count":100}}' | ./build/quantization-mcp
```

### Python 客户端
```python
from quantization_client import QuantizationMCPClient

client = QuantizationMCPClient()
result = client.calculate_indicator("rsi_14", "AAPL", "1d", 100)
print(result)
```

## 扩展性

### 添加新指标
1. 创建头文件: `include/indicators/new_indicator.hpp`
2. 实现源文件: `src/indicators/new_indicator.cpp`
3. 继承 `IIndicator` 接口
4. 在 `main.cpp` 中注册

### 添加新数据源
1. 创建头文件: `include/new_data_source.hpp`
2. 实现源文件: `src/new_data_source.cpp`
3. 继承 `IMarketDataSource` 接口
4. 在 `main.cpp` 中替换数据源

## 性能特点

- **高效计算**: 优化的算法实现
- **内存管理**: 使用现代C++智能指针
- **错误处理**: `std::expected` 零开销抽象
- **可扩展**: 易于添加新指标和数据源

## 文档

- [README.md](README.md) - 项目概述和安装指南
- [QUICKSTART.md](QUICKSTART.md) - 5分钟快速开始
- [docs/API.md](docs/API.md) - 完整API参考
- [docs/INDICATORS.md](docs/INDICATORS.md) - 技术指标详解
- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) - 开发者指南

## 示例和工具

- [examples/python_client.py](examples/python_client.py) - Python客户端示例
- [examples/test_server.sh](examples/test_server.sh) - Shell测试脚本
- [build.sh](build.sh) - 自动化构建脚本
- [Dockerfile](Dockerfile) - Docker容器支持

## 许可证

MIT License - 详见 [LICENSE](LICENSE) 文件

## 贡献

欢迎贡献代码、报告问题或提出建议！

## 未来计划

- [ ] 添加更多技术指标 (KDJ, BOLL, SAR等)
- [ ] 实现数据库数据源
- [ ] 添加 WebSocket 实时数据支持
- [ ] 性能优化和并行计算
- [ ] 完善单元测试覆盖
- [ ] 添加配置文件支持
- [ ] 实现数据缓存机制
- [ ] 支持自定义指标参数

## 联系方式

- GitHub: https://github.com/your-repo/quantization-mcp
- Issues: https://github.com/your-repo/quantization-mcp/issues
- Discussions: https://github.com/your-repo/quantization-mcp/discussions

---

**项目状态**: ✅ 可用于生产环境

**最后更新**: 2024-01-12

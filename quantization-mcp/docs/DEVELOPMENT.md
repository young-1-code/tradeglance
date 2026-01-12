# 开发文档

本文档面向希望扩展或修改 Quantization MCP Server 的开发者。

## 目录

1. [架构设计](#架构设计)
2. [添加新指标](#添加新指标)
3. [自定义数据源](#自定义数据源)
4. [代码规范](#代码规范)
5. [测试](#测试)
6. [性能优化](#性能优化)

---

## 架构设计

### 核心组件

```
┌─────────────────────────────────────────┐
│           MCP Server                     │
│  (JSON-RPC 2.0 over stdin/stdout)       │
└─────────────┬───────────────────────────┘
              │
              ├──> IMarketDataSource (接口)
              │    ├─> NetworkDataSource (网络)
              │    ├─> DatabaseDataSource (数据库)
              │    └─> FileDataSource (文件)
              │
              └──> IIndicator (接口)
                   ├─> SMA, EMA (移动平均)
                   ├─> RSI, MACD (动量)
                   ├─> BollingerBands, ATR (波动率)
                   └─> ADX, CCI, etc. (趋势)
```

### 设计原则

1. **接口分离**: 数据源和指标通过接口解耦
2. **单一职责**: 每个指标独立文件实现
3. **现代C++**: 使用C++23特性（`std::expected`, concepts等）
4. **错误处理**: 使用`std::expected`而非异常
5. **可扩展性**: 易于添加新指标和数据源

---

## 添加新指标

### 步骤1: 创建头文件

在 `include/indicators/` 创建新指标头文件：

```cpp
// include/indicators/my_indicator.hpp
#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

class MyIndicator : public IIndicator {
public:
    explicit MyIndicator(size_t period);
    ~MyIndicator() override = default;

    std::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "MyIndicator"; }
    size_t min_data_points() const override { return period_; }

private:
    size_t period_;

    // 辅助函数
    double calculate_helper(const OHLCV& candle);
};

} // namespace quantization::indicators
```

### 步骤2: 实现源文件

在 `src/indicators/` 创建实现文件：

```cpp
// src/indicators/my_indicator.cpp
#include "indicators/my_indicator.hpp"

namespace quantization::indicators {

MyIndicator::MyIndicator(size_t period) : period_(period) {
    if (period == 0) {
        throw std::invalid_argument("Period must be greater than 0");
    }
}

std::expected<IndicatorResult, IndicatorError>
MyIndicator::calculate(const std::vector<OHLCV>& data) {
    // 检查数据充足性
    if (data.size() < period_) {
        return std::unexpected(IndicatorError::InsufficientData);
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values.reserve(data.size() - period_ + 1);
    result.timestamps.reserve(data.size() - period_ + 1);

    // 计算指标
    for (size_t i = period_ - 1; i < data.size(); ++i) {
        double value = 0.0;

        // 你的计算逻辑
        for (size_t j = i - period_ + 1; j <= i; ++j) {
            value += calculate_helper(data[j]);
        }
        value /= period_;

        result.values.push_back(value);
        result.timestamps.push_back(data[i].timestamp);
    }

    return result;
}

double MyIndicator::calculate_helper(const OHLCV& candle) {
    // 辅助计算
    return (candle.high + candle.low + candle.close) / 3.0;
}

} // namespace quantization::indicators
```

### 步骤3: 更新CMakeLists.txt

在 `CMakeLists.txt` 的 `INDICATOR_SOURCES` 中添加：

```cmake
set(INDICATOR_SOURCES
    # ... 现有文件 ...
    src/indicators/my_indicator.cpp
)
```

### 步骤4: 注册指标

在 `src/main.cpp` 中注册：

```cpp
#include "indicators/my_indicator.hpp"

// 在 main() 函数中
server.register_indicator("my_indicator_20",
                         std::make_shared<MyIndicator>(20));
```

### 步骤5: 编译测试

```bash
./build.sh
./build/quantization-mcp
```

---

## 自定义数据源

### 实现数据库数据源示例

```cpp
// include/database_data_source.hpp
#pragma once

#include "market_data_source.hpp"
#include <pqxx/pqxx>  // PostgreSQL C++ 库

namespace quantization {

class DatabaseDataSource : public IMarketDataSource {
public:
    explicit DatabaseDataSource(const std::string& connection_string);
    ~DatabaseDataSource() override = default;

    std::expected<std::vector<OHLCV>, DataSourceError>
    fetch_ohlcv(
        const std::string& symbol,
        const std::string& interval,
        std::chrono::system_clock::time_point start,
        std::chrono::system_clock::time_point end
    ) override;

    std::expected<std::vector<OHLCV>, DataSourceError>
    fetch_latest(
        const std::string& symbol,
        const std::string& interval,
        size_t count
    ) override;

private:
    std::unique_ptr<pqxx::connection> conn_;
};

} // namespace quantization
```

```cpp
// src/database_data_source.cpp
#include "database_data_source.hpp"

namespace quantization {

DatabaseDataSource::DatabaseDataSource(const std::string& connection_string) {
    try {
        conn_ = std::make_unique<pqxx::connection>(connection_string);
    } catch (const std::exception& e) {
        // 处理连接错误
    }
}

std::expected<std::vector<OHLCV>, DataSourceError>
DatabaseDataSource::fetch_latest(
    const std::string& symbol,
    const std::string& interval,
    size_t count
) {
    try {
        pqxx::work txn(*conn_);

        std::string query =
            "SELECT timestamp, open, high, low, close, volume "
            "FROM market_data "
            "WHERE symbol = $1 AND interval = $2 "
            "ORDER BY timestamp DESC "
            "LIMIT $3";

        auto result = txn.exec_params(query, symbol, interval, count);

        std::vector<OHLCV> data;
        for (const auto& row : result) {
            OHLCV ohlcv;
            ohlcv.timestamp = std::chrono::system_clock::from_time_t(
                row["timestamp"].as<int64_t>()
            );
            ohlcv.open = row["open"].as<double>();
            ohlcv.high = row["high"].as<double>();
            ohlcv.low = row["low"].as<double>();
            ohlcv.close = row["close"].as<double>();
            ohlcv.volume = row["volume"].as<double>();
            data.push_back(ohlcv);
        }

        // 反转顺序（从旧到新）
        std::reverse(data.begin(), data.end());

        return data;

    } catch (const std::exception& e) {
        return std::unexpected(DataSourceError::Unknown);
    }
}

} // namespace quantization
```

### 实现文件数据源示例

```cpp
// include/file_data_source.hpp
#pragma once

#include "market_data_source.hpp"
#include <filesystem>

namespace quantization {

class FileDataSource : public IMarketDataSource {
public:
    explicit FileDataSource(const std::filesystem::path& data_dir);
    ~FileDataSource() override = default;

    std::expected<std::vector<OHLCV>, DataSourceError>
    fetch_ohlcv(...) override;

    std::expected<std::vector<OHLCV>, DataSourceError>
    fetch_latest(...) override;

private:
    std::filesystem::path data_dir_;

    std::expected<std::vector<OHLCV>, DataSourceError>
    read_csv_file(const std::filesystem::path& file_path);
};

} // namespace quantization
```

---

## 代码规范

### 命名约定

- **类名**: PascalCase (例: `MyIndicator`)
- **函数名**: snake_case (例: `calculate_value`)
- **变量名**: snake_case (例: `data_point`)
- **常量**: UPPER_SNAKE_CASE (例: `MAX_PERIOD`)
- **成员变量**: snake_case + 下划线后缀 (例: `period_`)

### 文件组织

```
include/
  ├── indicators/
  │   ├── indicator_base.hpp    # 基类
  │   └── specific_indicator.hpp # 具体指标
  └── market_data_source.hpp     # 数据源接口

src/
  ├── indicators/
  │   └── specific_indicator.cpp # 具体实现
  └── main.cpp                    # 主程序
```

### 注释规范

```cpp
/**
 * @brief 计算简单移动平均
 *
 * @param data OHLCV数据序列
 * @return 指标计算结果或错误
 */
std::expected<IndicatorResult, IndicatorError>
calculate(const std::vector<OHLCV>& data) override;
```

### 错误处理

优先使用 `std::expected` 而非异常：

```cpp
// 好的做法
std::expected<Result, Error> function() {
    if (error_condition) {
        return std::unexpected(Error::SomeError);
    }
    return result;
}

// 避免
Result function() {
    if (error_condition) {
        throw std::runtime_error("Error");
    }
    return result;
}
```

---

## 测试

### 单元测试框架

使用 Google Test 进行单元测试：

```cpp
// tests/test_sma.cpp
#include <gtest/gtest.h>
#include "indicators/sma.hpp"

using namespace quantization::indicators;

TEST(SMATest, BasicCalculation) {
    SMA sma(3);

    std::vector<OHLCV> data = {
        {.close = 10.0},
        {.close = 20.0},
        {.close = 30.0},
        {.close = 40.0}
    };

    auto result = sma.calculate(data);
    ASSERT_TRUE(result.has_value());
    EXPECT_EQ(result->values.size(), 2);
    EXPECT_DOUBLE_EQ(result->values[0], 20.0);  // (10+20+30)/3
    EXPECT_DOUBLE_EQ(result->values[1], 30.0);  // (20+30+40)/3
}

TEST(SMATest, InsufficientData) {
    SMA sma(5);

    std::vector<OHLCV> data = {
        {.close = 10.0},
        {.close = 20.0}
    };

    auto result = sma.calculate(data);
    ASSERT_FALSE(result.has_value());
    EXPECT_EQ(result.error(), IndicatorError::InsufficientData);
}
```

### 集成测试

```bash
# tests/integration_test.sh
#!/bin/bash

SERVER="./build/quantization-mcp"

# 测试1: 列出指标
echo '{"jsonrpc":"2.0","id":1,"method":"list_indicators","params":{}}' | \
  $SERVER | jq '.result.indicators | length'

# 测试2: 计算RSI
echo '{"jsonrpc":"2.0","id":2,"method":"calculate_indicator","params":{"indicator":"rsi_14","symbol":"TEST","count":50}}' | \
  $SERVER | jq '.result.values | length'
```

---

## 性能优化

### 1. 编译优化

```cmake
# CMakeLists.txt
set(CMAKE_CXX_FLAGS_RELEASE "-O3 -march=native -flto")
```

### 2. 并行计算

使用 OpenMP 并行化计算：

```cpp
#include <omp.h>

std::expected<IndicatorResult, IndicatorError>
calculate_parallel(const std::vector<OHLCV>& data) {
    IndicatorResult result;
    result.values.resize(data.size());

    #pragma omp parallel for
    for (size_t i = 0; i < data.size(); ++i) {
        result.values[i] = compute_value(data[i]);
    }

    return result;
}
```

### 3. 内存优化

```cpp
// 预分配内存
result.values.reserve(expected_size);
result.timestamps.reserve(expected_size);

// 使用移动语义
return std::move(result);
```

### 4. 缓存优化

```cpp
class CachedDataSource : public IMarketDataSource {
private:
    std::unordered_map<std::string, std::vector<OHLCV>> cache_;

public:
    std::expected<std::vector<OHLCV>, DataSourceError>
    fetch_latest(const std::string& symbol, ...) override {
        auto key = symbol + "_" + interval;

        if (auto it = cache_.find(key); it != cache_.end()) {
            return it->second;
        }

        auto data = fetch_from_source(symbol, ...);
        if (data) {
            cache_[key] = *data;
        }
        return data;
    }
};
```

---

## 调试技巧

### 1. 启用调试日志

```cpp
#ifdef DEBUG
    std::cerr << "Debug: " << message << std::endl;
#endif
```

### 2. 使用 GDB

```bash
gdb ./build/quantization-mcp
(gdb) break main
(gdb) run
(gdb) print variable
```

### 3. 内存检查

```bash
valgrind --leak-check=full ./build/quantization-mcp
```

### 4. 性能分析

```bash
perf record ./build/quantization-mcp
perf report
```

---

## 贡献指南

### 提交代码前检查清单

- [ ] 代码符合命名规范
- [ ] 添加了必要的注释
- [ ] 通过所有单元测试
- [ ] 更新了相关文档
- [ ] 没有内存泄漏
- [ ] 性能测试通过

### Pull Request 流程

1. Fork 项目
2. 创建特性分支 (`git checkout -b feature/my-feature`)
3. 提交更改 (`git commit -am 'Add some feature'`)
4. 推送到分支 (`git push origin feature/my-feature`)
5. 创建 Pull Request

---

## 常见问题

### Q: 如何添加多输出指标（如MACD）？

A: 创建自定义结果结构：

```cpp
struct MACDResult {
    std::vector<double> macd_line;
    std::vector<double> signal_line;
    std::vector<double> histogram;
};

class MACD : public IIndicator {
public:
    std::expected<MACDResult, IndicatorError>
    calculate_full(const std::vector<OHLCV>& data);
};
```

### Q: 如何处理实时数据流？

A: 实现 WebSocket 数据源：

```cpp
class WebSocketDataSource : public IMarketDataSource {
    // 使用 websocketpp 或类似库
};
```

### Q: 如何优化大数据量计算？

A: 使用增量计算和滑动窗口：

```cpp
class IncrementalSMA {
    double sum_ = 0.0;
    std::deque<double> window_;

    void update(double new_value) {
        sum_ += new_value;
        window_.push_back(new_value);

        if (window_.size() > period_) {
            sum_ -= window_.front();
            window_.pop_front();
        }
    }

    double get_value() const {
        return sum_ / window_.size();
    }
};
```

---

## 参考资源

- [C++23 标准](https://en.cppreference.com/w/cpp/23)
- [JSON-RPC 2.0 规范](https://www.jsonrpc.org/specification)
- [Technical Analysis Library](https://github.com/TA-Lib/ta-lib)
- [Modern C++ Design Patterns](https://refactoring.guru/design-patterns/cpp)

---

## 联系方式

- GitHub Issues: 报告 bug 和功能请求
- Discussions: 技术讨论和问答
- Email: dev@example.com

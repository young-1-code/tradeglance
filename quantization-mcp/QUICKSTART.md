# 快速开始指南

本指南帮助你在5分钟内启动并运行 Quantization MCP Server。

## 前置要求

- C++23 编译器（GCC 13+, Clang 16+, 或 MSVC 2022+）
- CMake 3.20+
- libcurl
- nlohmann-json

## 快速安装

### Ubuntu/Debian

```bash
# 1. 安装依赖
sudo apt update
sudo apt install -y build-essential cmake libcurl4-openssl-dev nlohmann-json3-dev

# 2. 克隆项目
git clone https://github.com/your-repo/quantization-mcp.git
cd quantization-mcp

# 3. 编译
chmod +x build.sh
./build.sh

# 4. 运行
./build/quantization-mcp
```

### macOS

```bash
# 1. 安装依赖
brew install cmake curl nlohmann-json

# 2. 克隆项目
git clone https://github.com/your-repo/quantization-mcp.git
cd quantization-mcp

# 3. 编译
chmod +x build.sh
./build.sh

# 4. 运行
./build/quantization-mcp
```

### Docker

```bash
# 构建镜像
docker build -t quantization-mcp .

# 运行容器
docker run -it quantization-mcp
```

## 第一个请求

启动服务器后，在另一个终端发送请求：

```bash
# 列出所有可用指标
echo '{"jsonrpc":"2.0","id":1,"method":"list_indicators","params":{}}' | ./build/quantization-mcp
```

## 常用命令

### 计算 RSI

```bash
cat <<EOF | ./build/quantization-mcp
{"jsonrpc":"2.0","id":1,"method":"calculate_indicator","params":{"indicator":"rsi_14","symbol":"AAPL","interval":"1d","count":100}}
EOF
```

### 计算 MACD

```bash
cat <<EOF | ./build/quantization-mcp
{"jsonrpc":"2.0","id":2,"method":"calculate_indicator","params":{"indicator":"macd","symbol":"AAPL","interval":"1d","count":100}}
EOF
```

### 获取原始数据

```bash
cat <<EOF | ./build/quantization-mcp
{"jsonrpc":"2.0","id":3,"method":"fetch_data","params":{"symbol":"AAPL","interval":"1d","count":50}}
EOF
```

## 使用 Python 客户端

```bash
# 运行示例客户端
python3 examples/python_client.py
```

## 配置数据源

默认使用网络数据源，可以通过环境变量配置：

```bash
export MARKET_DATA_API=http://your-api-endpoint.com/api
./build/quantization-mcp
```

## 支持的指标

- **移动平均**: SMA, EMA
- **动量指标**: RSI, MACD
- **波动率**: Bollinger Bands, ATR
- **趋势指标**: ADX, CCI
- **其他**: Stochastic, Williams %R, OBV

完整列表见 [API文档](docs/API.md)

## 下一步

- 阅读 [API文档](docs/API.md) 了解所有可用方法
- 查看 [技术指标说明](docs/INDICATORS.md) 了解各指标用法
- 参考 [开发文档](docs/DEVELOPMENT.md) 添加自定义指标

## 故障排除

### 编译错误

```bash
# 检查编译器版本
g++ --version  # 需要 13+
clang++ --version  # 需要 16+

# 检查 CMake 版本
cmake --version  # 需要 3.20+
```

### 依赖缺失

```bash
# Ubuntu/Debian
sudo apt install libcurl4-openssl-dev nlohmann-json3-dev

# macOS
brew install curl nlohmann-json
```

### 运行时错误

```bash
# 检查可执行文件
ls -l build/quantization-mcp

# 查看错误日志
./build/quantization-mcp 2>&1 | tee error.log
```

## 获取帮助

- GitHub Issues: https://github.com/your-repo/quantization-mcp/issues
- 文档: https://github.com/your-repo/quantization-mcp/docs
- 示例: https://github.com/your-repo/quantization-mcp/examples

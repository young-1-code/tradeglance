#!/bin/bash

# 示例：使用quantization-mcp服务器

SERVER_BIN="./build/quantization-mcp"
API_ENDPOINT="${MARKET_DATA_API:-http://localhost:8080/api}"

# 检查服务器是否存在
if [ ! -f "$SERVER_BIN" ]; then
    echo "Error: Server binary not found at $SERVER_BIN"
    echo "Please build the project first: mkdir build && cd build && cmake .. && make"
    exit 1
fi

# 启动服务器（后台运行）
export MARKET_DATA_API="$API_ENDPOINT"
$SERVER_BIN &
SERVER_PID=$!

echo "Server started with PID: $SERVER_PID"
echo "API Endpoint: $API_ENDPOINT"
echo ""

# 等待服务器启动
sleep 2

# 函数：发送请求
send_request() {
    local request="$1"
    echo "$request" | nc localhost 9999 2>/dev/null || echo "$request"
}

# 示例1：列出所有指标
echo "=== Example 1: List all indicators ==="
cat <<EOF | $SERVER_BIN
{"jsonrpc":"2.0","id":1,"method":"list_indicators","params":{}}
EOF
echo ""

# 示例2：计算RSI
echo "=== Example 2: Calculate RSI for AAPL ==="
cat <<EOF | $SERVER_BIN
{"jsonrpc":"2.0","id":2,"method":"calculate_indicator","params":{"indicator":"rsi_14","symbol":"AAPL","interval":"1d","count":100}}
EOF
echo ""

# 示例3：计算MACD
echo "=== Example 3: Calculate MACD for AAPL ==="
cat <<EOF | $SERVER_BIN
{"jsonrpc":"2.0","id":3,"method":"calculate_indicator","params":{"indicator":"macd","symbol":"AAPL","interval":"1d","count":100}}
EOF
echo ""

# 示例4：获取原始数据
echo "=== Example 4: Fetch raw market data ==="
cat <<EOF | $SERVER_BIN
{"jsonrpc":"2.0","id":4,"method":"fetch_data","params":{"symbol":"AAPL","interval":"1d","count":50}}
EOF
echo ""

# 清理
if [ ! -z "$SERVER_PID" ]; then
    kill $SERVER_PID 2>/dev/null
    echo "Server stopped"
fi

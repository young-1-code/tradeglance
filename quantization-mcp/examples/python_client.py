#!/usr/bin/env python3
"""
Python客户端示例 - 与quantization-mcp服务器交互
"""

import json
import subprocess
import sys
from typing import Dict, List, Any, Optional


class QuantizationMCPClient:
    """MCP客户端类"""

    def __init__(self, server_path: str = "./build/quantization-mcp",
                 api_endpoint: Optional[str] = None):
        """
        初始化客户端

        Args:
            server_path: 服务器可执行文件路径
            api_endpoint: 市场数据API端点
        """
        self.server_path = server_path
        self.request_id = 0

        # 设置环境变量
        env = {}
        if api_endpoint:
            env['MARKET_DATA_API'] = api_endpoint

        # 启动服务器进程
        self.process = subprocess.Popen(
            [server_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env
        )

    def _send_request(self, method: str, params: Dict[str, Any]) -> Dict[str, Any]:
        """发送JSON-RPC请求"""
        self.request_id += 1
        request = {
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": method,
            "params": params
        }

        # 发送请求
        request_json = json.dumps(request) + "\n"
        self.process.stdin.write(request_json)
        self.process.stdin.flush()

        # 读取响应
        response_line = self.process.stdout.readline()
        response = json.loads(response_line)

        if "error" in response:
            raise Exception(f"Server error: {response['error']['message']}")

        return response.get("result", {})

    def list_indicators(self) -> List[Dict[str, Any]]:
        """列出所有可用的技术指标"""
        result = self._send_request("list_indicators", {})
        return result.get("indicators", [])

    def calculate_indicator(self, indicator: str, symbol: str,
                          interval: str = "1d", count: int = 100) -> Dict[str, Any]:
        """
        计算技术指标

        Args:
            indicator: 指标名称（如 "rsi_14", "macd"）
            symbol: 股票代码
            interval: 时间间隔（如 "1d", "1h"）
            count: 数据点数量

        Returns:
            包含指标值的字典
        """
        params = {
            "indicator": indicator,
            "symbol": symbol,
            "interval": interval,
            "count": count
        }
        return self._send_request("calculate_indicator", params)

    def fetch_data(self, symbol: str, interval: str = "1d",
                   count: int = 100) -> Dict[str, Any]:
        """
        获取原始市场数据

        Args:
            symbol: 股票代码
            interval: 时间间隔
            count: 数据点数量

        Returns:
            包含OHLCV数据的字典
        """
        params = {
            "symbol": symbol,
            "interval": interval,
            "count": count
        }
        return self._send_request("fetch_data", params)

    def close(self):
        """关闭客户端连接"""
        if self.process:
            self.process.terminate()
            self.process.wait()


def main():
    """主函数 - 示例用法"""

    # 创建客户端
    client = QuantizationMCPClient(
        server_path="./build/quantization-mcp",
        api_endpoint="http://localhost:8080/api"
    )

    try:
        # 示例1：列出所有指标
        print("=== 可用的技术指标 ===")
        indicators = client.list_indicators()
        for ind in indicators[:5]:  # 只显示前5个
            print(f"  - {ind['name']}: {ind['display_name']} "
                  f"(最少需要 {ind['min_data_points']} 个数据点)")
        print(f"  ... 共 {len(indicators)} 个指标\n")

        # 示例2：计算RSI
        print("=== 计算 RSI (AAPL) ===")
        rsi_result = client.calculate_indicator("rsi_14", "AAPL", "1d", 100)
        print(f"指标: {rsi_result['indicator']}")
        print(f"股票: {rsi_result['symbol']}")
        print(f"最新值: {rsi_result['values'][-1]}\n")

        # 示例3：计算MACD
        print("=== 计算 MACD (AAPL) ===")
        macd_result = client.calculate_indicator("macd", "AAPL", "1d", 100)
        print(f"指标: {macd_result['indicator']}")
        print(f"数据点数量: {len(macd_result['values'])}")
        print(f"最新值: {macd_result['values'][-1]}\n")

        # 示例4：计算布林带
        print("=== 计算 Bollinger Bands (AAPL) ===")
        bb_result = client.calculate_indicator("bb_20", "AAPL", "1d", 100)
        print(f"指标: {bb_result['indicator']}")
        print(f"最新值: {bb_result['values'][-1]}\n")

        # 示例5：获取原始数据
        print("=== 获取原始市场数据 (AAPL) ===")
        data_result = client.fetch_data("AAPL", "1d", 10)
        print(f"股票: {data_result['symbol']}")
        print(f"数据点数量: {len(data_result['data'])}")
        if data_result['data']:
            latest = data_result['data'][-1]
            print(f"最新数据: 开={latest['open']}, 高={latest['high']}, "
                  f"低={latest['low']}, 收={latest['close']}, "
                  f"量={latest['volume']}\n")

    except Exception as e:
        print(f"错误: {e}", file=sys.stderr)
    finally:
        # 关闭客户端
        client.close()


if __name__ == "__main__":
    main()

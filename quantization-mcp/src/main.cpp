#include "mcp_server.hpp"
#include "network_data_source.hpp"
#include "indicators/sma.hpp"
#include "indicators/ema.hpp"
#include "indicators/rsi.hpp"
#include "indicators/macd.hpp"
#include "indicators/bollinger_bands.hpp"
#include "indicators/stochastic.hpp"
#include "indicators/atr.hpp"
#include "indicators/adx.hpp"
#include "indicators/cci.hpp"
#include "indicators/williams_r.hpp"
#include "indicators/obv.hpp"
#include <iostream>
#include <memory>
#include <cstdlib>

using namespace quantization;
using namespace quantization::indicators;

int main(int argc, char* argv[]) {
    try {
        // 从环境变量或命令行参数获取API端点
        std::string api_endpoint = "http://localhost:8080/api";
        if (const char* env_endpoint = std::getenv("MARKET_DATA_API")) {
            api_endpoint = env_endpoint;
        } else if (argc > 1) {
            api_endpoint = argv[1];
        }

        std::cerr << "Initializing Quantization MCP Server..." << std::endl;
        std::cerr << "Market Data API: " << api_endpoint << std::endl;

        // 创建数据源
        auto data_source = std::make_shared<NetworkDataSource>(api_endpoint);

        // 创建MCP服务器
        MCPServer server(data_source);

        // 注册所有技术指标
        std::cerr << "Registering technical indicators..." << std::endl;

        // 移动平均指标
        server.register_indicator("sma_5", std::make_shared<SMA>(5));
        server.register_indicator("sma_10", std::make_shared<SMA>(10));
        server.register_indicator("sma_20", std::make_shared<SMA>(20));
        server.register_indicator("sma_50", std::make_shared<SMA>(50));
        server.register_indicator("sma_200", std::make_shared<SMA>(200));

        server.register_indicator("ema_5", std::make_shared<EMA>(5));
        server.register_indicator("ema_10", std::make_shared<EMA>(10));
        server.register_indicator("ema_12", std::make_shared<EMA>(12));
        server.register_indicator("ema_20", std::make_shared<EMA>(20));
        server.register_indicator("ema_26", std::make_shared<EMA>(26));
        server.register_indicator("ema_50", std::make_shared<EMA>(50));

        // 动量指标
        server.register_indicator("rsi_14", std::make_shared<RSI>(14));
        server.register_indicator("rsi_9", std::make_shared<RSI>(9));

        // MACD
        server.register_indicator("macd", std::make_shared<MACD>(12, 26, 9));
        server.register_indicator("macd_fast", std::make_shared<MACD>(5, 13, 5));

        // 波动率指标
        server.register_indicator("bb_20", std::make_shared<BollingerBands>(20, 2.0));
        server.register_indicator("bb_20_3std", std::make_shared<BollingerBands>(20, 3.0));
        server.register_indicator("atr_14", std::make_shared<ATR>(14));

        // 随机指标
        server.register_indicator("stoch_14_3", std::make_shared<StochasticOscillator>(14, 3));
        server.register_indicator("stoch_5_3", std::make_shared<StochasticOscillator>(5, 3));

        // 趋势指标
        server.register_indicator("adx_14", std::make_shared<ADX>(14));
        server.register_indicator("cci_20", std::make_shared<CCI>(20));
        server.register_indicator("cci_14", std::make_shared<CCI>(14));

        // 其他指标
        server.register_indicator("williams_r_14", std::make_shared<WilliamsR>(14));
        server.register_indicator("obv", std::make_shared<OBV>());

        std::cerr << "Server initialized successfully!" << std::endl;
        std::cerr << "Ready to process requests via stdin/stdout" << std::endl;

        // 运行服务器
        server.run();

        return 0;

    } catch (const std::exception& e) {
        std::cerr << "Fatal error: " << e.what() << std::endl;
        return 1;
    }
}

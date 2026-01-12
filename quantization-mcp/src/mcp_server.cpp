#include "mcp_server.hpp"
#include <iostream>
#include <sstream>

namespace quantization {

MCPServer::MCPServer(std::shared_ptr<IMarketDataSource> data_source)
    : data_source_(std::move(data_source)) {}

void MCPServer::register_indicator(const std::string& name,
                                   std::shared_ptr<indicators::IIndicator> indicator) {
    indicators_[name] = std::move(indicator);
}

json MCPServer::create_error_response(const std::string& message) {
    return {
        {"jsonrpc", "2.0"},
        {"error", {
            {"code", -1},
            {"message", message}
        }}
    };
}

json MCPServer::create_success_response(const json& data) {
    return {
        {"jsonrpc", "2.0"},
        {"result", data}
    };
}

json MCPServer::handle_list_indicators(const json& /* params */) {
    json indicator_list = json::array();

    for (const auto& [name, indicator] : indicators_) {
        indicator_list.push_back({
            {"name", name},
            {"display_name", indicator->name()},
            {"min_data_points", indicator->min_data_points()}
        });
    }

    return create_success_response({
        {"indicators", indicator_list}
    });
}

json MCPServer::handle_calculate_indicator(const json& params) {
    try {
        // 验证参数
        if (!params.contains("indicator") || !params.contains("symbol")) {
            return create_error_response("Missing required parameters: indicator, symbol");
        }

        std::string indicator_name = params["indicator"];
        std::string symbol = params["symbol"];
        std::string interval = params.value("interval", "1d");
        size_t count = params.value("count", 100);

        // 查找指标
        auto it = indicators_.find(indicator_name);
        if (it == indicators_.end()) {
            return create_error_response("Indicator not found: " + indicator_name);
        }

        // 获取市场数据
        auto data_result = data_source_->fetch_latest(symbol, interval, count);
        if (!data_result) {
            return create_error_response("Failed to fetch market data");
        }

        // 计算指标
        auto calc_result = it->second->calculate(*data_result);
        if (!calc_result) {
            return create_error_response("Failed to calculate indicator");
        }

        // 构建响应
        json values = json::array();
        for (size_t i = 0; i < calc_result->values.size(); ++i) {
            auto timestamp = std::chrono::system_clock::to_time_t(calc_result->timestamps[i]);
            values.push_back({
                {"timestamp", timestamp},
                {"value", calc_result->values[i]}
            });
        }

        return create_success_response({
            {"indicator", indicator_name},
            {"symbol", symbol},
            {"interval", interval},
            {"values", values}
        });

    } catch (const std::exception& e) {
        return create_error_response(std::string("Exception: ") + e.what());
    }
}

json MCPServer::handle_fetch_data(const json& params) {
    try {
        if (!params.contains("symbol")) {
            return create_error_response("Missing required parameter: symbol");
        }

        std::string symbol = params["symbol"];
        std::string interval = params.value("interval", "1d");
        size_t count = params.value("count", 100);

        auto data_result = data_source_->fetch_latest(symbol, interval, count);
        if (!data_result) {
            return create_error_response("Failed to fetch market data");
        }

        json data_array = json::array();
        for (const auto& ohlcv : *data_result) {
            auto timestamp = std::chrono::system_clock::to_time_t(ohlcv.timestamp);
            data_array.push_back({
                {"timestamp", timestamp},
                {"open", ohlcv.open},
                {"high", ohlcv.high},
                {"low", ohlcv.low},
                {"close", ohlcv.close},
                {"volume", ohlcv.volume}
            });
        }

        return create_success_response({
            {"symbol", symbol},
            {"interval", interval},
            {"data", data_array}
        });

    } catch (const std::exception& e) {
        return create_error_response(std::string("Exception: ") + e.what());
    }
}

json MCPServer::handle_request(const json& request) {
    try {
        if (!request.contains("method")) {
            return create_error_response("Missing method field");
        }

        std::string method = request["method"];
        json params = request.value("params", json::object());

        if (method == "list_indicators") {
            return handle_list_indicators(params);
        } else if (method == "calculate_indicator") {
            return handle_calculate_indicator(params);
        } else if (method == "fetch_data") {
            return handle_fetch_data(params);
        } else {
            return create_error_response("Unknown method: " + method);
        }

    } catch (const std::exception& e) {
        return create_error_response(std::string("Exception: ") + e.what());
    }
}

void MCPServer::run() {
    std::cout << "Quantization MCP Server started. Listening for requests..." << std::endl;
    std::cout << "Registered indicators: " << indicators_.size() << std::endl;

    std::string line;
    while (std::getline(std::cin, line)) {
        if (line.empty()) {
            continue;
        }

        try {
            auto request = json::parse(line);
            auto response = handle_request(request);

            // 添加请求ID到响应
            if (request.contains("id")) {
                response["id"] = request["id"];
            }

            std::cout << response.dump() << std::endl;

        } catch (const json::exception& e) {
            auto error_response = create_error_response(std::string("JSON parse error: ") + e.what());
            std::cout << error_response.dump() << std::endl;
        }
    }
}

} // namespace quantization

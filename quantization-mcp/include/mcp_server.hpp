#pragma once

#include "market_data_source.hpp"
#include "indicators/indicator_base.hpp"
#include <memory>
#include <string>
#include <map>
#include <nlohmann/json.hpp>

namespace quantization {

using json = nlohmann::json;

class MCPServer {
public:
    explicit MCPServer(std::shared_ptr<IMarketDataSource> data_source);
    ~MCPServer() = default;

    // 注册技术指标
    void register_indicator(const std::string& name,
                           std::shared_ptr<indicators::IIndicator> indicator);

    // 处理MCP请求
    json handle_request(const json& request);

    // 启动服务器
    void run();

private:
    std::shared_ptr<IMarketDataSource> data_source_;
    std::map<std::string, std::shared_ptr<indicators::IIndicator>> indicators_;

    // 处理不同类型的请求
    json handle_list_indicators(const json& params);
    json handle_calculate_indicator(const json& params);
    json handle_fetch_data(const json& params);

    // 工具函数
    json create_error_response(const std::string& message);
    json create_success_response(const json& data);
};

} // namespace quantization

#pragma once

#include "market_data_source.hpp"
#include <string>
#include <memory>

namespace quantization {

class NetworkDataSource : public IMarketDataSource {
public:
    explicit NetworkDataSource(const std::string& api_endpoint);
    ~NetworkDataSource() override = default;

    quantization::expected<std::vector<OHLCV>, DataSourceError>
    fetch_ohlcv(
        const std::string& symbol,
        const std::string& interval,
        std::chrono::system_clock::time_point start,
        std::chrono::system_clock::time_point end
    ) override;

    quantization::expected<std::vector<OHLCV>, DataSourceError>
    fetch_latest(
        const std::string& symbol,
        const std::string& interval,
        size_t count
    ) override;

private:
    std::string api_endpoint_;

    quantization::expected<std::string, DataSourceError>
    http_get(const std::string& url);

    quantization::expected<std::vector<OHLCV>, DataSourceError>
    parse_response(const std::string& json_data);
};

} // namespace quantization

#pragma once

#include <vector>
#include <string>
#include <memory>
#include <chrono>
#include "compat/expected.hpp"

namespace quantization {

struct OHLCV {
    std::chrono::system_clock::time_point timestamp;
    double open;
    double high;
    double low;
    double close;
    double volume;
};

enum class DataSourceError {
    NetworkError,
    InvalidSymbol,
    InvalidTimeRange,
    ParseError,
    Timeout,
    Unknown
};

class IMarketDataSource {
public:
    virtual ~IMarketDataSource() = default;

    virtual expected<std::vector<OHLCV>, DataSourceError>
    fetch_ohlcv(
        const std::string& symbol,
        const std::string& interval,
        std::chrono::system_clock::time_point start,
        std::chrono::system_clock::time_point end
    ) = 0;

    virtual expected<std::vector<OHLCV>, DataSourceError>
    fetch_latest(
        const std::string& symbol,
        const std::string& interval,
        size_t count
    ) = 0;
};

} // namespace quantization

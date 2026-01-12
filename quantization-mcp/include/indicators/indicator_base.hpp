#pragma once

#include "../market_data_source.hpp"
#include <vector>
#include <string>
#include <optional>

namespace quantization::indicators {

enum class IndicatorError {
    InsufficientData,
    InvalidParameter,
    CalculationError
};

struct IndicatorResult {
    std::vector<double> values;
    std::vector<std::chrono::system_clock::time_point> timestamps;
    std::string indicator_name;
};

class IIndicator {
public:
    virtual ~IIndicator() = default;

    virtual quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) = 0;

    virtual std::string name() const = 0;
    virtual size_t min_data_points() const = 0;
};

} // namespace quantization::indicators

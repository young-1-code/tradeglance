#include "indicators/williams_r.hpp"
#include <algorithm>

namespace quantization::indicators {

WilliamsR::WilliamsR(size_t period) : period_(period) {
    if (period == 0) {
        throw std::invalid_argument("Williams %R period must be greater than 0");
    }
}

quantization::expected<IndicatorResult, IndicatorError>
WilliamsR::calculate(const std::vector<OHLCV>& data) {
    if (data.size() < period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values.reserve(data.size() - period_ + 1);
    result.timestamps.reserve(data.size() - period_ + 1);

    for (size_t i = period_ - 1; i < data.size(); ++i) {
        // 找到周期内的最高价和最低价
        double highest_high = data[i - period_ + 1].high;
        double lowest_low = data[i - period_ + 1].low;

        for (size_t j = i - period_ + 2; j <= i; ++j) {
            highest_high = std::max(highest_high, data[j].high);
            lowest_low = std::min(lowest_low, data[j].low);
        }

        // 计算Williams %R
        double williams_r = 0.0;
        if (highest_high != lowest_low) {
            williams_r = ((highest_high - data[i].close) / (highest_high - lowest_low)) * -100.0;
        }

        result.values.push_back(williams_r);
        result.timestamps.push_back(data[i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

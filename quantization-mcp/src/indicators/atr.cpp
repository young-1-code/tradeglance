#include "indicators/atr.hpp"
#include <algorithm>
#include <cmath>

namespace quantization::indicators {

ATR::ATR(size_t period) : period_(period) {
    if (period == 0) {
        throw std::invalid_argument("ATR period must be greater than 0");
    }
}

double ATR::calculate_true_range(const OHLCV& current, const OHLCV& previous) {
    double hl = current.high - current.low;
    double hc = std::abs(current.high - previous.close);
    double lc = std::abs(current.low - previous.close);
    return std::max({hl, hc, lc});
}

quantization::expected<IndicatorResult, IndicatorError>
ATR::calculate(const std::vector<OHLCV>& data) {
    if (data.size() < period_ + 1) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values.reserve(data.size() - period_);
    result.timestamps.reserve(data.size() - period_);

    // 计算初始ATR（前period个真实波幅的平均值）
    double sum = 0.0;
    for (size_t i = 1; i <= period_; ++i) {
        sum += calculate_true_range(data[i], data[i - 1]);
    }
    double atr = sum / static_cast<double>(period_);
    result.values.push_back(atr);
    result.timestamps.push_back(data[period_].timestamp);

    // 使用平滑方法计算后续ATR值
    for (size_t i = period_ + 1; i < data.size(); ++i) {
        double tr = calculate_true_range(data[i], data[i - 1]);
        atr = ((atr * (period_ - 1)) + tr) / static_cast<double>(period_);
        result.values.push_back(atr);
        result.timestamps.push_back(data[i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

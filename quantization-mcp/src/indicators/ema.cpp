#include "indicators/ema.hpp"

namespace quantization::indicators {

EMA::EMA(size_t period) : period_(period) {
    if (period == 0) {
        throw std::invalid_argument("EMA period must be greater than 0");
    }
    multiplier_ = 2.0 / (period + 1.0);
}

quantization::expected<IndicatorResult, IndicatorError>
EMA::calculate(const std::vector<OHLCV>& data) {
    if (data.size() < period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values.reserve(data.size());
    result.timestamps.reserve(data.size());

    // 计算初始SMA作为第一个EMA值
    double sum = 0.0;
    for (size_t i = 0; i < period_; ++i) {
        sum += data[i].close;
    }
    double ema = sum / static_cast<double>(period_);
    result.values.push_back(ema);
    result.timestamps.push_back(data[period_ - 1].timestamp);

    // 计算后续EMA值
    for (size_t i = period_; i < data.size(); ++i) {
        ema = (data[i].close - ema) * multiplier_ + ema;
        result.values.push_back(ema);
        result.timestamps.push_back(data[i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

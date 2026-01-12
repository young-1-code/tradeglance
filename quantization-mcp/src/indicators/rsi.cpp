#include "indicators/rsi.hpp"
#include <cmath>

namespace quantization::indicators {

RSI::RSI(size_t period) : period_(period) {
    if (period == 0) {
        throw std::invalid_argument("RSI period must be greater than 0");
    }
}

quantization::expected<IndicatorResult, IndicatorError>
RSI::calculate(const std::vector<OHLCV>& data) {
    if (data.size() < period_ + 1) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values.reserve(data.size() - period_);
    result.timestamps.reserve(data.size() - period_);

    // 计算价格变化
    std::vector<double> gains, losses;
    for (size_t i = 1; i < data.size(); ++i) {
        double change = data[i].close - data[i - 1].close;
        gains.push_back(change > 0 ? change : 0.0);
        losses.push_back(change < 0 ? -change : 0.0);
    }

    // 计算初始平均增益和损失
    double avg_gain = 0.0, avg_loss = 0.0;
    for (size_t i = 0; i < period_; ++i) {
        avg_gain += gains[i];
        avg_loss += losses[i];
    }
    avg_gain /= period_;
    avg_loss /= period_;

    // 计算第一个RSI值
    double rs = (avg_loss == 0.0) ? 100.0 : avg_gain / avg_loss;
    double rsi = 100.0 - (100.0 / (1.0 + rs));
    result.values.push_back(rsi);
    result.timestamps.push_back(data[period_].timestamp);

    // 计算后续RSI值（使用平滑方法）
    for (size_t i = period_; i < gains.size(); ++i) {
        avg_gain = (avg_gain * (period_ - 1) + gains[i]) / period_;
        avg_loss = (avg_loss * (period_ - 1) + losses[i]) / period_;

        rs = (avg_loss == 0.0) ? 100.0 : avg_gain / avg_loss;
        rsi = 100.0 - (100.0 / (1.0 + rs));

        result.values.push_back(rsi);
        result.timestamps.push_back(data[i + 1].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

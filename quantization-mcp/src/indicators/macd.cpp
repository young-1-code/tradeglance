#include "indicators/macd.hpp"
#include "indicators/ema.hpp"

namespace quantization::indicators {

MACD::MACD(size_t fast_period, size_t slow_period, size_t signal_period)
    : fast_period_(fast_period), slow_period_(slow_period), signal_period_(signal_period) {
    if (fast_period >= slow_period) {
        throw std::invalid_argument("Fast period must be less than slow period");
    }
    if (signal_period == 0) {
        throw std::invalid_argument("Signal period must be greater than 0");
    }
}

quantization::expected<IndicatorResult, IndicatorError>
MACD::calculate(const std::vector<OHLCV>& data) {
    auto full_result = calculate_full(data);
    if (!full_result) {
        return quantization::unexpected(full_result.error());
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values = std::move(full_result->macd_line);
    result.timestamps = std::move(full_result->timestamps);

    return result;
}

quantization::expected<MACDResult, IndicatorError>
MACD::calculate_full(const std::vector<OHLCV>& data) {
    if (data.size() < slow_period_ + signal_period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    // 计算快速和慢速EMA
    EMA fast_ema(fast_period_);
    EMA slow_ema(slow_period_);

    auto fast_result = fast_ema.calculate(data);
    auto slow_result = slow_ema.calculate(data);

    if (!fast_result || !slow_result) {
        return quantization::unexpected(IndicatorError::CalculationError);
    }

    // 计算MACD线（快速EMA - 慢速EMA）
    MACDResult result;
    size_t start_idx = slow_period_ - fast_period_;

    for (size_t i = start_idx; i < fast_result->values.size(); ++i) {
        double macd_value = fast_result->values[i] - slow_result->values[i - start_idx];
        result.macd_line.push_back(macd_value);
        result.timestamps.push_back(fast_result->timestamps[i]);
    }

    // 计算信号线（MACD的EMA）
    if (result.macd_line.size() < signal_period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    // 手动计算信号线的EMA
    double multiplier = 2.0 / (signal_period_ + 1.0);

    // 初始SMA
    double sum = 0.0;
    for (size_t i = 0; i < signal_period_; ++i) {
        sum += result.macd_line[i];
    }
    double signal = sum / signal_period_;
    result.signal_line.push_back(signal);
    result.histogram.push_back(result.macd_line[signal_period_ - 1] - signal);

    // 后续EMA
    for (size_t i = signal_period_; i < result.macd_line.size(); ++i) {
        signal = (result.macd_line[i] - signal) * multiplier + signal;
        result.signal_line.push_back(signal);
        result.histogram.push_back(result.macd_line[i] - signal);
    }

    // 调整时间戳
    result.timestamps.erase(result.timestamps.begin(),
                           result.timestamps.begin() + signal_period_ - 1);

    // 调整MACD线
    result.macd_line.erase(result.macd_line.begin(),
                          result.macd_line.begin() + signal_period_ - 1);

    return result;
}

} // namespace quantization::indicators

#include "indicators/stochastic.hpp"
#include <algorithm>

namespace quantization::indicators {

StochasticOscillator::StochasticOscillator(size_t k_period, size_t d_period)
    : k_period_(k_period), d_period_(d_period) {
    if (k_period == 0 || d_period == 0) {
        throw std::invalid_argument("Stochastic periods must be greater than 0");
    }
}

quantization::expected<IndicatorResult, IndicatorError>
StochasticOscillator::calculate(const std::vector<OHLCV>& data) {
    auto full_result = calculate_full(data);
    if (!full_result) {
        return quantization::unexpected(full_result.error());
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values = std::move(full_result->k_line);
    result.timestamps = std::move(full_result->timestamps);

    return result;
}

quantization::expected<StochasticResult, IndicatorError>
StochasticOscillator::calculate_full(const std::vector<OHLCV>& data) {
    if (data.size() < k_period_ + d_period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    StochasticResult result;
    std::vector<double> k_values;

    // 计算%K值
    for (size_t i = k_period_ - 1; i < data.size(); ++i) {
        double highest_high = data[i - k_period_ + 1].high;
        double lowest_low = data[i - k_period_ + 1].low;

        for (size_t j = i - k_period_ + 2; j <= i; ++j) {
            highest_high = std::max(highest_high, data[j].high);
            lowest_low = std::min(lowest_low, data[j].low);
        }

        double k_value = 0.0;
        if (highest_high != lowest_low) {
            k_value = ((data[i].close - lowest_low) / (highest_high - lowest_low)) * 100.0;
        }

        k_values.push_back(k_value);
    }

    // 计算%D值（%K的SMA）
    for (size_t i = d_period_ - 1; i < k_values.size(); ++i) {
        double sum = 0.0;
        for (size_t j = i - d_period_ + 1; j <= i; ++j) {
            sum += k_values[j];
        }
        double d_value = sum / static_cast<double>(d_period_);

        result.k_line.push_back(k_values[i]);
        result.d_line.push_back(d_value);
        result.timestamps.push_back(data[k_period_ - 1 + i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

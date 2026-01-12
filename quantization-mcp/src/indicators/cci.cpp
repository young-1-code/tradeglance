#include "indicators/cci.hpp"
#include <cmath>

namespace quantization::indicators {

CCI::CCI(size_t period) : period_(period) {
    if (period == 0) {
        throw std::invalid_argument("CCI period must be greater than 0");
    }
}

quantization::expected<IndicatorResult, IndicatorError>
CCI::calculate(const std::vector<OHLCV>& data) {
    if (data.size() < period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values.reserve(data.size() - period_ + 1);
    result.timestamps.reserve(data.size() - period_ + 1);

    for (size_t i = period_ - 1; i < data.size(); ++i) {
        // 计算典型价格（Typical Price）
        std::vector<double> typical_prices;
        for (size_t j = i - period_ + 1; j <= i; ++j) {
            double tp = (data[j].high + data[j].low + data[j].close) / 3.0;
            typical_prices.push_back(tp);
        }

        // 计算典型价格的SMA
        double sum = 0.0;
        for (double tp : typical_prices) {
            sum += tp;
        }
        double sma_tp = sum / static_cast<double>(period_);

        // 计算平均偏差（Mean Deviation）
        double mad = 0.0;
        for (double tp : typical_prices) {
            mad += std::abs(tp - sma_tp);
        }
        mad /= static_cast<double>(period_);

        // 计算CCI
        double current_tp = typical_prices.back();
        double cci = (mad != 0.0) ? (current_tp - sma_tp) / (constant_ * mad) : 0.0;

        result.values.push_back(cci);
        result.timestamps.push_back(data[i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

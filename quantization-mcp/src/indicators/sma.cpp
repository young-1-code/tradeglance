#include "indicators/sma.hpp"
#include <numeric>

namespace quantization::indicators {

SMA::SMA(size_t period) : period_(period) {
    if (period == 0) {
        throw std::invalid_argument("SMA period must be greater than 0");
    }
}

quantization::expected<IndicatorResult, IndicatorError>
SMA::calculate(const std::vector<OHLCV>& data) {
    if (data.size() < period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values.reserve(data.size() - period_ + 1);
    result.timestamps.reserve(data.size() - period_ + 1);

    for (size_t i = period_ - 1; i < data.size(); ++i) {
        double sum = 0.0;
        for (size_t j = i - period_ + 1; j <= i; ++j) {
            sum += data[j].close;
        }
        double sma = sum / static_cast<double>(period_);

        result.values.push_back(sma);
        result.timestamps.push_back(data[i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

#include "indicators/bollinger_bands.hpp"
#include <cmath>
#include <numeric>

namespace quantization::indicators {

BollingerBands::BollingerBands(size_t period, double std_dev)
    : period_(period), std_dev_(std_dev) {
    if (period == 0) {
        throw std::invalid_argument("Bollinger Bands period must be greater than 0");
    }
    if (std_dev <= 0) {
        throw std::invalid_argument("Standard deviation must be greater than 0");
    }
}

quantization::expected<IndicatorResult, IndicatorError>
BollingerBands::calculate(const std::vector<OHLCV>& data) {
    auto full_result = calculate_full(data);
    if (!full_result) {
        return quantization::unexpected(full_result.error());
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values = std::move(full_result->middle_band);
    result.timestamps = std::move(full_result->timestamps);

    return result;
}

quantization::expected<BollingerBandsResult, IndicatorError>
BollingerBands::calculate_full(const std::vector<OHLCV>& data) {
    if (data.size() < period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    BollingerBandsResult result;
    result.upper_band.reserve(data.size() - period_ + 1);
    result.middle_band.reserve(data.size() - period_ + 1);
    result.lower_band.reserve(data.size() - period_ + 1);
    result.timestamps.reserve(data.size() - period_ + 1);

    for (size_t i = period_ - 1; i < data.size(); ++i) {
        // 计算SMA（中轨）
        double sum = 0.0;
        for (size_t j = i - period_ + 1; j <= i; ++j) {
            sum += data[j].close;
        }
        double sma = sum / static_cast<double>(period_);

        // 计算标准差
        double variance = 0.0;
        for (size_t j = i - period_ + 1; j <= i; ++j) {
            double diff = data[j].close - sma;
            variance += diff * diff;
        }
        double std = std::sqrt(variance / static_cast<double>(period_));

        // 计算上下轨
        double upper = sma + (std_dev_ * std);
        double lower = sma - (std_dev_ * std);

        result.middle_band.push_back(sma);
        result.upper_band.push_back(upper);
        result.lower_band.push_back(lower);
        result.timestamps.push_back(data[i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

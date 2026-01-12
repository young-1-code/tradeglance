#include "indicators/adx.hpp"
#include <algorithm>
#include <cmath>

namespace quantization::indicators {

ADX::ADX(size_t period) : period_(period) {
    if (period == 0) {
        throw std::invalid_argument("ADX period must be greater than 0");
    }
}

quantization::expected<IndicatorResult, IndicatorError>
ADX::calculate(const std::vector<OHLCV>& data) {
    auto full_result = calculate_full(data);
    if (!full_result) {
        return quantization::unexpected(full_result.error());
    }

    IndicatorResult result;
    result.indicator_name = name();
    result.values = std::move(full_result->adx);
    result.timestamps = std::move(full_result->timestamps);

    return result;
}

quantization::expected<ADXResult, IndicatorError>
ADX::calculate_full(const std::vector<OHLCV>& data) {
    if (data.size() < period_ * 2) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    ADXResult result;
    std::vector<double> tr_values, plus_dm, minus_dm;

    // 计算TR, +DM, -DM
    for (size_t i = 1; i < data.size(); ++i) {
        // True Range
        double hl = data[i].high - data[i].low;
        double hc = std::abs(data[i].high - data[i - 1].close);
        double lc = std::abs(data[i].low - data[i - 1].close);
        double tr = std::max({hl, hc, lc});
        tr_values.push_back(tr);

        // Directional Movement
        double high_diff = data[i].high - data[i - 1].high;
        double low_diff = data[i - 1].low - data[i].low;

        double plus_dm_val = 0.0, minus_dm_val = 0.0;
        if (high_diff > low_diff && high_diff > 0) {
            plus_dm_val = high_diff;
        }
        if (low_diff > high_diff && low_diff > 0) {
            minus_dm_val = low_diff;
        }

        plus_dm.push_back(plus_dm_val);
        minus_dm.push_back(minus_dm_val);
    }

    // 计算平滑的TR, +DM, -DM
    std::vector<double> smoothed_tr, smoothed_plus_dm, smoothed_minus_dm;

    // 初始值
    double sum_tr = 0.0, sum_plus_dm = 0.0, sum_minus_dm = 0.0;
    for (size_t i = 0; i < period_; ++i) {
        sum_tr += tr_values[i];
        sum_plus_dm += plus_dm[i];
        sum_minus_dm += minus_dm[i];
    }

    smoothed_tr.push_back(sum_tr);
    smoothed_plus_dm.push_back(sum_plus_dm);
    smoothed_minus_dm.push_back(sum_minus_dm);

    // 平滑后续值
    for (size_t i = period_; i < tr_values.size(); ++i) {
        smoothed_tr.push_back(smoothed_tr.back() - (smoothed_tr.back() / period_) + tr_values[i]);
        smoothed_plus_dm.push_back(smoothed_plus_dm.back() - (smoothed_plus_dm.back() / period_) + plus_dm[i]);
        smoothed_minus_dm.push_back(smoothed_minus_dm.back() - (smoothed_minus_dm.back() / period_) + minus_dm[i]);
    }

    // 计算+DI和-DI
    std::vector<double> plus_di, minus_di, dx;
    for (size_t i = 0; i < smoothed_tr.size(); ++i) {
        double plus_di_val = (smoothed_tr[i] != 0) ? (smoothed_plus_dm[i] / smoothed_tr[i]) * 100.0 : 0.0;
        double minus_di_val = (smoothed_tr[i] != 0) ? (smoothed_minus_dm[i] / smoothed_tr[i]) * 100.0 : 0.0;

        plus_di.push_back(plus_di_val);
        minus_di.push_back(minus_di_val);

        // 计算DX
        double di_sum = plus_di_val + minus_di_val;
        double di_diff = std::abs(plus_di_val - minus_di_val);
        double dx_val = (di_sum != 0) ? (di_diff / di_sum) * 100.0 : 0.0;
        dx.push_back(dx_val);
    }

    // 计算ADX（DX的平滑移动平均）
    if (dx.size() < period_) {
        return quantization::unexpected(IndicatorError::InsufficientData);
    }

    // 初始ADX
    double sum_dx = 0.0;
    for (size_t i = 0; i < period_; ++i) {
        sum_dx += dx[i];
    }
    double adx = sum_dx / period_;
    result.adx.push_back(adx);
    result.plus_di.push_back(plus_di[period_ - 1]);
    result.minus_di.push_back(minus_di[period_ - 1]);
    result.timestamps.push_back(data[period_ * 2 - 1].timestamp);

    // 平滑后续ADX值
    for (size_t i = period_; i < dx.size(); ++i) {
        adx = ((adx * (period_ - 1)) + dx[i]) / period_;
        result.adx.push_back(adx);
        result.plus_di.push_back(plus_di[i]);
        result.minus_di.push_back(minus_di[i]);
        result.timestamps.push_back(data[period_ + i].timestamp);
    }

    return result;
}

} // namespace quantization::indicators

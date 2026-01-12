#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

struct MACDResult {
    std::vector<double> macd_line;
    std::vector<double> signal_line;
    std::vector<double> histogram;
    std::vector<std::chrono::system_clock::time_point> timestamps;
};

class MACD : public IIndicator {
public:
    explicit MACD(size_t fast_period = 12, size_t slow_period = 26, size_t signal_period = 9);
    ~MACD() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    quantization::expected<MACDResult, IndicatorError>
    calculate_full(const std::vector<OHLCV>& data);

    std::string name() const override { return "MACD"; }
    size_t min_data_points() const override { return slow_period_ + signal_period_; }

private:
    size_t fast_period_;
    size_t slow_period_;
    size_t signal_period_;
};

} // namespace quantization::indicators

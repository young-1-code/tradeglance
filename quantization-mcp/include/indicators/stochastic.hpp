#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

struct StochasticResult {
    std::vector<double> k_line;
    std::vector<double> d_line;
    std::vector<std::chrono::system_clock::time_point> timestamps;
};

class StochasticOscillator : public IIndicator {
public:
    explicit StochasticOscillator(size_t k_period = 14, size_t d_period = 3);
    ~StochasticOscillator() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    quantization::expected<StochasticResult, IndicatorError>
    calculate_full(const std::vector<OHLCV>& data);

    std::string name() const override { return "StochasticOscillator"; }
    size_t min_data_points() const override { return k_period_ + d_period_; }

private:
    size_t k_period_;
    size_t d_period_;
};

} // namespace quantization::indicators

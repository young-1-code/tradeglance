#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

class ATR : public IIndicator {
public:
    explicit ATR(size_t period = 14);
    ~ATR() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    std::string name() const override { return "ATR"; }
    size_t min_data_points() const override { return period_ + 1; }

private:
    size_t period_;

    double calculate_true_range(const OHLCV& current, const OHLCV& previous);
};

} // namespace quantization::indicators

#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

struct ADXResult {
    std::vector<double> adx;
    std::vector<double> plus_di;
    std::vector<double> minus_di;
    std::vector<std::chrono::system_clock::time_point> timestamps;
};

class ADX : public IIndicator {
public:
    explicit ADX(size_t period = 14);
    ~ADX() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    quantization::expected<ADXResult, IndicatorError>
    calculate_full(const std::vector<OHLCV>& data);

    std::string name() const override { return "ADX"; }
    size_t min_data_points() const override { return period_ * 2; }

private:
    size_t period_;
};

} // namespace quantization::indicators

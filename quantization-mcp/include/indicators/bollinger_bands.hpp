#pragma once

#include "indicator_base.hpp"

namespace quantization::indicators {

struct BollingerBandsResult {
    std::vector<double> upper_band;
    std::vector<double> middle_band;
    std::vector<double> lower_band;
    std::vector<std::chrono::system_clock::time_point> timestamps;
};

class BollingerBands : public IIndicator {
public:
    explicit BollingerBands(size_t period = 20, double std_dev = 2.0);
    ~BollingerBands() override = default;

    quantization::expected<IndicatorResult, IndicatorError>
    calculate(const std::vector<OHLCV>& data) override;

    quantization::expected<BollingerBandsResult, IndicatorError>
    calculate_full(const std::vector<OHLCV>& data);

    std::string name() const override { return "BollingerBands"; }
    size_t min_data_points() const override { return period_; }

private:
    size_t period_;
    double std_dev_;
};

} // namespace quantization::indicators

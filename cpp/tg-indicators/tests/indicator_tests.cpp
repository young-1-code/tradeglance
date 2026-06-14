#include <cmath>
#include <string>
#include <vector>

#include <gtest/gtest.h>

#include "tg_indicators/indicator_service.h"
#include "tg_indicators/indicators/adx.h"
#include "tg_indicators/indicators/atr.h"
#include "tg_indicators/indicators/bollinger_bands.h"
#include "tg_indicators/indicators/cci.h"
#include "tg_indicators/indicators/ema.h"
#include "tg_indicators/indicators/macd.h"
#include "tg_indicators/indicators/obv.h"
#include "tg_indicators/indicators/rsi.h"
#include "tg_indicators/indicators/sma.h"
#include "tg_indicators/indicators/stochastic.h"
#include "tg_indicators/indicators/williams_r.h"

namespace {

using tg_indicators::OHLCV;
using tg_indicators::Params;

std::vector<OHLCV> increasing_bars(size_t count) {
  std::vector<OHLCV> bars;
  bars.reserve(count);
  for (size_t i = 0; i < count; ++i) {
    const double close = 10.0 + static_cast<double>(i);
    bars.push_back(OHLCV{
        1'700'000'000'000 + static_cast<int64_t>(i) * 86'400'000,
        close - 0.5,
        close + 1.0,
        close - 1.0,
        close,
        100 + static_cast<int64_t>(i),
        close * 1000.0,
    });
  }
  return bars;
}

tg::v1::Bar make_proto_bar(const OHLCV& bar) {
  tg::v1::Bar proto;
  proto.set_symbol("000001");
  proto.set_exchange(tg::v1::EXCHANGE_SZ);
  proto.set_period(tg::v1::BAR_PERIOD_DAILY);
  proto.set_ts_epoch_millis(bar.ts_millis);
  proto.set_trading_date("2026-01-01");
  proto.set_open(std::to_string(bar.open));
  proto.set_high(std::to_string(bar.high));
  proto.set_low(std::to_string(bar.low));
  proto.set_close(std::to_string(bar.close));
  proto.set_volume(bar.volume);
  proto.set_amount(std::to_string(bar.amount));
  return proto;
}

void expect_nan(double value) {
  EXPECT_TRUE(std::isnan(value));
}

}  // namespace

TEST(SmaIndicatorTest, ComputesAlignedSeries) {
  tg_indicators::SmaIndicator indicator;
  const auto result = indicator.compute(increasing_bars(5), {{"period", 3.0}});
  const auto& sma = result.at("sma");
  ASSERT_EQ(sma.size(), 5U);
  expect_nan(sma[0]);
  expect_nan(sma[1]);
  EXPECT_NEAR(sma[2], 11.0, 1e-12);
  EXPECT_NEAR(sma[4], 13.0, 1e-12);
}

TEST(EmaIndicatorTest, SeedsWithSmaThenSmooths) {
  tg_indicators::EmaIndicator indicator;
  const auto result = indicator.compute(increasing_bars(5), {{"period", 3.0}});
  const auto& ema = result.at("ema");
  expect_nan(ema[1]);
  EXPECT_NEAR(ema[2], 11.0, 1e-12);
  EXPECT_NEAR(ema[3], 12.0, 1e-12);
  EXPECT_NEAR(ema[4], 13.0, 1e-12);
}

TEST(MacdIndicatorTest, FlatPricesProduceZeroAfterWarmup) {
  auto bars = increasing_bars(40);
  for (auto& bar : bars) {
    bar.close = 10.0;
  }
  tg_indicators::MacdIndicator indicator;
  const auto result = indicator.compute(bars, {{"fast", 3.0}, {"slow", 6.0}, {"signal", 3.0}});
  EXPECT_NEAR(result.at("dif").back(), 0.0, 1e-12);
  EXPECT_NEAR(result.at("dea").back(), 0.0, 1e-12);
  EXPECT_NEAR(result.at("hist").back(), 0.0, 1e-12);
}

TEST(RsiIndicatorTest, RisingPricesReachOneHundred) {
  tg_indicators::RsiIndicator indicator;
  const auto result = indicator.compute(increasing_bars(8), {{"period", 3.0}});
  const auto& rsi = result.at("rsi");
  expect_nan(rsi[2]);
  EXPECT_NEAR(rsi[3], 100.0, 1e-12);
  EXPECT_NEAR(rsi.back(), 100.0, 1e-12);
}

TEST(BollingerBandsIndicatorTest, ComputesPopulationBands) {
  tg_indicators::BollingerBandsIndicator indicator;
  const auto result = indicator.compute(increasing_bars(5), {{"period", 3.0}, {"std_dev", 2.0}});
  EXPECT_NEAR(result.at("mid")[2], 11.0, 1e-12);
  EXPECT_NEAR(result.at("upper")[2], 11.0 + 2.0 * std::sqrt(2.0 / 3.0), 1e-12);
  EXPECT_NEAR(result.at("lower")[2], 11.0 - 2.0 * std::sqrt(2.0 / 3.0), 1e-12);
}

TEST(AtrIndicatorTest, ComputesWilderAtr) {
  tg_indicators::AtrIndicator indicator;
  const auto result = indicator.compute(increasing_bars(5), {{"period", 3.0}});
  const auto& atr = result.at("atr");
  EXPECT_NEAR(atr[2], 2.0, 1e-12);
  EXPECT_NEAR(atr[4], 2.0, 1e-12);
}

TEST(AdxIndicatorTest, StrongUptrendProducesHighDirectionalValues) {
  tg_indicators::AdxIndicator indicator;
  const auto result = indicator.compute(increasing_bars(8), {{"period", 3.0}});
  EXPECT_NEAR(result.at("plus_di").back(), 50.0, 1e-12);
  EXPECT_NEAR(result.at("minus_di").back(), 0.0, 1e-12);
  EXPECT_NEAR(result.at("adx").back(), 100.0, 1e-12);
}

TEST(CciIndicatorTest, ComputesKnownWindowValue) {
  tg_indicators::CciIndicator indicator;
  const auto result = indicator.compute(increasing_bars(5), {{"period", 3.0}});
  EXPECT_NEAR(result.at("cci")[2], 100.0, 1e-12);
  EXPECT_NEAR(result.at("cci")[4], 100.0, 1e-12);
}

TEST(StochasticIndicatorTest, ComputesKdjFromRsv) {
  tg_indicators::StochasticIndicator indicator;
  const auto result = indicator.compute(increasing_bars(3), {{"k_period", 3.0}, {"d_period", 3.0}});
  EXPECT_NEAR(result.at("k")[2], 58.3333333333, 1e-10);
  EXPECT_NEAR(result.at("d")[2], 52.7777777778, 1e-10);
  EXPECT_NEAR(result.at("j")[2], 69.4444444444, 1e-10);
}

TEST(WilliamsRIndicatorTest, ComputesKnownWindowValue) {
  tg_indicators::WilliamsRIndicator indicator;
  const auto result = indicator.compute(increasing_bars(3), {{"period", 3.0}});
  EXPECT_NEAR(result.at("willr")[2], -25.0, 1e-12);
}

TEST(ObvIndicatorTest, AccumulatesSignedVolume) {
  tg_indicators::ObvIndicator indicator;
  const auto result = indicator.compute(increasing_bars(4), {});
  const auto& obv = result.at("obv");
  EXPECT_NEAR(obv[0], 0.0, 1e-12);
  EXPECT_NEAR(obv[1], 101.0, 1e-12);
  EXPECT_NEAR(obv[3], 306.0, 1e-12);
}

TEST(IndicatorServiceTest, ComputesRequestInProcess) {
  tg_indicators::IndicatorServiceImpl service;
  tg::v1::IndicatorRequest request;
  request.set_indicator("SMA");
  (*request.mutable_params())["period"] = 3.0;
  for (const auto& bar : increasing_bars(5)) {
    *request.add_bars() = make_proto_bar(bar);
  }
  tg::v1::IndicatorResult response;
  const grpc::Status status = service.Compute(nullptr, &request, &response);
  ASSERT_TRUE(status.ok()) << status.error_message();
  EXPECT_EQ(response.indicator(), "SMA");
  ASSERT_EQ(response.ts_epoch_millis_size(), 5);
  ASSERT_TRUE(response.series().contains("sma"));
  EXPECT_NEAR(response.series().at("sma").values(4), 13.0, 1e-12);
}

TEST(IndicatorServiceTest, RejectsUnknownIndicator) {
  tg_indicators::IndicatorServiceImpl service;
  tg::v1::IndicatorRequest request;
  request.set_indicator("NOPE");
  tg::v1::IndicatorResult response;
  const grpc::Status status = service.Compute(nullptr, &request, &response);
  EXPECT_EQ(status.error_code(), grpc::StatusCode::NOT_FOUND);
}

int main(int argc, char** argv) {
  testing::InitGoogleTest(&argc, argv);
  return RUN_ALL_TESTS();
}

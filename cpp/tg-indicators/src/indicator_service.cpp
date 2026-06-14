#include "tg_indicators/indicator_service.h"

#include <csignal>
#include <exception>
#include <iostream>

#include "tg_indicators/bar_codec.h"
#include "tg_indicators/indicators/registry.h"

namespace tg_indicators {
namespace {

void fill_result(const tg::v1::IndicatorRequest& request,
                 const std::vector<OHLCV>& bars,
                 const SeriesMap& series,
                 tg::v1::IndicatorResult* response) {
  response->Clear();
  response->set_indicator(request.indicator());
  for (const auto& bar : bars) {
    response->add_ts_epoch_millis(bar.ts_millis);
  }
  auto* out_series = response->mutable_series();
  for (const auto& [name, values] : series) {
    auto& double_series = (*out_series)[name];
    for (double value : values) {
      double_series.add_values(value);
    }
  }
}

grpc::Status compute_request(const tg::v1::IndicatorRequest& request,
                             tg::v1::IndicatorResult* response) {
  auto indicator = create_indicator(request.indicator());
  if (!indicator) {
    return {grpc::StatusCode::NOT_FOUND, "unknown indicator: " + request.indicator()};
  }

  Params params;
  params.reserve(static_cast<size_t>(request.params().size()));
  for (const auto& [key, value] : request.params()) {
    params.emplace(key, value);
  }

  try {
    const std::vector<OHLCV> bars = decode_bars(request.bars());
    const SeriesMap series = indicator->compute(bars, params);
    fill_result(request, bars, series, response);
    return grpc::Status::OK;
  } catch (const std::invalid_argument& e) {
    return {grpc::StatusCode::INVALID_ARGUMENT, e.what()};
  } catch (const std::exception& e) {
    return {grpc::StatusCode::INTERNAL, e.what()};
  }
}

}  // namespace

grpc::Status IndicatorServiceImpl::Compute(grpc::ServerContext*,
                                           const tg::v1::IndicatorRequest* request,
                                           tg::v1::IndicatorResult* response) {
  return compute_request(*request, response);
}

grpc::Status IndicatorServiceImpl::BatchCompute(
    grpc::ServerContext*,
    grpc::ServerReaderWriter<tg::v1::IndicatorResult, tg::v1::IndicatorRequest>* stream) {
  tg::v1::IndicatorRequest request;
  while (stream->Read(&request)) {
    tg::v1::IndicatorResult response;
    const grpc::Status status = compute_request(request, &response);
    if (!status.ok()) {
      response.Clear();
      response.set_indicator(request.indicator());
    }
    stream->Write(response);
  }
  return grpc::Status::OK;
}

std::unique_ptr<grpc::Server> StartIndicatorServer(const std::string& address,
                                                   IndicatorServiceImpl* service) {
  grpc::ServerBuilder builder;
  builder.AddListeningPort(address, grpc::InsecureServerCredentials());
  builder.RegisterService(service);
  return builder.BuildAndStart();
}

}  // namespace tg_indicators


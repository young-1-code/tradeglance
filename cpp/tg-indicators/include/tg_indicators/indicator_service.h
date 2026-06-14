#pragma once

#include <memory>

#include <grpcpp/grpcpp.h>

#include "tg/v1/contracts.grpc.pb.h"

namespace tg_indicators {

class IndicatorServiceImpl final : public tg::v1::IndicatorService::Service {
 public:
  grpc::Status Compute(grpc::ServerContext* context,
                       const tg::v1::IndicatorRequest* request,
                       tg::v1::IndicatorResult* response) override;

  grpc::Status BatchCompute(
      grpc::ServerContext* context,
      grpc::ServerReaderWriter<tg::v1::IndicatorResult, tg::v1::IndicatorRequest>* stream) override;
};

std::unique_ptr<grpc::Server> StartIndicatorServer(const std::string& address,
                                                   IndicatorServiceImpl* service);

}  // namespace tg_indicators


#include <atomic>
#include <csignal>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <string>
#include <thread>

#include "tg_indicators/indicator_service.h"

namespace {

std::atomic_bool shutdown_requested{false};

void handle_signal(int) {
  shutdown_requested.store(true);
}

}  // namespace

int main() {
  const char* port_env = std::getenv("TG_INDICATORS_PORT");
  const std::string port = port_env == nullptr ? "50053" : port_env;
  const std::string address = "0.0.0.0:" + port;

  std::signal(SIGINT, handle_signal);
  std::signal(SIGTERM, handle_signal);

  tg_indicators::IndicatorServiceImpl service;
  std::unique_ptr<grpc::Server> server = tg_indicators::StartIndicatorServer(address, &service);
  if (!server) {
    std::cerr << "failed to start tg-indicators on " << address << '\n';
    return 1;
  }

  std::cout << "tg-indicators listening on " << address << '\n';
  while (!shutdown_requested.load()) {
    std::this_thread::sleep_for(std::chrono::milliseconds(100));
  }
  server->Shutdown();
  return 0;
}


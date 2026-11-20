#pragma once
#include "json.hpp"
#include <chrono>
#include <cstdint>
#include <string>
#include <sys/types.h>

namespace senti {
// One subprocess serializes access to the shared Rust user model. No shell,
// TCP listener, or foreign pointers cross the protocol boundary.
class Bridge {
public:
  explicit Bridge(std::string executable, bool ephemeral = false);
  ~Bridge();
  Bridge(const Bridge &) = delete;
  Bridge &operator=(const Bridge &) = delete;
  Json exchange(const Json &command);
  void stop(bool graceful = false);
  uint64_t generation() const { return generation_; }

private:
  void start();
  void wait(short events, std::chrono::steady_clock::time_point deadline);
  std::string executable_;
  bool ephemeral_;
  int socket_ = -1;
  pid_t child_ = -1;
  uint64_t sequence_ = 0;
  uint64_t generation_ = 0;
  std::string pending_;
  std::chrono::steady_clock::time_point retry_{};
};
} // namespace senti

#include "bridge.hpp"
#include <chrono>
#include <cstdlib>
#include <iostream>
#include <stdexcept>
void check(bool value, const char *message) {
  if (!value)
    throw std::runtime_error(message);
}
int main(int argc, char **argv) {
  if (argc != 3)
    return 2;
  try {
    senti::Bridge healthy(argv[2], true);
    check(senti::truth(
              senti::get(healthy.exchange(senti::command("ping")), "ok")),
          "healthy bridge failed");
    for (const auto *mode : {"malformed", "version", "exit", "hang"}) {
      setenv("SENTI_TEST_FAILURE", mode, 1);
      senti::Bridge failed(argv[1], true);
      auto start = std::chrono::steady_clock::now();
      bool rejected = false;
      try {
        failed.exchange(senti::command("ping"));
      } catch (const std::exception &) {
        rejected = true;
      }
      check(rejected, "bad backend was accepted");
      check(std::chrono::steady_clock::now() - start < std::chrono::seconds(2),
            "backend timeout was not bounded");
      auto generation = failed.generation();
      try {
        failed.exchange(senti::command("ping"));
      } catch (const std::exception &) {
      }
      check(failed.generation() == generation,
            "failure cooldown spawned another process");
    }
    std::cout << "IPC failure handling passed\n";
  } catch (const std::exception &error) {
    std::cerr << error.what() << '\n';
    return 1;
  }
}

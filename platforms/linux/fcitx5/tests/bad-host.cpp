#include <chrono>
#include <cstdlib>
#include <iostream>
#include <string>
#include <thread>
int main() {
  std::string line;
  std::getline(std::cin, line);
  const char *configured = std::getenv("SENTI_TEST_FAILURE");
  std::string mode = configured ? configured : "malformed";
  if (mode == "hang")
    std::this_thread::sleep_for(std::chrono::seconds(5));
  else if (mode == "version")
    std::cout << "{\"version\":99,\"id\":1,\"ok\":true}\n";
  else if (mode == "malformed")
    std::cout << "not-json\n";
  return 0;
}

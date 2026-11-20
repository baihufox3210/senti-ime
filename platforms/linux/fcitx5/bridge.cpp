#include "bridge.hpp"
#include <cerrno>
#include <csignal>
#include <fcntl.h>
#include <poll.h>
#include <spawn.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <thread>
#include <unistd.h>

extern char **environ;
namespace senti {
namespace {
constexpr size_t maxFrame = 64 * 1024;
}
Bridge::Bridge(std::string executable, bool ephemeral)
    : executable_(std::move(executable)), ephemeral_(ephemeral) {}
Bridge::~Bridge() { stop(true); }
void Bridge::start() {
  if (socket_ >= 0)
    return;
  if (std::chrono::steady_clock::now() < retry_)
    throw std::runtime_error("backend cooling down");
  if (executable_.empty() || executable_[0] != '/')
    throw std::runtime_error("backend path must be absolute");
  int pair[2];
  if (socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, pair) < 0)
    throw std::runtime_error("socketpair failed");
  for (int &fd : pair) {
    if (fd <= STDERR_FILENO) {
      int copy = fcntl(fd, F_DUPFD_CLOEXEC, 3);
      close(fd);
      fd = copy;
    }
  }
  if (pair[0] < 0 || pair[1] < 0) {
    if (pair[0] >= 0)
      close(pair[0]);
    if (pair[1] >= 0)
      close(pair[1]);
    throw std::runtime_error("socket duplication failed");
  }
  posix_spawn_file_actions_t actions;
  posix_spawn_file_actions_init(&actions);
  posix_spawn_file_actions_adddup2(&actions, pair[1], STDIN_FILENO);
  posix_spawn_file_actions_adddup2(&actions, pair[1], STDOUT_FILENO);
  posix_spawn_file_actions_addclose(&actions, pair[0]);
  posix_spawn_file_actions_addclose(&actions, pair[1]);
  char ephemeral[] = "--ephemeral";
  char *args[] = {executable_.data(), ephemeral_ ? ephemeral : nullptr,
                  nullptr};
  int result = posix_spawn(&child_, executable_.c_str(), &actions, nullptr,
                           args, environ);
  posix_spawn_file_actions_destroy(&actions);
  close(pair[1]);
  if (result != 0) {
    close(pair[0]);
    child_ = -1;
    throw std::runtime_error("cannot start backend");
  }
  socket_ = pair[0];
  int flags = fcntl(socket_, F_GETFL, 0);
  if (flags < 0 || fcntl(socket_, F_SETFL, flags | O_NONBLOCK) < 0) {
    stop();
    throw std::runtime_error("nonblocking socket failed");
  }
  pending_.clear();
  ++generation_;
}
void Bridge::stop(bool graceful) {
  if (socket_ >= 0) {
    close(socket_);
    socket_ = -1;
  }
  if (child_ > 0) {
    // Give EOF a short chance to shut down normally, then bound cleanup.
    bool reaped = false;
    for (int i = 0; i < (graceful ? 250 : 10); ++i) {
      pid_t result = waitpid(child_, nullptr, WNOHANG);
      if (result == child_ || (result < 0 && errno == ECHILD)) {
        reaped = true;
        break;
      }
      std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }
    if (!reaped) {
      kill(child_, SIGKILL);
      while (waitpid(child_, nullptr, 0) < 0 && errno == EINTR) {
      }
    }
    child_ = -1;
  }
  pending_.clear();
}
void Bridge::wait(short events,
                  std::chrono::steady_clock::time_point deadline) {
  for (;;) {
    auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(
                         deadline - std::chrono::steady_clock::now())
                         .count();
    if (remaining <= 0)
      throw std::runtime_error("backend timeout");
    pollfd descriptor{socket_, events, 0};
    int result = poll(&descriptor, 1, static_cast<int>(remaining));
    if (result < 0 && errno == EINTR)
      continue;
    if (result <= 0)
      throw std::runtime_error("backend unavailable");
    if (descriptor.revents & events)
      return;
    throw std::runtime_error("backend disconnected");
  }
}
Json Bridge::exchange(const Json &commandValue) {
  try {
    start();
    auto request = object();
    set(request, "version", number(1));
    set(request, "id", number(++sequence_));
    set(request, "command", commandValue);
    std::string payload =
        json_object_to_json_string_ext(request.get(), JSON_C_TO_STRING_PLAIN);
    payload += '\n';
    if (payload.size() > maxFrame)
      throw std::runtime_error("request too large");
    auto deadline =
        std::chrono::steady_clock::now() + std::chrono::milliseconds(500);
    size_t sent = 0;
    while (sent < payload.size()) {
      wait(POLLOUT, deadline);
      ssize_t n = send(socket_, payload.data() + sent, payload.size() - sent,
                       MSG_NOSIGNAL);
      if (n < 0 && (errno == EAGAIN || errno == EINTR))
        continue;
      if (n <= 0)
        throw std::runtime_error("backend write failed");
      sent += static_cast<size_t>(n);
    }
    while (pending_.find('\n') == std::string::npos) {
      wait(POLLIN, deadline);
      char buffer[4096];
      ssize_t n = recv(socket_, buffer, sizeof(buffer), 0);
      if (n < 0 && (errno == EAGAIN || errno == EINTR))
        continue;
      if (n <= 0)
        throw std::runtime_error("backend read failed");
      pending_.append(buffer, static_cast<size_t>(n));
      if (pending_.size() > maxFrame)
        throw std::runtime_error("response too large");
    }
    size_t end = pending_.find('\n');
    auto *parser = json_tokener_new();
    auto response = own(
        json_tokener_parse_ex(parser, pending_.data(), static_cast<int>(end)));
    bool parsed = json_tokener_get_error(parser) == json_tokener_success;
    size_t parsedEnd = json_tokener_get_parse_end(parser);
    json_tokener_free(parser);
    pending_.erase(0, end + 1);
    if (!parsed || parsedEnd != end || integer(get(response, "version")) != 1 ||
        integer(get(response, "id")) != static_cast<int64_t>(sequence_) ||
        !truth(get(response, "ok")))
      throw std::runtime_error("invalid backend response");
    return response;
  } catch (...) {
    stop();
    retry_ = std::chrono::steady_clock::now() + std::chrono::seconds(3);
    throw;
  }
}
} // namespace senti

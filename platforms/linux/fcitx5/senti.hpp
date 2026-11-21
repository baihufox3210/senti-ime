#pragma once
#include "bridge.hpp"
#include <fcitx-config/configuration.h>
#include <fcitx-config/option.h>
#include <fcitx/action.h>
#include <fcitx/inputcontextproperty.h>
#include <fcitx/inputmethodengine.h>
#include <fcitx/instance.h>
#include <fcitx/menu.h>

namespace senti {
FCITX_CONFIGURATION(
    Settings, fcitx::Option<bool> learning{this, "Learning",
                                           "Enable local learning", true};
    fcitx::Option<bool> memory{
        this, "Memory", "Remember vocabulary and submitted context locally",
        true};
    fcitx::Option<std::string> memoryProfile{
        this, "MemoryProfile",
        "Memory profile (empty: app vocabulary; custom: saved conversation "
        "context)",
        ""};
    fcitx::Option<int> memoryDays{this, "MemoryDays",
                                  "Memory retention in days (1-3650)", 30};
    fcitx::Option<bool> clearMemory{
        this, "ClearMemory", "Clear memory in this profile on next input",
        false};
    fcitx::Option<bool> emoji{this, "Emoji", "Offer emoji for manual selection", false};
    fcitx::Option<bool> datetime{this, "DateTime", "Enable date/time shortcuts",
                                 true};
    fcitx::Option<bool> calculator{this, "Calculator", "Enable calculator",
                                   true};);

struct ContextState : fcitx::InputContextProperty {
  std::string id;
  std::string entry;
  std::string mode = "Mixed";
  std::string chineseMode = "Mixed";
  Json view;
  uint64_t generation = 0;
  uint64_t revision = 0;
  uint64_t configRevision = 0;
  bool sensitive = false;
  bool surroundingValid = false;
  bool memoryScopeValid = false;
  std::string before, after;
};

class Engine final : public fcitx::InputMethodEngineV3 {
public:
  explicit Engine(fcitx::Instance *instance, std::string executable = {});
  ~Engine() override;
  void keyEvent(const fcitx::InputMethodEntry &, fcitx::KeyEvent &) override;
  void activate(const fcitx::InputMethodEntry &,
                fcitx::InputContextEvent &) override;
  void deactivate(const fcitx::InputMethodEntry &,
                  fcitx::InputContextEvent &) override;
  void reset(const fcitx::InputMethodEntry &,
             fcitx::InputContextEvent &) override;
  std::string subModeLabelImpl(const fcitx::InputMethodEntry &,
                               fcitx::InputContext &) override;
  const fcitx::Configuration *getConfig() const override { return &settings_; }
  void setConfig(const fcitx::RawConfig &) override;
  void reloadConfig() override;

private:
  bool sensitive(fcitx::InputContext *) const;
  void ensureActive(fcitx::InputContext *);
  void apply(fcitx::InputContext *, const Json &);
  void clear(fcitx::InputContext *);
  void updateMemoryMenu(fcitx::InputContext *);
  void failure(fcitx::InputContext *, bool recover);
  void surrounding(fcitx::InputContext *);
  void select(fcitx::InputContext *, size_t label, uint64_t revision);
  void page(fcitx::InputContext *, bool forward, uint64_t revision);
  Json sendKey(fcitx::InputContext *, const char *name);
  fcitx::Instance *instance_;
  uint64_t contexts_ = 0;
  Bridge bridge_;
  std::string active_;
  Settings settings_;
  std::unique_ptr<fcitx::Action> memoryAction_;
  std::unique_ptr<fcitx::Menu> memoryMenu_;
  std::vector<std::unique_ptr<fcitx::Action>> memoryItems_;
  std::string memoryMenuStamp_;
  size_t memoryMenuOffset_ = 0;
  uint64_t configRevision_ = 1;
  bool unavailable_ = false;
  std::shared_ptr<int> lifetime_ = std::make_shared<int>(0);
  fcitx::FactoryFor<ContextState> factory_;
  std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>>
      surroundingWatcher_;
  std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>>
      capabilityWatcher_;
};
} // namespace senti

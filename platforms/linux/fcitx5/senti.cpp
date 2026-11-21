#include "senti.hpp"
#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <fcitx-config/iniparser.h>
#include <fcitx-utils/cutf8.h>
#include <fcitx-utils/log.h>
#include <fcitx/addonfactory.h>
#include <fcitx/addonmanager.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputcontextmanager.h>
#include <fcitx/inputpanel.h>
#include <fcitx/statusarea.h>
#include <fcitx/userinterfacemanager.h>
#include <functional>
#include <utility>

#ifndef SENTI_HOST_PATH
#define SENTI_HOST_PATH "/usr/libexec/senti-ime/senti-host"
#endif
namespace senti {
namespace {
class MemoryItem final : public fcitx::Action {
public:
  MemoryItem(std::string label,
             std::function<void(fcitx::InputContext *)> callback = {},
             std::string detail = {})
      : label_(std::move(label)), callback_(std::move(callback)),
        detail_(std::move(detail)) {}
  std::string shortText(fcitx::InputContext *) const override { return label_; }
  std::string icon(fcitx::InputContext *) const override {
    return "document-properties";
  }
  std::string longText(fcitx::InputContext *) const override { return detail_; }
  void activate(fcitx::InputContext *ic) override {
    auto callback = callback_;
    if (callback)
      callback(ic);
  }

private:
  std::string label_;
  std::function<void(fcitx::InputContext *)> callback_;
  std::string detail_;
};
std::string backendPath(std::string value) {
  if (!value.empty())
    return value;
  const char *overridePath = std::getenv("SENTI_IME_HOST_BINARY");
  return overridePath && *overridePath ? overridePath : SENTI_HOST_PATH;
}
bool ephemeral() {
  const char *value = std::getenv("SENTI_IME_EPHEMERAL");
  return value && std::string(value) == "1";
}
std::string application(fcitx::InputContext *ic) {
  auto program = ic->program();
  std::transform(
      program.begin(), program.end(), program.begin(),
      [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
  if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Terminal) ||
      program.find("code") != std::string::npos ||
      program.find("vim") != std::string::npos)
    return "Code";
  if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Email) ||
      program.find("thunderbird") != std::string::npos)
    return "Email";
  if (program.find("telegram") != std::string::npos ||
      program.find("discord") != std::string::npos ||
      program.find("slack") != std::string::npos)
    return "Chat";
  return "Document";
}
class Word final : public fcitx::CandidateWord {
public:
  Word(const std::string &value,
       std::function<void(fcitx::InputContext *)> select)
      : CandidateWord(fcitx::Text(value)), select_(std::move(select)) {}
  void select(fcitx::InputContext *ic) const override { select_(ic); }

private:
  std::function<void(fcitx::InputContext *)> select_;
};
class Candidates final : public fcitx::CandidateList,
                         public fcitx::PageableCandidateList {
public:
  Candidates(const Json &view,
             std::function<void(fcitx::InputContext *, size_t)> select,
             std::function<void(bool)> turnPage)
      : turnPage_(std::move(turnPage)) {
    setPageable(this);
    page_ = static_cast<int>(integer(get(view, "page")));
    pages_ = static_cast<int>(integer(get(view, "page_count")));
    cursor_ = truth(get(view, "selecting"))
                  ? static_cast<int>(integer(get(view, "candidate_cursor")))
                  : 0;
    auto values = get(view, "candidates");
    if (!values || !json_object_is_type(values.get(), json_type_array))
      return;
    for (size_t i = 0; i < json_object_array_length(values.get()); ++i) {
      auto value =
          own(json_object_get(json_object_array_get_idx(values.get(), i)));
      size_t label = static_cast<size_t>(integer(get(value, "label")));
      labels_.emplace_back(std::to_string(label) + ". ");
      words_.push_back(std::make_unique<Word>(
          text(get(value, "text")),
          [select, label](fcitx::InputContext *ic) { select(ic, label); }));
    }
  }
  const fcitx::Text &label(int i) const override { return labels_.at(i); }
  const fcitx::CandidateWord &candidate(int i) const override {
    return *words_.at(i);
  }
  int size() const override { return static_cast<int>(words_.size()); }
  int cursorIndex() const override { return cursor_; }
  fcitx::CandidateLayoutHint layoutHint() const override {
    return fcitx::CandidateLayoutHint::Vertical;
  }
  bool hasPrev() const override { return page_ > 0; }
  bool hasNext() const override { return page_ + 1 < pages_; }
  void prev() override {
    if (hasPrev())
      turnPage_(false);
  }
  void next() override {
    if (hasNext())
      turnPage_(true);
  }
  bool usedNextBefore() const override { return page_ > 0; }
  int currentPage() const override { return page_; }
  int totalPages() const override { return pages_; }

private:
  std::vector<fcitx::Text> labels_;
  std::vector<std::unique_ptr<Word>> words_;
  std::function<void(bool)> turnPage_;
  int page_ = 0, pages_ = 0, cursor_ = -1;
};
} // namespace

Engine::Engine(fcitx::Instance *instance, std::string executable)
    : instance_(instance),
      bridge_(backendPath(std::move(executable)), ephemeral()),
      factory_([this](fcitx::InputContext &) {
        auto *state = new ContextState;
        state->id = std::to_string(++contexts_);
        return state;
      }) {
  instance_->inputContextManager().registerProperty("sentiState", &factory_);
  reloadConfig();
  memoryMenu_ = std::make_unique<fcitx::Menu>();
  memoryAction_ = std::make_unique<MemoryItem>("個人記憶（點選條目可刪除）");
  memoryAction_->registerAction("senti-memory",
                                &instance_->userInterfaceManager());
  memoryAction_->setMenu(memoryMenu_.get());
  surroundingWatcher_ = instance_->watchEvent(
      fcitx::EventType::InputContextSurroundingTextUpdated,
      fcitx::EventWatcherPhase::Default, [this](fcitx::Event &event) {
        auto *ic =
            static_cast<fcitx::InputContextEvent &>(event).inputContext();
        if (ic->propertyFor(&factory_)->id == active_) {
          try {
            surrounding(ic);
          } catch (...) {
            failure(ic, false);
          }
        }
      });
  capabilityWatcher_ = instance_->watchEvent(
      fcitx::EventType::InputContextCapabilityChanged,
      fcitx::EventWatcherPhase::Default, [this](fcitx::Event &event) {
        auto *ic =
            static_cast<fcitx::InputContextEvent &>(event).inputContext();
        if (ic->propertyFor(&factory_)->id == active_) {
          try {
            ensureActive(ic);
          } catch (...) {
            failure(ic, false);
          }
        }
      });
}
Engine::~Engine() { lifetime_.reset(); }
bool Engine::sensitive(fcitx::InputContext *ic) const {
  auto flags = ic->capabilityFlags();
  return flags.test(fcitx::CapabilityFlag::Password) ||
         flags.test(fcitx::CapabilityFlag::Sensitive) ||
         flags.test(fcitx::CapabilityFlag::Number) ||
         flags.test(fcitx::CapabilityFlag::Digit) ||
         flags.test(fcitx::CapabilityFlag::Disable);
}
void Engine::reloadConfig() {
  fcitx::readAsIni(settings_, "conf/senti.conf");
  ++configRevision_;
}
void Engine::setConfig(const fcitx::RawConfig &config) {
  settings_.load(config, true);
  fcitx::safeSaveAsIni(settings_, "conf/senti.conf");
  ++configRevision_;
}
void Engine::ensureActive(fcitx::InputContext *ic) {
  auto *state = ic->propertyFor(&factory_);
  bool secret = sensitive(ic);
  if (active_ == state->id && state->generation == bridge_.generation() &&
      state->sensitive == secret && state->configRevision == configRevision_)
    return;
  if (!active_.empty() && active_ != state->id) {
    const auto previous = active_;
    instance_->inputContextManager().foreach ([&](fcitx::InputContext *other) {
      auto *old = other->propertyFor(&factory_);
      if (old->id == previous) {
        old->view.reset();
        ++old->revision;
        clear(other);
      }
      return true;
    });
  }
  auto activate = command("activate", state->id);
  set(activate, "mode", string(state->mode));
  set(activate, "chinese_mode", string(state->chineseMode));
  set(activate, "app", string(application(ic)));
  set(activate, "sensitive", boolean(secret));
  auto response = bridge_.exchange(activate);
  active_ = state->id;
  state->surroundingValid = false;
  unavailable_ = false;
  state->generation = bridge_.generation();
  state->sensitive = secret;
  state->configRevision = configRevision_;
  // Global options belong to the one backend/model owner, not each client
  // field.
  auto learning = command("set_learning");
  set(learning, "enabled", boolean(*settings_.learning));
  bridge_.exchange(learning);
  if (!secret) {
    auto profile = *settings_.memoryProfile;
    if (profile.empty()) {
      auto program = ic->program();
      profile =
          "app:" +
          (program.empty() ? std::string("unknown") : program.substr(0, [&] {
            size_t end = std::min<size_t>(120, program.size());
            while (end > 0 && end < program.size() &&
                   (static_cast<unsigned char>(program[end]) & 0xc0) == 0x80)
              --end;
            return end;
          }()));
    }
    auto scope = command("memory_scope");
    set(scope, "scope", string(profile));
    const auto scoped = bridge_.exchange(scope);
    state->memoryScopeValid = truth(get(scoped, "ok"));
    auto memory = command("memory_enabled");
    set(memory, "enabled",
        boolean(*settings_.memory && truth(get(scoped, "ok"))));
    bridge_.exchange(memory);
    auto retention = command("memory_retention");
    set(retention, "days", number(std::clamp(*settings_.memoryDays, 1, 3650)));
    bridge_.exchange(retention);
    if (*settings_.clearMemory && truth(get(scoped, "ok"))) {
      bridge_.exchange(command("memory_clear"));
      bridge_.exchange(command("memory_flush"));
      settings_.clearMemory.setValue(false);
      fcitx::safeSaveAsIni(settings_, "conf/senti.conf");
    }
  }
  for (auto [name, enabled] : {std::pair{"emoji", *settings_.emoji},
                               {"datetime", *settings_.datetime},
                               {"calculator", *settings_.calculator}}) {
    auto plugin = command("set_plugin");
    set(plugin, "plugin", string(name));
    set(plugin, "enabled", boolean(enabled));
    bridge_.exchange(plugin);
  }
  if (!secret)
    response = bridge_.exchange(command("ping"));
  apply(ic, response);
  if (!secret)
    surrounding(ic);
}
void Engine::activate(const fcitx::InputMethodEntry &entry,
                      fcitx::InputContextEvent &event) {
  auto *ic = event.inputContext();
  auto *state = ic->propertyFor(&factory_);
  if (state->entry != entry.uniqueName()) {
    state->entry = entry.uniqueName();
    state->mode = entry.uniqueName() == "senti-zhuyin"    ? "Zhuyin"
                  : entry.uniqueName() == "senti-pinyin"  ? "Chinese"
                  : entry.uniqueName() == "senti-english" ? "English"
                                                          : "Mixed";
    state->chineseMode = state->mode == "English" ? "Mixed" : state->mode;
    state->configRevision = 0;
  }
  try {
    ensureActive(ic);
    if (!sensitive(ic) && ic->propertyFor(&factory_)->memoryScopeValid) {
      ic->statusArea().addAction(fcitx::StatusGroup::InputMethod,
                                 memoryAction_.get());
      memoryMenuStamp_.clear();
      memoryMenuOffset_ = 0;
      updateMemoryMenu(ic);
    }
  } catch (...) {
    failure(ic, false);
  }
}
void Engine::deactivate(const fcitx::InputMethodEntry &,
                        fcitx::InputContextEvent &event) {
  auto *ic = event.inputContext();
  auto *state = ic->propertyFor(&factory_);
  if (active_ == state->id) {
    try {
      bridge_.exchange(command("deactivate", state->id));
    } catch (...) {
    }
    active_.clear();
  }
  state->view.reset();
  ++state->revision;
  clear(ic);
}
void Engine::reset(const fcitx::InputMethodEntry &,
                   fcitx::InputContextEvent &event) {
  auto *ic = event.inputContext();
  auto *state = ic->propertyFor(&factory_);
  if (active_ == state->id) {
    try {
      apply(ic, bridge_.exchange(command("reset", state->id)));
    } catch (...) {
      failure(ic, false);
    }
  } else {
    state->view.reset();
    ++state->revision;
    clear(ic);
  }
}
void Engine::keyEvent(const fcitx::InputMethodEntry &, fcitx::KeyEvent &event) {
  auto *ic = event.inputContext();
  try {
    ensureActive(ic);
    if (sensitive(ic))
      return; // Never send password/sensitive keystrokes to the subprocess.
    auto sym = event.key().sym();
    if (event.isRelease() && sym != FcitxKey_Shift_L && sym != FcitxKey_Shift_R)
      return;
    const char *name = "other";
    switch (sym) {
    case FcitxKey_space:
      name = "space";
      break;
    case FcitxKey_Return:
    case FcitxKey_KP_Enter:
      name = "enter";
      break;
    case FcitxKey_Escape:
      name = "escape";
      break;
    case FcitxKey_BackSpace:
      name = "backspace";
      break;
    case FcitxKey_Tab:
      name = "tab";
      break;
    case FcitxKey_Up:
    case FcitxKey_KP_Up:
      name = "up";
      break;
    case FcitxKey_Down:
    case FcitxKey_KP_Down:
      name = "down";
      break;
    case FcitxKey_Left:
    case FcitxKey_KP_Left:
      name = "left";
      break;
    case FcitxKey_Right:
    case FcitxKey_KP_Right:
      name = "right";
      break;
    case FcitxKey_Home:
      name = "home";
      break;
    case FcitxKey_End:
      name = "end";
      break;
    case FcitxKey_Delete:
      name = "delete";
      break;
    case FcitxKey_Page_Up:
    case FcitxKey_KP_Prior:
      name = "page_up";
      break;
    case FcitxKey_Page_Down:
    case FcitxKey_KP_Next:
      name = "page_down";
      break;
    case FcitxKey_Shift_L:
    case FcitxKey_Shift_R:
      name = "shift";
      break;
    default:
      break;
    }
    auto key = object();
    set(key, "name", string(name));
    if (std::string(name) == "other") {
      uint32_t unicode = fcitx::Key::keySymToUnicode(sym);
      if (unicode >= 0x20 && unicode != 0x7f) {
        char bytes[8]{};
        int length = fcitx_ucs4_to_utf8(unicode, bytes);
        set(key, "name", string("character"));
        set(key, "value",
            string(std::string(bytes, static_cast<size_t>(length))));
      }
    }
    auto modifiers = object();
    auto states = event.key().states();
    set(modifiers, "control", boolean(states.test(fcitx::KeyState::Ctrl)));
    set(modifiers, "alt", boolean(states.test(fcitx::KeyState::Alt)));
    set(modifiers, "shift",
        boolean(event.rawKey().states().test(fcitx::KeyState::Shift)));
    set(modifiers, "meta",
        boolean(states.test(fcitx::KeyState::Super) ||
                states.test(fcitx::KeyState::Super2) ||
                states.test(fcitx::KeyState::Meta)));
    auto request = command("key", ic->propertyFor(&factory_)->id);
    set(request, "key", key);
    set(request, "modifiers", modifiers);
    set(request, "release", boolean(event.isRelease()));
    auto response = bridge_.exchange(request);
    if (truth(get(response, "consumed")))
      event.filterAndAccept();
    apply(ic, response);
  } catch (...) {
    failure(ic, true);
  }
}
void Engine::updateMemoryMenu(fcitx::InputContext *ic) {
  if (sensitive(ic) || !ic->hasFocus())
    return;
  auto *state = ic->propertyFor(&factory_);
  if (!state->memoryScopeValid)
    return;
  const auto info = get(state->view, "memory");
  const auto scope = text(get(info, "scope"));
  const auto stamp = scope + ":" + std::to_string(integer(get(info, "count"))) +
                     (truth(get(info, "enabled")) ? ":on" : ":off");
  const auto pageStamp = stamp + ":" + std::to_string(memoryMenuOffset_);
  if (memoryMenuStamp_ == pageStamp)
    return;
  memoryMenuStamp_ = pageStamp;
  for (const auto &item : memoryItems_)
    memoryMenu_->removeAction(item.get());
  memoryItems_.clear();
  auto listing = command("memory_page");
  set(listing, "offset", number(static_cast<int64_t>(memoryMenuOffset_)));
  auto entries = get(bridge_.exchange(listing), "memory");
  if (!entries || !json_object_is_type(entries.get(), json_type_array))
    return;
  auto weak = std::weak_ptr<int>(lifetime_);
  const auto context = state->id;
  const auto append = [&](std::string label, const char *operation, int64_t id,
                          std::string detail = {}) {
    auto item = std::make_unique<MemoryItem>(
        std::move(label),
        [this, weak, scope, context, operation,
         id](fcitx::InputContext *target) {
          if (!weak.lock() || !target || sensitive(target) ||
              !target->hasFocus())
            return;
          auto *current = target->propertyFor(&factory_);
          if (!current->memoryScopeValid || active_ != context ||
              current->id != context ||
              text(get(get(current->view, "memory"), "scope")) != scope)
            return;
          try {
            if (std::string(operation) == "memory_next") {
              memoryMenuOffset_ =
                  id > 0 ? std::min<size_t>(512, memoryMenuOffset_ + 8) : 0;
              memoryMenuStamp_.clear();
              updateMemoryMenu(target);
              return;
            }
            auto request = command(operation);
            if (id > 0)
              set(request, "id", number(id));
            auto response = bridge_.exchange(request);
            if (std::string(operation) == "memory_clear")
              memoryMenuOffset_ = 0;
            bridge_.exchange(command("memory_flush"));
            memoryMenuStamp_.clear();
            apply(target, response);
          } catch (...) {
            failure(target, false);
          }
        },
        std::move(detail));
    memoryMenu_->addAction(item.get());
    memoryItems_.push_back(std::move(item));
  };
  append("清除目前範圍的記憶", "memory_clear", 0);
  if (memoryMenuOffset_ > 0)
    append("回到最新記憶", "memory_next", 0);
  if (json_object_array_length(entries.get()) == 8 &&
      memoryMenuOffset_ + 8 < static_cast<size_t>(integer(get(info, "count"))))
    append("下一頁記憶", "memory_next", 1);
  for (size_t i = 0;
       i < std::min<size_t>(30, json_object_array_length(entries.get())); ++i) {
    auto entry =
        own(json_object_get(json_object_array_get_idx(entries.get(), i)));
    auto detail = text(get(entry, "text"));
    size_t end = std::min<size_t>(72, detail.size());
    while (end > 0 && end < detail.size() &&
           (static_cast<unsigned char>(detail[end]) & 0xc0) == 0x80)
      --end;
    auto label =
        "刪除：" + detail.substr(0, end) + (end < detail.size() ? "…" : "");
    append(label, "memory_delete", integer(get(entry, "id")), detail);
  }
  ic->updateUserInterface(fcitx::UserInterfaceComponent::StatusArea);
}
void Engine::clear(fcitx::InputContext *ic) {
  ic->inputPanel().reset();
  ic->updatePreedit();
  ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}
void Engine::failure(fcitx::InputContext *ic, bool recover) {
  auto *state = ic->propertyFor(&factory_);
  auto visible = text(get(state->view, "converted_preedit"),
                      text(get(state->view, "preedit")));
  state->view.reset();
  ++state->revision;
  active_.clear();
  clear(ic);
  if (recover && !sensitive(ic) && ic->hasFocus() && !visible.empty())
    ic->commitString(visible);
  // Never log keystrokes, surrounding text, or protocol payloads.
  if (!sensitive(ic) && ic->hasFocus()) {
    ic->inputPanel().setAuxUp(fcitx::Text("SentiIME 暫時離線，按鍵將直接輸入"));
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
  }
  if (!unavailable_)
    FCITX_WARN() << "SentiIME backend unavailable; native key passes through";
  unavailable_ = true;
}
void Engine::apply(fcitx::InputContext *ic, const Json &response,
                   bool scheduleModelPoll) {
  auto *state = ic->propertyFor(&factory_);
  state->view = get(response, "view");
  ++state->revision;
  if (scheduleModelPoll)
    state->modelPoll.reset();
  const auto &view = state->view;
  state->chineseMode = text(get(view, "chinese_mode"), state->chineseMode);
  auto mode = text(get(view, "mode"));
  if (!mode.empty())
    state->mode = mode;
  auto commit = text(get(response, "commit"));
  if (!commit.empty() && !sensitive(ic))
    ic->commitString(commit);
  // Build the complete next panel before notifying the frontend. Publishing
  // an empty preedit first caused flicker and redundant surrounding events.
  ic->inputPanel().reset();
  if (sensitive(ic) || !view ||
      json_object_is_type(view.get(), json_type_null)) {
    ic->updatePreedit();
    ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
    return;
  }
  auto preedit =
      text(get(view, "converted_preedit"), text(get(view, "preedit")));
  fcitx::Text textValue(
      preedit, fcitx::TextFormatFlags(fcitx::TextFormatFlag::Underline) |
                   fcitx::TextFormatFlag::DontCommit);
  auto suggestion = text(get(view, "suggestion"));
  if (!suggestion.empty() && !truth(get(view, "selecting")))
    textValue.append(suggestion,
                     fcitx::TextFormatFlags(fcitx::TextFormatFlag::DontCommit) |
                         fcitx::TextFormatFlag::Italic);
  textValue.setCursor(static_cast<int>(integer(
      get(view, "preedit_cursor"), static_cast<int64_t>(preedit.size()))));
  if (ic->capabilityFlags().test(fcitx::CapabilityFlag::Preedit))
    ic->inputPanel().setClientPreedit(textValue);
  else
    ic->inputPanel().setPreedit(textValue);
  auto warning = text(get(response, "warning"));
  const auto pages = integer(get(view, "page_count"));
  const bool open = truth(get(view, "selecting"));
  if (open && pages > 0) {
    auto title = "候選 " + std::to_string(integer(get(view, "page")) + 1) +
                 "/" + std::to_string(pages);

    ic->inputPanel().setAuxUp(fcitx::Text(title));
  } else if (!warning.empty()) {
    ic->inputPanel().setAuxUp(fcitx::Text(warning));
  }
  if (!warning.empty() && open && pages > 0)
    ic->inputPanel().setAuxDown(fcitx::Text(warning));
  auto weak = std::weak_ptr<int>(lifetime_);
  auto revision = state->revision;
  if (scheduleModelPoll && truth(get(view, "model_pending"))) {
    const auto hostRevision = integer(get(view, "decode_revision"));
    const auto watched = ic->watch();
    state->modelPoll = instance_->eventLoop().addTimeEvent(
        CLOCK_MONOTONIC, fcitx::now(CLOCK_MONOTONIC) + 20000, 2000,
        [this, weak, watched, revision, hostRevision](fcitx::EventSourceTime *event,
                                                     uint64_t) {
          auto *context = watched.get();
          if (!weak.lock() || !context)
            return false;
          auto *current = context->propertyFor(&factory_);
          if (!context->hasFocus() || sensitive(context) ||
              current->revision != revision || current->id != active_)
            return false;
          try {
            auto request = command("poll_word_model", current->id);
            set(request, "revision", number(hostRevision));
            auto reply = bridge_.exchange(request);
            auto latest = get(reply, "view");
            if (truth(get(latest, "model_pending"))) {
              event->setNextInterval(20000);
              return true;
            }
            if (integer(get(latest, "decode_revision")) != hostRevision)
              apply(context, reply, false);
          } catch (...) {
            // An optional model update must never commit or clear the draft.
          }
          return false;
        });
  }
  auto candidates = get(view, "candidates");
  if (open && candidates &&
      json_object_is_type(candidates.get(), json_type_array) &&
      json_object_array_length(candidates.get()) > 0) {
    ic->inputPanel().setCandidateList(std::make_unique<Candidates>(
        view,
        [this, weak, revision](fcitx::InputContext *target, size_t label) {
          if (weak.lock())
            select(target, label, revision);
        },
        [this, weak, ic, revision](bool forward) {
          if (weak.lock())
            page(ic, forward, revision);
        }));
  }
  if (!commit.empty())
    memoryMenuStamp_.clear();
  updateMemoryMenu(ic);
  ic->updatePreedit();
  ic->updateUserInterface(fcitx::UserInterfaceComponent::InputPanel);
}
void Engine::surrounding(fcitx::InputContext *ic) {
  if (sensitive(ic) || !ic->surroundingText().isValid())
    return;
  const auto &value = ic->surroundingText();
  // Fcitx cursor/anchor are Unicode scalar offsets, not UTF-8 byte offsets.
  const auto &full = value.text();
  size_t length = fcitx_utf8_strlen(full.c_str());
  size_t stop =
      std::min<size_t>(std::min(value.cursor(), value.anchor()), length);
  const char *begin = fcitx_utf8_get_nth_char(
      full.c_str(), static_cast<uint32_t>(stop > 400 ? stop - 400 : 0));
  const char *end =
      fcitx_utf8_get_nth_char(full.c_str(), static_cast<uint32_t>(stop));
  size_t startAfter =
      std::min<size_t>(std::max(value.cursor(), value.anchor()), length);
  const char *afterBegin =
      fcitx_utf8_get_nth_char(full.c_str(), static_cast<uint32_t>(startAfter));
  const char *afterEnd = fcitx_utf8_get_nth_char(
      full.c_str(), static_cast<uint32_t>(std::min(length, startAfter + 80)));
  std::string before(begin, end), after(afterBegin, afterEnd);
  auto *state = ic->propertyFor(&factory_);
  if (state->surroundingValid && state->before == before &&
      state->after == after)
    return;
  auto request = command("surrounding", state->id);
  set(request, "before_cursor", string(before));
  set(request, "after_cursor", string(after));
  auto response = bridge_.exchange(request);
  state->before = std::move(before);
  state->after = std::move(after);
  state->surroundingValid = true;
  apply(ic, response);
}
void Engine::select(fcitx::InputContext *ic, size_t label, uint64_t revision) {
  auto *state = ic->propertyFor(&factory_);
  if (state->revision != revision || state->configRevision != configRevision_ ||
      state->id != active_ || sensitive(ic))
    return;
  try {
    auto request = command("select", state->id);
    set(request, "label", number(static_cast<int64_t>(label)));
    apply(ic, bridge_.exchange(request));
  } catch (...) {
    failure(ic, false);
  }
}
Json Engine::sendKey(fcitx::InputContext *ic, const char *name) {
  auto request = command("key", ic->propertyFor(&factory_)->id);
  auto key = object();
  set(key, "name", string(name));
  set(request, "key", key);
  return bridge_.exchange(request);
}
void Engine::page(fcitx::InputContext *ic, bool forward, uint64_t revision) {
  auto *state = ic->propertyFor(&factory_);
  if (state->revision != revision || state->configRevision != configRevision_ ||
      state->id != active_ || sensitive(ic))
    return;
  try {
    apply(ic, sendKey(ic, forward ? "page_down" : "page_up"));
  } catch (...) {
    failure(ic, false);
  }
}
std::string Engine::subModeLabelImpl(const fcitx::InputMethodEntry &,
                                     fcitx::InputContext &ic) {
  auto mode = ic.propertyFor(&factory_)->mode;
  return mode == "English"   ? "EN"
         : mode == "Zhuyin"  ? "注"
         : mode == "Chinese" ? "拼"
                             : "中英";
}
class Factory final : public fcitx::AddonFactory {
  fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
    return new Engine(manager->instance());
  }
};
} // namespace senti
FCITX_ADDON_FACTORY(senti::Factory);

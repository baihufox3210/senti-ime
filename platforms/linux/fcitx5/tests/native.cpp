#include "senti.hpp"
#include <algorithm>
#include <dlfcn.h>
#include <fcitx/addonfactory.h>
#include <fcitx/candidatelist.h>
#include <fcitx/inputcontext.h>
#include <fcitx/inputcontextmanager.h>
#include <fcitx/inputpanel.h>
#include <fcitx/statusarea.h>
#include <fcitx/userinterfacemanager.h>
#include <iostream>
#include <stdexcept>
#include <thread>

namespace {
void check(bool value, const char *message) {
  if (!value)
    throw std::runtime_error(message);
}
class Client final : public fcitx::InputContext {
public:
  explicit Client(fcitx::InputContextManager &manager)
      : InputContext(manager, "senti-test-editor") {
    created();
    setCapabilityFlags(fcitx::CapabilityFlag::Preedit);
    focusIn();
  }
  ~Client() override {
    focusOut();
    destroy();
  }
  const char *frontend() const override { return "senti-test"; }
  std::string committed;
  unsigned int preeditUpdates = 0;

protected:
  void commitStringImpl(const std::string &text) override { committed += text; }
  void deleteSurroundingTextImpl(int, unsigned int) override {}
  void forwardKeyImpl(const fcitx::ForwardKeyEvent &) override {}
  void updatePreeditImpl() override { ++preeditUpdates; }
};
bool key(fcitx::InputMethodEngine *engine, const fcitx::InputMethodEntry &entry,
         Client &client, fcitx::KeySym sym,
         fcitx::KeyStates states = fcitx::KeyStates(), bool release = false) {
  fcitx::KeyEvent event(&client, fcitx::Key(sym, states), release);
  engine->keyEvent(entry, event);
  return event.accepted();
}
void type(fcitx::InputMethodEngine *engine,
          const fcitx::InputMethodEntry &entry, Client &client,
          const std::string &text) {
  for (unsigned char character : text)
    check(key(engine, entry, client, fcitx::Key::keySymFromUnicode(character)),
          "printable key unexpectedly passed through");
}
} // namespace
int main(int argc, char **argv) {
  if (argc != 3 && argc != 4)
    return 2;
  try {
    // Load the actual exported addon factory, not a replacement engine.
    void *module = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    check(module != nullptr, "dlopen addon failed");
    using Factory = fcitx::AddonFactory *(*)();
    auto factory = reinterpret_cast<Factory>(
        dlsym(module, "fcitx_addon_factory_instance"));
    check(factory != nullptr, "addon factory missing");
    setenv("SENTI_IME_HOST_BINARY", argv[2], 1);
    char program[] = "senti-native-test", disabled[] = "--disable=all";
    char *arguments[] = {program, disabled, nullptr};
    fcitx::Instance instance(2, arguments);
    instance.initialize();
    auto addon = std::unique_ptr<fcitx::AddonInstance>(
        factory()->create(&instance.addonManager()));
    auto *engine = dynamic_cast<fcitx::InputMethodEngine *>(addon.get());
    check(engine != nullptr, "addon is not an input method");
    if (argc == 4 && std::string_view(argv[3]) == "qwen") {
      {
        Client client(instance.inputContextManager());
        client.setCapabilityFlags(
            fcitx::CapabilityFlags(fcitx::CapabilityFlag::Preedit) |
            fcitx::CapabilityFlag::SurroundingText);
        const fcitx::InputMethodEntry entry("senti", "SentiIME", "zh_TW", "senti");
        fcitx::InputContextEvent focus(&client, fcitx::EventType::InputContextFocusIn);
        engine->activate(entry, focus);
        std::this_thread::sleep_for(std::chrono::seconds(2));
        client.surroundingText().setText("他是一位優秀的，代理過許多藝人", 7, 7);
        client.updateSurroundingText();
        type(engine, entry, client, "ru/ ru4");
        auto stop = instance.eventLoop().addTimeEvent(
            CLOCK_MONOTONIC, fcitx::now(CLOCK_MONOTONIC) + 300000, 1000,
            [&instance](fcitx::EventSourceTime *, uint64_t) {
              instance.eventLoop().exit();
              return false;
            });
        instance.eventLoop().exec();
        const auto draft = client.inputPanel().clientPreedit().toString();
        check(draft == "經紀", ("Qwen preview: " + draft).c_str());
        check(!client.inputPanel().candidateList(), "reranking opened the menu");
        client.committed.clear();
        key(engine, entry, client, FcitxKey_Return);
        check(client.committed == draft, "model preview changed at commit");
        engine->deactivate(entry, focus);
      }
      addon.reset();
      dlclose(module);
      std::cout << "native Qwen context reranking passed\n";
      return 0;
    }

    if (argc == 4) {
      {
        Client client(instance.inputContextManager());
        fcitx::InputMethodEntry entry("senti", "SentiIME", "zh_TW", "senti");
        fcitx::InputContextEvent focus(&client,
                                       fcitx::EventType::InputContextFocusIn);
        engine->activate(entry, focus);
        check(!key(engine, entry, client, FcitxKey_s),
              "unavailable backend swallowed native key");
        check(client.committed.empty() &&
                  client.inputPanel().clientPreedit().empty(),
              "unavailable backend committed unexpected text");
      }
      addon.reset();
      dlclose(module);
      std::cout << "native failure passthrough passed\n";
      return 0;
    }
    {
      {
        Client initial(instance.inputContextManager());
        fcitx::InputMethodEntry defaultEntry("senti", "SentiIME", "zh_TW",
                                             "senti");
        fcitx::InputContextEvent initialFocus(
            &initial, fcitx::EventType::InputContextFocusIn);
        engine->activate(defaultEntry, initialFocus);
        check(engine->subModeLabel(defaultEntry, initial) == "混",
              "default entry did not start with Chinese mixed mode");
        for (const auto &[keys, expected] : {std::pair{"s", "s"},
                                             {"u", "su"},
                                             {"3", "你"},
                                             {"c", "你c"},
                                             {"l", "你cl"},
                                             {"3", "你好"}}) {
          type(engine, defaultEntry, initial, keys);
          check(initial.inputPanel().clientPreedit().toString() == expected,
                "mixed preedit must keep Latin keys until a supported "
                "pronunciation is complete");
        }
        key(engine, defaultEntry, initial, FcitxKey_Escape);
        type(engine, defaultEntry, initial, "hello");
        check(initial.inputPanel().clientPreedit().toString() == "hello",
              "default mixed entry did not preserve English");
        key(engine, defaultEntry, initial, FcitxKey_Escape);
        check(initial.inputPanel().clientPreedit().empty(),
              "idle preedit contains a ghost");
        key(engine, defaultEntry, initial, FcitxKey_Shift_L);
        key(engine, defaultEntry, initial, FcitxKey_Shift_L, fcitx::KeyStates(),
            true);
        check(engine->subModeLabel(defaultEntry, initial) == "EN",
              "Shift did not toggle English");
        type(engine, defaultEntry, initial, "su3cl3");
        check(initial.inputPanel().clientPreedit().toString() == "su3cl3",
              "explicit English converted phonetic keys");
        key(engine, defaultEntry, initial, FcitxKey_Escape);
        key(engine, defaultEntry, initial, FcitxKey_Shift_L);
        key(engine, defaultEntry, initial, FcitxKey_Shift_L, fcitx::KeyStates(),
            true);
        type(engine, defaultEntry, initial, "su3cl3");
        check(initial.inputPanel().clientPreedit().toString() == "你好",
              "Shift did not restore mixed input");
        engine->deactivate(defaultEntry, initialFocus);
      }
      {
        Client phonetic(instance.inputContextManager());
        fcitx::InputMethodEntry zhuyinEntry("senti-zhuyin", "SentiIME Zhuyin",
                                            "zh_TW", "senti");
        fcitx::InputContextEvent focus(&phonetic,
                                       fcitx::EventType::InputContextFocusIn);
        engine->activate(zhuyinEntry, focus);
        type(engine, zhuyinEntry, phonetic, "su3cl3");
        check(phonetic.inputPanel().clientPreedit().toString() == "你好",
              "explicit Zhuyin entry cannot input Chinese");
        key(engine, zhuyinEntry, phonetic, FcitxKey_Return);
        check(phonetic.committed == "你好", "explicit Zhuyin could not commit");
        type(engine, zhuyinEntry, phonetic, "D9 VUP ");
        check(phonetic.inputPanel().clientPreedit().toString() == "開心",
              "uppercase keys or first-tone space broke explicit Zhuyin");
        key(engine, zhuyinEntry, phonetic, FcitxKey_Return);
        check(phonetic.committed == "你好開心",
              "first-tone Zhuyin failed to commit");
        engine->deactivate(zhuyinEntry, focus);
      }
      {
        Client punctuation(instance.inputContextManager());
        fcitx::InputMethodEntry mixedEntry("senti", "SentiIME", "zh_TW",
                                           "senti");
        fcitx::InputContextEvent focus(&punctuation,
                                       fcitx::EventType::InputContextFocusIn);
        engine->activate(mixedEntry, focus);
        std::string committed;
        for (const auto &[raw, expected] : {std::pair{".", "."},
                                            {"su3.", "你."},
                                            {"su3cl3.", "你好."},
                                            {"g.3", "手"},
                                            {"su3cl", "你cl"}}) {
          // Each fixture is a fresh document; prior fixture text is not a
          // grammatical context for this one. Long-context behaviour is tested
          // below.
          engine->deactivate(mixedEntry, focus);
          engine->activate(mixedEntry, focus);
          type(engine, mixedEntry, punctuation, raw);
          check(punctuation.inputPanel().clientPreedit().toString() == expected,
                "period/unfinished phonetic preview differs from typed text");
          key(engine, mixedEntry, punctuation, FcitxKey_Return);
          committed += expected;
          check(punctuation.committed == committed,
                "Enter guessed a Chinese character for a literal period");
        }
        engine->deactivate(mixedEntry, focus);
      }
      {
        Client mixed(instance.inputContextManager());
        fcitx::InputMethodEntry entry("senti", "SentiIME", "zh_TW", "senti");
        fcitx::InputContextEvent focus(&mixed,
                                       fcitx::EventType::InputContextFocusIn);
        engine->activate(entry, focus);
        for (const auto &[reading, expected] :
             {std::pair{"ji3sk7", "我呢"}, std::pair{"ji3sk ", "我sk "}}) {
          type(engine, entry, mixed, reading);
          check(mixed.inputPanel().clientPreedit().toString() == expected,
                "mixed neutral particle used the wrong Space policy");
          key(engine, entry, mixed, FcitxKey_Return);
          check(mixed.committed == expected,
                "mixed neutral particle preview differed from commit");
          mixed.committed.clear();
        }
        engine->deactivate(entry, focus);
        engine->activate(entry, focus);
        type(engine, entry, mixed, "tomo");
        check(mixed.inputPanel().clientPreedit().toString() == "tomorrow",
              "mixed English word hint was not displayed");
        check(!mixed.inputPanel().candidateList(),
              "word hint unexpectedly opened the candidate window");
        key(engine, entry, mixed, FcitxKey_Tab);
        check(mixed.committed == "tomorrow ",
              "Tab did not accept the visible word");
        mixed.committed.clear();
        engine->deactivate(entry, focus);
        engine->activate(entry, focus);
        type(engine, entry, mixed, "up");
        key(engine, entry, mixed, FcitxKey_Down);
        key(engine, entry, mixed, FcitxKey_Return);
        check(mixed.committed == "因", "could not select 因 without typing 為");
        mixed.committed.clear();
        engine->deactivate(entry, focus);
        engine->activate(entry, focus);
        type(engine, entry, mixed, "u uv, ");
        check(mixed.inputPanel().clientPreedit().toString() == "一些",
              "transposed first-tone phrase did not convert");
        key(engine, entry, mixed, FcitxKey_Return);
        check(mixed.committed == "一些", "first-tone phrase failed to commit");
        mixed.committed.clear();
        engine->deactivate(entry, focus);
        engine->activate(entry, focus);
        type(engine, entry, mixed, "up ");
        check(mixed.inputPanel().clientPreedit().toString() == "因",
              "a completed first tone did not prioritize Chinese");
        check(!mixed.inputPanel().candidateList(),
              "a completed first tone opened candidates without Down");
        key(engine, entry, mixed, FcitxKey_Return);
        check(mixed.committed == "因", "standalone first tone changed at commit");
        mixed.committed.clear();
        type(engine, entry, mixed, "up there ");
        check(mixed.committed == "up there ", "known English continuation was lost");
        mixed.committed.clear();
        engine->deactivate(entry, focus);
        engine->activate(entry, focus);
        type(engine, entry, mixed, "up jo4");
        check(mixed.inputPanel().clientPreedit().toString() == "因為",
              "English up prevented a first-tone dictionary word");
        key(engine, entry, mixed, FcitxKey_Return);
        check(mixed.committed == "因為",
              "mixed first-tone word failed to commit");
        mixed.committed.clear();
        type(engine, entry, mixed, "su3cl3qzz");
        check(mixed.inputPanel().clientPreedit().toString() == "你好qzz",
              "invalid phonetic tail reverted Chinese");
        key(engine, entry, mixed, FcitxKey_Return);
        check(mixed.committed == "你好qzz", "commit lost the Chinese prefix");
        engine->deactivate(entry, focus);
        engine->activate(entry, focus);
        type(engine, entry, mixed, "1j6e.45 cjo4");
        check(mixed.inputPanel().clientPreedit().toString() == "不夠智慧",
              "degree context did not correct the mistyped tone");
        key(engine, entry, mixed, FcitxKey_Return);
        check(mixed.committed == "你好qzz不夠智慧",
              "tone correction changed at commit");
        engine->deactivate(entry, focus);
      }
      Client first(instance.inputContextManager());
      fcitx::InputMethodEntry entry("senti", "SentiIME", "zh_TW", "senti");
      fcitx::InputContextEvent activated(&first,
                                         fcitx::EventType::InputContextFocusIn);
      engine->activate(entry, activated);
      const auto updatesBefore = first.preeditUpdates;
      type(engine, entry, first, "su3cl3");
      check(first.preeditUpdates - updatesBefore == 6,
            "each key published an intermediate empty preedit");
      check(first.inputPanel().clientPreedit().toString() == "你好",
            "live Chinese preedit missing");
      check(first.inputPanel().clientPreedit().toStringForCommit().empty(),
            "unconfirmed preedit can auto-commit on focus loss");
      check(!first.inputPanel().candidateList(),
            "candidate window opened without Down");
      check(key(engine, entry, first, FcitxKey_Down),
            "Down did not open candidates");
      auto list = first.inputPanel().candidateList();
      check(list && list->size() > 0 &&
                list->candidate(0).text().toString() == "好",
            "Chinese candidate missing");
      check(list->candidate(0).comment().empty(),
            "candidate contains extra annotations");
      check(first.inputPanel().auxUp().toString().find(" · ") ==
                std::string::npos,
            "candidate header still contains phonetic metadata");
      check(first.inputPanel().clientPreedit().cursor() == 3,
            "Down did not focus the final character");
      check(key(engine, entry, first, FcitxKey_space),
            "Space was not handled while selecting");
      check(first.committed.empty() &&
                first.inputPanel().clientPreedit().toString() == "你好",
            "Space lost the editable candidate");
      key(engine, entry, first, FcitxKey_Down);
      check(static_cast<bool>(first.inputPanel().candidateList()),
            "Space prevented correction of the marked candidate");
      key(engine, entry, first, FcitxKey_End);
      check(key(engine, entry, first, FcitxKey_Left),
            "Left did not edit marked text");
      check(first.inputPanel().clientPreedit().cursor() == 3,
            "marked text cursor did not move before previous character");
      check(key(engine, entry, first, FcitxKey_Home), "Home not consumed");
      check(first.inputPanel().clientPreedit().cursor() == 0,
            "Home cursor incorrect");
      check(key(engine, entry, first, FcitxKey_End), "End not consumed");
      list = first.inputPanel().candidateList();
      // At the end of an editing session there is no active target candidate.
      check(!list, "end cursor still shows stale candidates");
      key(engine, entry, first, FcitxKey_Down);
      check(first.inputPanel().clientPreedit().cursor() == 3,
            "Down after End did not focus the final character");
      list = first.inputPanel().candidateList();
      check(list && list->size() > 0, "focused character has no candidates");
      check(list->layoutHint() == fcitx::CandidateLayoutHint::Vertical,
            "candidate menu is not vertical");
      check(first.inputPanel().auxUp().toString().find("候選 1/") == 0,
            "candidate page heading missing");
      key(engine, entry, first, FcitxKey_Escape);
      check(!first.inputPanel().candidateList(),
            "Escape did not hide candidates");
      check(key(engine, entry, first, FcitxKey_Return), "Enter not consumed");
      check(first.committed == "你好", "commit did not reach target client");
      // Stale UI callbacks must not select a candidate from a later revision.
      list->candidate(0).select(&first);
      check(first.committed == "你好",
            "stale candidate callback committed text");
      type(engine, entry, first, "hello ");
      check(first.committed == "你好hello ", "English mixed commit failed");
      type(engine, entry, first, "u");
      key(engine, entry, first, FcitxKey_Down);
      list = first.inputPanel().candidateList();
      check(list && list->toPageable() && list->toPageable()->hasNext(),
            "native paging is missing");
      const auto cursorBeforePage = first.inputPanel().clientPreedit().cursor();
      check(key(engine, entry, first, FcitxKey_Right),
            "Right was not consumed while selecting");
      auto secondPage = first.inputPanel().candidateList();
      check(secondPage && secondPage->toPageable()->currentPage() == 1 &&
                first.inputPanel().clientPreedit().cursor() == cursorBeforePage,
            "Right did not turn the page in place");
      check(key(engine, entry, first, FcitxKey_Up), "Up was not consumed");
      auto previousPage = first.inputPanel().candidateList();
      check(previousPage && previousPage->toPageable()->currentPage() == 0 &&
                previousPage->cursorIndex() == previousPage->size() - 1,
            "Up did not cross to the last candidate on page one");
      check(key(engine, entry, first, FcitxKey_Down), "Down was not consumed");
      check(first.inputPanel().candidateList()->toPageable()->currentPage() == 1,
            "Down did not cross to page two");
      check(key(engine, entry, first, FcitxKey_KP_Prior),
            "keypad PageUp was not consumed");
      check(first.inputPanel().candidateList()->toPageable()->currentPage() == 0,
            "keypad PageUp did not reach page one");
      check(key(engine, entry, first, FcitxKey_Page_Down),
            "PageDown was not consumed");
      check(first.inputPanel().candidateList()->toPageable()->currentPage() == 1,
            "PageDown did not reach page two");
      auto clicked = first.inputPanel().candidateList();
      auto chosen = clicked->candidate(0).text().toString();
      check(key(engine, entry, first, FcitxKey_1), "page-two digit was not consumed");
      check(first.inputPanel().clientPreedit().toString() == chosen,
            "keyboard choice from page two did not remain editable");
      key(engine, entry, first, FcitxKey_Return);
      check(first.committed == "你好hello " + chosen,
            "page-two keyboard selection failed");
      type(engine, entry, first, "su3");
      check(!key(engine, entry, first, FcitxKey_c, fcitx::KeyState::Ctrl),
            "Ctrl+C was swallowed");
      check(first.inputPanel().clientPreedit().empty(),
            "shortcut left pending preedit");
      check(!key(engine, entry, first, FcitxKey_Tab), "Tab was swallowed");
      check(key(engine, entry, first, FcitxKey_comma, fcitx::KeyState::Ctrl),
            "Ctrl+comma was not consumed");
      check(first.committed.ends_with("，"), "full-width punctuation missing");
      type(engine, entry, first, "su3cl3!");
      check(first.committed.ends_with("你好!"), "mixed punctuation was not half-width");
      type(engine, entry, first, "su3cl3");
      key(engine, entry, first, FcitxKey_Home);
      key(engine, entry, first, FcitxKey_question, fcitx::KeyState::Shift);
      check(first.committed.ends_with("你好?"), "edited mixed punctuation was not half-width");
      check(key(engine, entry, first, FcitxKey_question,
                fcitx::KeyStates(fcitx::KeyState::Ctrl) | fcitx::KeyState::Shift),
            "Ctrl+Shift+question was not consumed");
      check(first.committed.ends_with("？"), "shifted full-width punctuation missing");
      type(engine, entry, first, "su3");
      Client second(instance.inputContextManager());
      fcitx::InputContextEvent secondFocus(
          &second, fcitx::EventType::InputContextFocusIn);
      engine->activate(entry, secondFocus);
      check(first.inputPanel().empty(),
            "previous field retained stale preedit");
      type(engine, entry, second, "hello");
      key(engine, entry, second, FcitxKey_Return);
      check(second.committed == "hello", "composition leaked across contexts");
      second.setCapabilityFlags(
          fcitx::CapabilityFlags(fcitx::CapabilityFlag::Preedit) |
          fcitx::CapabilityFlag::Password);
      check(!key(engine, entry, second, FcitxKey_s),
            "password key intercepted");
      check(second.inputPanel().empty(), "password field shows candidates");
      second.setCapabilityFlags(fcitx::CapabilityFlag::Preedit);
      type(engine, entry, second, "su3cl3");
      key(engine, entry, second, FcitxKey_Return);
      check(second.committed == "hello你好",
            "normal field did not recover after password field");
      engine->deactivate(entry, secondFocus);
      check(second.inputPanel().empty(), "deactivation did not clear UI");
      Client third(instance.inputContextManager());
      third.setCapabilityFlags(
          fcitx::CapabilityFlags(fcitx::CapabilityFlag::Preedit) |
          fcitx::CapabilityFlag::SurroundingText);
      third.surroundingText().setText("🙂前文後文", 3, 3);
      fcitx::InputContextEvent thirdFocus(
          &third, fcitx::EventType::InputContextFocusIn);
      engine->activate(entry, thirdFocus);
      type(engine, entry, third, "us3lc3");
      check(third.inputPanel().clientPreedit().toString() == "你好",
            "transposed keys did not convert");
      key(engine, entry, third, FcitxKey_Down);
      third.updateSurroundingText();
      auto stable = third.inputPanel().candidateList();
      third.updateSurroundingText();
      check(third.inputPanel().candidateList() == stable,
            "unchanged surrounding text rebuilt candidate UI");
      key(engine, entry, third, FcitxKey_Escape);
      check(key(engine, entry, third, FcitxKey_Return),
            "corrected text not consumed");
      check(third.committed == "你好", "corrected commit failed");
      type(engine, entry, third, "su3cl3g4ru,4su3cl3g4ru,4");
      check(third.inputPanel().clientPreedit().toString() == "你好世界你好世界",
            "local window damaged a previous word");
      key(engine, entry, third, FcitxKey_Home);
      key(engine, entry, third, FcitxKey_Down);
      auto local = third.inputPanel().candidateList();
      bool replaced = false;
      for (int i = 0; local && i < local->size(); ++i) {
        if (local->candidate(i).text().toString() == "妳好") {
          local->candidate(i).select(&third);
          replaced = true;
          break;
        }
      }
      check(replaced && third.inputPanel().clientPreedit().toString() ==
                            "妳好世界你好世界",
            "word replacement changed neighbours");
      engine->deactivate(entry, thirdFocus);
      Client fourth(instance.inputContextManager());
      fourth.setCapabilityFlags(
          fcitx::CapabilityFlags(fcitx::CapabilityFlag::Preedit) |
          fcitx::CapabilityFlag::SurroundingText);
      const std::string document = "i want ";
      fourth.surroundingText().setText(document, document.size(),
                                       document.size());
      fcitx::InputContextEvent fourthFocus(
          &fourth, fcitx::EventType::InputContextFocusIn);
      engine->activate(entry, fourthFocus);
      type(engine, entry, fourth, "some");
      check(!fourth.inputPanel().candidateList(),
            "context suggestions opened popup without Down");
      const auto firstGhost = fourth.inputPanel().clientPreedit().toString();
      check(firstGhost.starts_with("some ") && firstGhost != "some eats",
            "document-context ghost missing or malformed");
      check(fourth.inputPanel().clientPreedit().cursor() == 4,
            "cursor moved into unaccepted ghost");
      check(fourth.inputPanel().clientPreedit().toStringForCommit().empty(),
            "ghost can auto-commit on focus loss");
      check(key(engine, entry, fourth, FcitxKey_Tab),
            "Tab did not accept ghost");
      check(fourth.committed == firstGhost + " ", "ghost accepted incorrectly");
      // Start a fresh document context. After accepting the first continuation,
      // the model may correctly abstain for the changed sentence.
      engine->deactivate(entry, fourthFocus);
      engine->activate(entry, fourthFocus);
      type(engine, entry, fourth, "some");
      const auto secondGhost = fourth.inputPanel().clientPreedit().toString();
      key(engine, entry, fourth, FcitxKey_Down);
      auto suggestions = fourth.inputPanel().candidateList();
      bool accepted = false;
      for (int i = 0; suggestions && i < suggestions->size(); ++i) {
        if (suggestions->candidate(i).text().toString() == secondGhost) {
          key(engine, entry, fourth, fcitx::Key::keySymFromUnicode('1' + i));
          accepted = true;
          break;
        }
      }
      check(accepted &&
                fourth.committed == firstGhost + " " + secondGhost + " ",
            "Down/numeric context acceptance failed");
      engine->deactivate(entry, fourthFocus);
      Client fifth(instance.inputContextManager());
      fcitx::InputContextEvent fifthFocus(
          &fifth, fcitx::EventType::InputContextFocusIn);
      engine->activate(entry, fifthFocus);
      type(engine, entry, fifth, "ru,4gl4u vu84su3y4ru31u/4fu,3y42j/4vm03y4");
      check(fifth.inputPanel().clientPreedit().toString() ==
                "介紹一下你自己並且自動選字",
            "native Chinese look-ahead decoding regressed");
      check(!fifth.inputPanel().candidateList(),
            "Chinese neural decoding opened popup without Down");
      key(engine, entry, fifth, FcitxKey_Return);
      check(fifth.committed == "介紹一下你自己並且自動選字",
            "native Chinese sentence commit differs from preview");
      engine->reset(entry, fifthFocus);
      fifth.committed.clear();
      type(engine, entry, fifth, "wj3");
      key(engine, entry, fifth, FcitxKey_Home);
      type(engine, entry, fifth, "su6");
      const auto inserted = fifth.inputPanel().clientPreedit().toString();
      check(inserted == "泥土",
            ("native insertion preview: " + inserted).c_str());
      check(!fifth.inputPanel().candidateList(),
            "native insertion opened candidates without Down");
      key(engine, entry, fifth, FcitxKey_Return);
      check(fifth.committed == "泥土", "right-context preview differed from commit");
      fifth.committed.clear();
      type(engine, entry, fifth, "d9 vup ");
      key(engine, entry, fifth, FcitxKey_Down);
      auto emotion = fifth.inputPanel().candidateList();
      check(emotion && emotion->size() > 0, "text candidates unavailable");
      for (int i = 0; i < emotion->size(); ++i) {
        check(emotion->candidate(i).text().toString() != "😊",
              "emoji appeared in the default candidate list");
      }
      key(engine, entry, fifth, FcitxKey_Escape);
      key(engine, entry, fifth, FcitxKey_Return);
      check(fifth.committed == "開心", "default emotion word lost its text");
      fifth.committed.clear();
      type(engine, entry, fifth, "b06c.4vu. 5/4");
      key(engine, entry, fifth, FcitxKey_Home);
      key(engine, entry, fifth, FcitxKey_Down);
      auto word = fifth.inputPanel().candidateList();
      check(word && word->size() > 0, "word candidates unavailable");
      bool selectedWord = false;
      for (int i = 0; i < word->size(); ++i) {
        const auto text = word->candidate(i).text().toString();
        check(text.find("修") == std::string::npos,
              "correction menu included the next word");
        if (text == "然後") {
          word->candidate(i).select(&fifth);
          selectedWord = true;
          break;
        }
      }
      check(selectedWord, "current word was not selectable");
      check(fifth.inputPanel().clientPreedit().toString() == "然後修正",
            "correcting the current word damaged the suffix");
      key(engine, entry, fifth, FcitxKey_Return);
      check(fifth.committed == "然後修正", "local correction changed at commit");
      engine->reset(entry, fifthFocus);
      fifth.committed.clear();
      type(engine, entry, fifth, "su3cl3ru,4gl4u vu84su3");
      key(engine, entry, fifth, FcitxKey_End);
      type(engine, entry, fifth, "y4ru3");
      const auto contextual = fifth.inputPanel().clientPreedit().toString();
      check(contextual.ends_with("自己"), "appended reading did not revise the current word");
      key(engine, entry, fifth, FcitxKey_Return);
      check(fifth.committed == contextual, "contextual repair changed at commit");
      fifth.committed.clear();
      type(engine, entry, fifth, "c9 ");
      check(fifth.inputPanel().clientPreedit().toString() == "嗨",
            "first-tone casual greeting did not rank first");
      key(engine, entry, fifth, FcitxKey_Return);
      check(fifth.committed == "嗨", "casual greeting commit differed from preview");
      engine->deactivate(entry, fifthFocus);
      Client sixth(instance.inputContextManager());
      fcitx::InputContextEvent sixthFocus(
          &sixth, fcitx::EventType::InputContextFocusIn);
      engine->activate(entry, sixthFocus);
      for (const auto &[raw, expected] :
           {std::pair{"wjo ru045 c.4", "推薦之後"},
            std::pair{"1j3fm06j06jp6y4", "補全完文字"},
            std::pair{"y xul4ru6", "資料集"}}) {
        engine->reset(entry, sixthFocus);
        type(engine, entry, sixth, raw);
        check(sixth.inputPanel().clientPreedit().toString() == expected,
              "expanded Chinese word decoding failed");
        auto remaining = std::string(expected);
        while (!remaining.empty()) {
          remaining.erase(
              remaining.size() -
              3); // BMP CJK fixtures: one scalar is three UTF-8 bytes.
          key(engine, entry, sixth, FcitxKey_BackSpace);
          check(sixth.inputPanel().clientPreedit().toString() == remaining,
                "Backspace split a converted character into raw Latin keys");
        }
        check(sixth.committed.empty(),
              "deleting marked Chinese text committed it");
      }
      engine->deactivate(entry, sixthFocus);
      {
        Client managed(instance.inputContextManager());
        fcitx::InputContextEvent focus(&managed,
                                       fcitx::EventType::InputContextFocusIn);
        engine->activate(entry, focus);
        type(engine, entry, managed, "su3");
        key(engine, entry, managed, FcitxKey_Down);
        key(engine, entry, managed, FcitxKey_1);
        key(engine, entry, managed, FcitxKey_Return);
        auto *memoryAction =
            instance.userInterfaceManager().lookupAction("senti-memory");
        const auto statusActions = managed.statusArea().allActions();
        check(memoryAction &&
                  std::find(statusActions.begin(), statusActions.end(),
                            memoryAction) != statusActions.end(),
              "memory action missing from native status area");
        check(memoryAction->menu() &&
                  memoryAction->menu()->actions().size() >= 2,
              "native memory menu did not list committed choices");
        memoryAction->menu()->actions().front()->activate(&managed);
        check(memoryAction->menu()->actions().size() == 1,
              "native memory clear did not refresh the menu");
        for (int repetition = 0; repetition < 2; ++repetition) {
          managed.committed.clear();
          type(engine, entry, managed, "j;6vul3au/6");
          key(engine, entry, managed, FcitxKey_Home);
          for (const std::string_view character : {"王", "曉", "鳴"}) {
            key(engine, entry, managed, FcitxKey_Down);
            bool found = false;
            while (auto words = managed.inputPanel().candidateList()) {
              for (int i = 0; i < words->size(); ++i) {
                if (words->candidate(i).text().toString() == character) {
                  words->candidate(i).select(&managed);
                  found = true;
                  break;
                }
              }
              if (found || !words->toPageable()->hasNext())
                break;
              key(engine, entry, managed, FcitxKey_Page_Down);
            }
            check(found, "could not choose a character in the name");
          }
          key(engine, entry, managed, FcitxKey_Return);
          check(managed.committed == "王曉鳴", "selected name did not commit");
        }
        type(engine, entry, managed, "j;6vul3au/6");
        check(managed.inputPanel().clientPreedit().toString() == "王曉鳴",
              "learned name was split during native composition");
        engine->deactivate(entry, focus);
      }
    }
    addon.reset();
    dlclose(module);
    std::cout << "native Fcitx5 integration passed\n";
  } catch (const std::exception &error) {
    std::cerr << error.what() << '\n';
    return 1;
  }
}

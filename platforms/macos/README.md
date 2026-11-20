# macOS adapter reservation

目前已共用 `senti-platform` 與 `senti-host` 的協定、引擎、詞庫和使用者模型，尚未提供 macOS 原生輸入法 bundle／安裝程式。
Linux 的 POSIX／Fcitx 程式碼不會進入 Rust 共用 crate。

預定以 Swift／Objective-C 的 InputMethodKit adapter 包住主程序：

- `IMKServer`／`IMKInputController` 管理輸入 session，對應唯一 context 與 activate／deactivate。
- 鍵盤事件只選一條路徑送到 backend，避免 inputText 與 key event 對同一鍵重複提交。
- 由 IMKTextInput client 處理 marked text／insertText，`IMKCandidates` 對應候選與 select。
- 安全輸入、失焦、取消 composition 時清空前端候選，不把內容送進其他 session。
- IMKServer 共用一個 Foundation Process／Pipe 啟動同 bundle 中的 `senti-host`，所有 controller 序列化到同一 writer。
- NSRange／caret 使用 UTF-16，與協定 UTF-8 文字明確換算。
- bundle metadata、codesign、公證與輸入來源註冊由 macOS adapter 負責。

主程序預設使用 `~/Library/Application Support/SentiIME/user_model.json`，也可指定 `SENTI_IME_DATA_DIR`。
不需要修改既有學習 JSON 或把 AppKit／InputMethodKit 放進 senti-core／senti-engine。
協定：[PLATFORM_PROTOCOL.md](../../docs/PLATFORM_PROTOCOL.md)。

API 依據：[Apple IMKInputController](https://developer.apple.com/documentation/inputmethodkit/imkinputcontroller)、
[IMKCandidates](https://developer.apple.com/documentation/inputmethodkit/imkcandidates)。

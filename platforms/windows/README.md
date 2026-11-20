# Windows adapter reservation

目前已共用 `senti-platform` 與 `senti-host` 的協定、引擎、詞庫和使用者模型，尚未提供 Windows 原生 DLL／安裝程式。
Linux 的 POSIX／Fcitx 程式碼不會進入 Rust 共用 crate。

預定以 C++ TSF adapter 包住主程序：

- `ITfTextInputProcessor` 管理啟用／停用，對應 activate／deactivate。
- `ITfKeyEventSink` 轉譯按鍵；OnTestKeyDown 不修改 backend，真正 OnKeyDown 才發送一次 key。
- `ITfContext`／edit session 管理組字與 commit，候選 UI 回呼對應 select／翻頁。
- 每個文字欄位保存唯一 context 與上次 mode；偵測密碼／敏感 scope 後不送出內容。
- 每個使用者由單一 broker 管理 `senti-host.exe` 與匿名 pipes；各 TSF instance 透過使用者私有 IPC 進入同一 writer，不可各自覆寫同一份 JSON。
- 共用 JSON Lines；遵守逾時／不重播與版本檢查，broker 的註冊、生命週期與 ACL 留在 Windows adapter。
- OS range／caret 使用 UTF-16，與協定 UTF-8 文字明確換算。
- 文字服務註冊、語言 profile、COM 生命週期、簽章與卸載由 Windows adapter 負責。

主程序預設使用 `%LOCALAPPDATA%\SentiIME\user_model.json`，也可指定 `SENTI_IME_DATA_DIR`。
不需要修改既有學習 JSON 或把 Windows API 放進 senti-core／senti-engine。
協定：[PLATFORM_PROTOCOL.md](../../docs/PLATFORM_PROTOCOL.md)。

API 依據：[Microsoft TSF interfaces](https://learn.microsoft.com/en-us/windows/win32/tsf/text-services-framework-interfaces)、
[ITfTextInputProcessor](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nn-msctf-itftextinputprocessor)、
[文字服務註冊](https://learn.microsoft.com/en-us/windows/win32/tsf/text-service-registration)。

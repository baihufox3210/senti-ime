# Native host protocol v1

Linux Fcitx5、未來 Windows TSF 與 macOS InputMethodKit 共用 `senti-platform`，
由 `senti-host` 子程序管理一個輸入引擎及一份學習模型。
OS 前端只做鍵盤／焦點轉譯、預編輯與候選呈現、上屏；不重做拼音、注音、混打或學習演算法。

## 傳輸

UTF-8 JSON Lines，一行一個請求，一行一個回應，stdout 僅供通訊；每筆回應立即 flush。
命令／回應上限 64 KiB，包含換行；過大或無效輸入會回傳錯誤，並繼續下一行。
Linux 使用私有 socketpair 接到 stdin/stdout；其他平台可用匿名 pipes，wire format 相同。
沒有 TCP port、全域 socket 路徑或網路 API。

```json
{"version":1,"id":1,"command":{"type":"activate","context":"editor-1","mode":"Mixed","app":"Document","sensitive":false}}
{"version":1,"id":2,"command":{"type":"key","context":"editor-1","key":{"name":"character","value":"s"}}}
```

請求的 `version` 必須是 1，`id` 是呼叫端的整數識別碼；回應原樣帶回。
版本錯誤在修改引擎前拒絕。id 用於配對，並不是重複執行保護；前端不可重播失敗的按鍵。
前端應設等待上限，在無回應／格式錯誤時關閉子程序，清除舊候選並放行原生按鍵。

## 命令

| type | 其他欄位 | 行為 |
|---|---|---|
| `ping` | 無 | 版本／程序檢查 |
| `activate` | `context`, `mode`, `chinese_mode`（可省略）, `app`, `sensitive` | 取消舊組字、切換目前欄位、清空舊周圍文字 |
| `deactivate` | `context` | 只停用相符的活動欄位，取消未確認文字 |
| `key` | `context`, `key`, `modifiers`, `release` | 一筆框架按鍵；省略 modifiers／release 代表全 false |
| `select` | `context`, `label` | 以 1 起算的目前頁面序號選字；中文／混打組字保留至明確確認，聯想／英文仍可直接上屏 |
| `surrounding` | `context`, `before_cursor`, `after_cursor`（可省略） | 更新選取範圍前後的 UTF-8 文字供排序，不擴大替換範圍；after_cursor 省略為空 |
| `reset` | `context` | 取消組字／候選與 Shift 狀態，不上屏、不學習 |
| `set_mode` | `context`, `mode` | 切換引擎方案，回應可能含上屏內容 |
| `set_plugin` | `plugin`, `enabled` | 啟停 emoji／datetime／calculator |
| `set_learning` | `enabled` | 全域學習開關，跨 focus 保留；敏感欄位始終不學習 |
| `save` | 無 | 明確儲存；沒有持久化或已保護損壞資料時回傳錯誤 |

`mode` 值為 `Mixed`、`Zhuyin`、`Chinese`（拼音）、`English`。
`chinese_mode` 是 Shift 從英文切回的方案，允許 `Mixed`、`Zhuyin`、`Chinese`，省略時為 `Mixed`。
前端應保存回應的 `view.mode` 和 `view.chinese_mode`，重新啟用同一欄位時一起還原，避免欄位之間互相影響。
`app` 值為 `Chat`、`Email`、`Document`、`Code`、`Search`。
context 是前端產生的非空、不重複欄位識別字串，最多 256 bytes。除了 activate／deactivate，
欄位命令必須符合目前活動 context，否則拒絕；切換回舊欄位需重新 activate，前端可還原該欄位方案。

key 形狀為 `{"name":"character","value":"s"}`，value 是一個 Unicode scalar。
其他 name 不含 value：`space`、`enter`、`escape`、`backspace`、`tab`、`up`、`down`、
`left`、`right`、`home`、`end`、`delete`、`page_up`、`page_down`、`shift`、`toggle_mode`、`commit_raw`、`other`。

modifiers 為 `{"control":false,"alt":false,"shift":false,"meta":false}`。
Shift press/release 必須成對傳入，其他 release 可直接放行；一般按鍵或快捷鍵會取消 Shift 單擊判定。
Ctrl 加支援的標點鍵直接輸入全形中文標點（例如 Ctrl+, → ，、Ctrl+. → 。），先確認現有組字，不把標點當作注音。
原生 Tab 在沒有預選時、以及無組字的游標移動保留系統行為；其他 Ctrl／Alt／Meta 快捷鍵放行並取消未確認組字，
但 Ctrl／Meta+Enter 會保留原始鍵碼上屏。OS 特有全域按鍵可在前端先處理。

## 回應與 OS 動作

```json
{"version":1,"id":2,"ok":true,"consumed":true,"commit":null,"view":{"mode":"Mixed","chinese_mode":"Mixed","preedit":"s","phonetic_preview":"ㄋ","composing":true,"selecting":false,"candidate_cursor":0,"association":false,"candidates":[],"page":0,"page_count":0,"context":{}},"error":null,"warning":null}
```

上例省略候選與完整的 context 詳細欄位；完整 shape 由 `senti_engine::EngineView` 定義。

- `consumed=false`：OS 前端放行目前原生按鍵；其他命令不用解讀此欄位。
- `commit` 非 null：**只向原請求所屬的輸入欄上屏一次**，不可把它再作為按鍵送回引擎。
- `view.preedit`：原始可編輯組字；`view.converted_preedit`：聲調完成即轉成中文的原生組字顯示，未完成部分顯示注音。兩者不代表已上屏或已學習。`phonetic_preview` 供候選窗輔助顯示。
- `view.candidates`：目前頁面候選，含 label、text、annotation、input_mode、source、score、partial。
- `view.selecting`：候選選單開關。前端僅在 true 時建立候選窗，↓ 開啟、Esc 隱藏，其他鍵改變組字會關閉。
- `view.suggestion`：尚未接受的英文接續尾字串，可附在原生組字尾端呈現，必須帶 DontCommit。游標仍停在實際輸入位置；不可將預選文字加入故障恢復提交或學習。原生 Tab 在有預選時接受，沒有時照常放行。
- `view.preedit_cursor`：`converted_preedit` 內的 UTF-8 byte offset，前端需設定原生組字游標；Windows／macOS 需換算 UTF-16。
- `view.page`／`page_count`：0 起算頁碼及總頁數；選字游標也是 0 起算。
- `view.context`：情境分析結果。前端不得以它作為整份宿主文件或游標位置。
- `ok=false`：不要套用回應的內容；停用／重建通訊狀態。`error` 是協定或命令錯誤。
- `warning`：例如使用者模型損壞，功能仍可使用但本次禁止覆寫原檔。
- 沒有活動欄位或 sensitive=true 時 `view=null`，敏感欄位的按鍵回應不 consumed、不 commit。

宿主掌握實際文件與游標；原生前端不可使用 Demo 的 `Session.document` 當作真實文件。
前端負責 UTF-8 / UTF-16 / OS range 換算。組字可左右移動、局部替換、插入及刪除；Linux Text cursor 是 UTF-8 bytes，
Windows TSF／macOS NSRange 需換成對應的 UTF-16 單位，不能直接套用 bytes。
已確認上屏的文件編輯仍由宿主應用程式處理，本版尚未提供選取任意已上屏文字的反查注音重轉換。
舊 UI 回呼必須帶本地 view revision，避免點到已被新一筆按鍵替換的候選。

原生候選目前限制在附近一組最多六個注音字的詞語，並保留已知較長詞語的輸入前綴；更前面的組字保留為已轉換片段供上下文判斷。
局部改詞只替換候選實際覆蓋的音節，不會把游標後面的整句重寫。

## 學習與檔案

同一個 backend 序列化所有欄位的提交，避免每個欄位各持有模型造成最後寫入者覆蓋。
只在完整轉換／英文原文提交時學習；插件、強制原文、敏感欄位維持既有不學習規則。
焦點／reset 不學習未確認文字。語言分區與 schema version 1 保留；新增可省略的 `confirmed_words` 保存明確選字次數，舊檔缺少該欄位時預設為空。

`SENTI_IME_DATA_DIR` 可覆寫原生資料位置；預設 Linux XDG、Windows LOCALAPPDATA、
macOS `~/Library/Application Support/SentiIME`。資料目錄無法推導時只使用記憶體，
不退回宿主工作目錄。可用 `senti-host --ephemeral` 或 `--data-dir PATH` 控制。

## 平台邊界

Linux 現在提供可建置的 Fcitx5 shared-library addon；Windows／macOS 目前僅預留接入規格。
平台實作與 API 參考見 `platforms/windows/README.md`、`platforms/macos/README.md`。

## 個人記憶管理

回應的 `view.memory` 提供 `scope`、`enabled`、`persistent` 與 `count`，不包含全文。
以下為可選的相容新增命令；管理走同一個 host，不能另外啟動 writer 直接修改活動記憶檔。

| 命令 | 欄位 | 行為 |
|---|---|---|
| `memory_scope` | `scope` | 切換 1–160 UTF-8 bytes 的非空範圍，禁止控制字元；取消組字並清空當前前後文 |
| `memory_enabled` | `enabled` | 暫停／恢復本機記憶；不刪除已有資料 |
| `memory_retention` | `days` | 1–3650 天，並清理已過期條目 |
| `memory_list` | 無 | 回應的可選 `memory` 陣列列出目前範圍最新 8 筆條目，包含 id、kind、mode、code、text、at、accepts、context、replaced |
| `memory_page` | `offset` | 從指定偏移取得至多 8 筆；清單回應不附 `view`，維持 64 KiB 封包限制 |
| `memory_delete` | `id` | 只刪目前範圍的指定條目 |
| `memory_clear` | 無 | 清除目前範圍新記憶，保留舊詞庫 |
| `memory_flush` | 無 | 等待背景快照落盤，失敗回傳錯誤 |

敏感欄位期間拒絕以上操作，也不回傳條目。`save` 會同時等待記憶 flush。
`app:` 與 `default` 範圍僅保存用詞；自訂範圍才保存提交脈絡。
目前無法從 InputContext ID 自動識別聊天對象，原生外殼不得把臨時欄位 ID 當作持久對話 ID。

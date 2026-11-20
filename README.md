# SentiIME — 情境感知智慧輸入法（MVP）

<img src="assets/branding/senti-ime.svg" alt="SentiIME：注音ㄅ與英文 a" width="96" height="96">

[SVG Logo 與單色版本](assets/branding/README.md)

以 Rust 2024 開發，具備**情境感知**、**自適應個人化預測**與**插件架構**的輸入法引擎、Linux Fcitx 5 原生前端與互動 Demo。

## Linux 原生輸入法

先建置並測試，不會變更目前的桌面輸入法：

```sh
./scripts/build-linux.sh
ctest --test-dir target/fcitx5 --output-on-failure
```

需要 Fcitx 5 開發套件與 json-c；安裝、啟用及資料位置見
[Linux / Fcitx 5 說明](platforms/linux/fcitx5/README.md)。
安裝後可在一般應用程式透過 Fcitx 輸入 `su3cl3 → 你好`，不需開啟 Demo。
原生組字支援注音錯序／重複鍵容錯、前後文選字、局部改詞、左右游標、插入與刪除；候選列只在按 ↓ 後出現，只顯示序號與文字，每頁最多九筆。
中英文選字使用實際訓練的本機小型神經模型；高信心英文預選可用原生 Tab 明確接受，低信心時不補字。
模型權重、資料來源、重訓方式與驗證限制見 [tiny-ime-v2](models/tiny-ime-v2/README.md)。
注音選字使用[本機神經模型、詞庫與 beam search](docs/LOCAL_LANGUAGE_MODEL.md)，搭配既有個人化學習。

共用的原生主程序與[協定](docs/PLATFORM_PROTOCOL.md)已預留 Windows TSF 與 macOS InputMethodKit 的接入方式；
這兩個平台目前尚未實作原生前端。

## 快速開始

```bash
# 圖形介面 Demo（egui）
cargo run -p senti-gui --release

# 終端機 Demo（逐行輸入，:help 查看指令）
cargo run -p senti-app --bin senti-cli
cargo run -p senti-app --bin senti-cli -- --ephemeral   # 不寫入學習資料
```

學習資料預設寫在工作目錄下的 `.senti-ime/user_model.json`，可用環境變數
`SENTI_IME_DATA_DIR` 改變位置。GUI 會自動尋找系統中文字型，找不到時可用
`SENTI_IME_FONT=/path/to/font.ttc` 指定。

## 中文顯示與中英混打

啟動後預設顯示「中英」，同時判讀台灣標準／大千注音與英文；直接輸入英文即可保留英文。Shift 可手動切換純英文模式。
請先將作業系統的輸入法切成英文，讓 Demo 收到原始鍵碼。

| 鍵碼 | 按 Enter 後 |
|---|---|
| `su3cl3` | 你好 |
| `hello` | hello |
| `hello su3cl3` | hello 你好（英文空白會先確認前詞） |
| `hellosu3cl3world` | hello你好world |
| `OpenAIsu3cl3` | OpenAI你好 |

Demo 主輸入區先保留正在輸入的字母／鍵碼，完成注音音節後顯示中文；按 ↓ 才顯示候選與注音解讀。
歧義輸入會保留中英文兩種候選；可按 ↓ 開啟選字，以 1–9、方向鍵、滑鼠選擇。

- **Enter**：確認目前首選／整段組字；選字狀態則確認游標候選。
- **Space**：英文保留所打內容並加空白；注音未完成音節時補一聲，已完成聲調時確認中文。
- **Ctrl／Cmd+Enter** 或「保留原文」：略過轉換，保留整段原始鍵碼，不納入學習。
- 英文大小寫、未知英文詞與數字原文可保留；英文補全仍可透過候選選取。
- 連續輸入中，明確的注音與已知英文詞／縮寫前綴可自動分段，分段在上屏前仍可退格復原。
- **Esc**：先離開選字，再取消整段組字；Shift 單擊／Tab 可暫切純英文，再切回混打。

GUI 頂部仍提供「純注音／拼音／English」模式，原有按鍵行為與功能保留。
UI 預設跟隨系統明暗，也可手動切換；設定與注音鍵盤提示皆可展開。
小視窗中的設定會以浮動面板開啟，保留輸入區寬度。

## 台灣標準注音

- 空白：一聲；`6`／`3`／`4`／`7`：二聲／三聲／四聲／輕聲，聲調結束音節。
- 聲調打錯時可重按聲調鍵更正；混打中的純數字串會保留，避免誤刪數字。
- 純注音模式可連續輸入多音節，按 Enter 確認整句；拼音模式仍支援全拼、縮寫與分段選字。
- 切換進出混打或純注音模式會取消未確認組字，保留既有純注音切換規則。

GUI 提供「複製全文」按鈕，並支援 Ctrl／Cmd+C、Ctrl／Cmd+V。貼上時保留原文、
空白與換行，且不把剪貼簿內容納入學習；若正在組字，會先完成轉換，純注音無法轉換則保留顯示中的注音；混打可保留原始鍵碼。
清空或載入範例會一起取消舊組字。

詞庫含 **80,104 個不重複的多字詞語／片語、21,694 個單字**，共 101,798 個不同詞形、
109,923 筆帶聲調注音讀音，包括生活、工作、教育、交通與台灣地名；拼音同步支援這些詞語。
同一詞語的不同聲調／讀音不會重複計入 80,104 詞的規模。

CLI 使用 `:scheme mixed|zhuyin|pinyin|english` 切換方案，`:choose` 開啟選字，
`:right`／`:left` 移動候選，`:enter` 確認，`:raw` 保留原始鍵碼。`--ephemeral` 僅在記憶體學習。
注音資料與授權見 [SOURCES.md](crates/senti-predictors/data/SOURCES.md)。

## 功能

| 類別 | 內容 |
|---|---|
| 中英混打 | 同一列並行注音／英文候選、原始鍵碼與注音解讀、連續分段、保留原文 |
| 注音輸入 | 標準大千鍵盤、五聲、詞語與整句、分段選字、本地學習 |
| 拼音輸入 | 全拼、未完成音節（`nih`）、首字母縮寫（`nh`）、`'` 分隔、分段選字、整句轉換（`woxiangchifan → 我想吃飯`）、全形標點 |
| 英文輸入 | 前綴補全、保留大小寫（`Hel → Hello`、`HEL → HELLO`） |
| 情境感知 | 應用類型（聊天／郵件／文件／程式／搜尋）、情緒、正式程度、時段，即時影響排序 |
| 個人化 | 選字頻率、片語學習、接續詞（聯想），本地 JSON 持久化 |
| 插件 | Emoji（正式場合自動停用）、日期時間（`rq`/`sj`/`xq`）、計算機（`v(1+2)*3`），可即時啟停；日期／計算機快捷碼在混打或拼音模式使用 |

## 拼音／英文按鍵

| 按鍵 | 作用 |
|---|---|
| `a–z` | 輸入拼音／英文 |
| `Space` | 中文：選第一個候選；英文：上屏並加空白 |
| `1–9` / 滑鼠點擊 | 選擇候選詞 |
| `Enter` / `Esc` | 上屏原始字母 / 取消 |
| `- =`、方向鍵、PageUp/Down | 翻頁 |
| `Shift` 單擊 / `Tab` | 中英切換 |

## 開發

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace

# 不連網、不改寫檔案，驗證擴充詞庫與來源清單一致
python3 scripts/import_mcbopomofo.py --check
```

架構說明見 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

混打模式預設顯示原始字母：`s → su → 你 → 你c → 你cl → 你好`。
完成合法注音音節（包含聲調鍵；一聲用空白）才轉換成中文；未完成或無字可選時保留原文。
混打的注音解讀提示只在開啟候選時提供；純注音入口仍保留注音顯示。

## 穩定混打與跨重啟記憶

混打的顯示、Enter 與原生前段切分共用解碼快照，保留原始鍵碼與局部編輯。
新增有工作範圍、保存期限及來源的本機記憶；Linux 可在 SentiIME 設定與狀態選單管理。
預設按應用程式保存已選用詞；設定自訂 MemoryProfile 後，可保存自己提交的對話脈絡。
詳細操作與限制見 [本機模型與記憶](docs/LOCAL_LANGUAGE_MODEL.md)。

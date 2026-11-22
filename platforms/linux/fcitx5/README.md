# Linux / Fcitx 5

Linux 原生前端已實作，使用 Fcitx 5 的輸入欄、預編輯、候選列及上屏 API。
不需要開啟 egui Demo。中英混打、純注音、拼音及英文都由同一個 Rust 引擎處理。

## 建置

需要 Linux、Rust 1.85+、CMake 3.21+、Ninja、C++20 編譯器、Fcitx5Core 5.1.9+ 開發檔及 json-c 0.15+。
請先確認 `pkg-config --modversion Fcitx5Core json-c` 能找到套件。

在 Repo 根目錄執行：

```sh
./scripts/build-linux.sh
ctest --test-dir target/fcitx5 --output-on-failure
```

腳本會建立 release `senti-host` 和 `senti.so`，不安裝、不啟動或重啟你的桌面輸入法。
需要手動指定建置參數時：

```sh
cargo build -p senti-platform --release --locked
cmake -S platforms/linux/fcitx5 -B target/fcitx5 -G Ninja \
  -DCMAKE_INSTALL_PREFIX=/usr -DCMAKE_BUILD_TYPE=Release \
  -DSENTI_HOST_EXECUTABLE="$PWD/target/release/senti-host"
cmake --build target/fcitx5
```

`CMAKE_INSTALL_PREFIX` 必須在設定階段指定；插件會將該 prefix 的 libexec 路徑嵌入執行檔。
安裝時單獨改 `--prefix` 不會重寫已編譯的主程序路徑。

## 安裝與啟用

先看 `target/fcitx5/install_manifest.txt`（安裝／暫存安裝後產生）以核對檔案清單。
一般系統 prefix `/usr` 的安裝指令：

```sh
sudo cmake --install target/fcitx5
```

若圖示仍顯示舊快取或破圖，可執行 `sudo gtk-update-icon-cache -f /usr/share/icons/hicolor`（需系統有此工具）。
圖示使用安裝 prefix 下 `share/pixmaps/senti-ime-panel-v2.png` 的絕對路徑，繞過舊的 SVG／主題名稱快取。
接著執行 `fcitx5 -rd` 重新啟動自己的 Fcitx 5，在 Fcitx 5 設定工具中：

1. 新增 **SentiIME — Mixed / SentiIME 混**，語言為繁體中文。此入口預設顯示「混」，中文注音與英文可直接混打。
   原有的「中文起始」入口保留為相容別名，使用相同的混打判斷。
2. 若找不到，關閉「只顯示目前語言」篩選。
3. 在文字編輯器中切換至 SentiIME，輸入 `su3cl3`，按 Enter 應上屏「你好」。
4. 輸入 `hellosu3cl3world` 再按 Enter，應上屏 `hello你好world`。

另外三個項目是 `SentiIME — zhuyin`、`SentiIME — pinyin`、`SentiIME — english`。
設定工具中可開關個人化學習、Emoji、日期時間與計算機插件。開啟 HomeRowSelection 可在候選窗用 `asdfghjkl` 選字。Emoji 預設關閉，開啟後須手動選取表情候選。

Fcitx 5 必須已能在你的桌面／應用程式中運作。X11／Wayland 和 GTK／Qt／瀏覽器的接入
由 Fcitx 5 與桌面環境提供；SentiIME 不改寫系統的 GTK_IM_MODULE、QT_IM_MODULE、XMODIFIERS
或桌面啟動設定。若其他 Fcitx 輸入法也不能上屏，請先依
[Fcitx 5 設定指南](https://fcitx-im.org/wiki/Setup_Fcitx_5)處理框架／應用程式設定。
原生插件結構遵循 [Fcitx 5 addon 文件](https://fcitx-im.org/wiki/Develop_an_simple_input_method/en)。

## 原生操作

- 聲調完成後，輸入框直接顯示轉換後的中文，例如 `su3cl3` 立即顯示「你好」。組字可退格、選字或用 Ctrl+Enter 上屏原始鍵碼。Enter 上屏目前文字。
- 候選只在按下 ↓ 後出現；採直式 1–9 選單，每頁最多九筆，只顯示序號與文字，標題顯示「候選 1/6」等目前頁碼；不顯示語言、讀音與來源註解。實際底色、圓角與字型由 Fcitx5／桌面候選窗主題控制。
- Ctrl 加標點直接輸入中文符號：`Ctrl+,` → `，`、`Ctrl+.` → `。`、`Ctrl+;` → `；`、`Ctrl+[`／`]` → `「`／`」`、`Ctrl+\` → `、`。問號、驚嘆號等需同時按 Shift 產生對應鍵。
- ↓ 開啟選字；方向鍵、1–9、PageUp／PageDown、候選列翻頁及滑鼠點選都可使用。
- 游標在混打組字末尾時，Shift+↓ 可直接選最後一個詞庫詞。例如在「世界你好」後按 Shift+↓ 可改選「妳好」。
- 用 Fcitx5 的輸入法切換選擇輸入法。單按 Shift 不改變 SentiIME 的模式。
- Ctrl+Enter 保留原始鍵碼，不學習。其他 Ctrl／Alt／Super 快捷鍵交給應用程式，並取消未確認組字。
- 有英文預選時，Tab 明確接受預選；沒有預選時，Tab 保留原生焦點移動功能。Demo 的 Tab 仍是模式切換便利鍵。
- 組字中的 ←／→ 移動游標，Home／End 跳至首尾；候選改為游標右側附近的詞語（最多六個注音字，依詞庫邊界判斷），亦可選單字。↓ 開啟後可按 1–9 選字，也可滑鼠點選；改字會保留後面的文字。
- 游標中間可插入新注音；Backspace 刪前一字、Delete 刪後一字。音節尚未完成時，Backspace 刪除正在輸入的鍵碼。
- 滑鼠、數字與候選窗中的 Space 選字都保留可編輯的組字。平常的 Space 可插入空格，與詞庫吻合時也可完成一聲，例如 `up` 加 Space 上屏「因」。
- ↓ 開啟候選後，會在同讀音候選後加入部分近音備選。完整輸入姓名或術語並確認字形後，可從「個人記憶」選「記住目前組字」。
- 沒有組字時，方向鍵交給應用程式。已上屏文字的游標移動、刪除交給應用程式。
- 切換輸入欄、重設或停用輸入法會取消未確認的組字，不送到另一個欄位。
- 密碼、敏感、限定數字或要求停用 IME 的欄位不送出按鍵／周圍文字、不顯示候選，也不學習。

## 本機神經模型與預選

預設載入已訓練、約 7.1 MB 的雙語神經模型，搭配既有詞庫、聲調與 beam search，直接以 CPU 推論。
舊的猜名詞／加字尾推薦已移除。權重嵌入主程序，正常使用不下載模型、不呼叫雲端。
完整詞語優先，未確認的前段可隨新上下文重新解碼；明確選字仍保留，手動修改只作用於目標詞。

高信心英文預選以斜體顯示，Tab 或候選選取才接受。模型信心不足時不顯示建議；
這個小模型不保證每次都能推斷語意、單複數或長句意圖。候選窗仍只在按 ↓ 時出現。
「介紹一下你自己」「並且」「自動選字」及含舊模型污染的長句已加入回歸測試。

資料、重訓及評估限制見[模型說明](../../../models/tiny-ime-v2/README.md)。

## 資料與程序

預設學習檔位置為 `$XDG_DATA_HOME/senti-ime/user_model.json`；未設定 XDG_DATA_HOME 時，
使用 `~/.local/share/senti-ime/user_model.json`。原生版本不將資料寫入應用程式工作目錄。
若要沿用 Demo 的學習檔，可為啟動 Fcitx 的環境指定 `SENTI_IME_DATA_DIR` 至原資料夾。
GUI／CLI 與原生主程序不應同時寫同一份學習檔。

每個 Fcitx addon 使用一個 `senti-host` 子程序，序列化同一份使用者模型的寫入。
焦點方案保存在各輸入欄，只有目前欄位的組字進入共用引擎；同時使用多個輸入欄時，
切換活動欄位會取消前一欄位的組字。程序只用私有本機 socketpair，不開 TCP 或 D-Bus 服務。
周圍文字最多擷取選取範圍前 400、後 80 個 Unicode 字元作為選字上下文；實際改詞不會擴大成整段前後文。未變更的前後文通知會略過。

RPC 有 500ms 等待上限；子程序退出、無回應或回傳無效封包時會關閉／回收子程序，
三秒內不重複啟動，按鍵改交回應用程式。若故障發生在仍有焦點的普通欄位按鍵處理中，
會保留目前可見的預編輯文字，再放行當次按鍵；其他焦點／敏感欄位不會如此提交。
不重播失敗的按鍵封包，以免重複輸入。日誌不記錄按鍵與文件內容。

開發環境可使用 `SENTI_IME_HOST_BINARY=/absolute/path/to/senti-host` 覆寫子程序位置，
`SENTI_IME_EPHEMERAL=1` 啟用純記憶體學習。這些環境變數需由啟動 Fcitx 的環境繼承。

## 安裝前檢視／封裝

不碰系統目錄即可產生完整安裝樹：

```sh
DESTDIR="$PWD/target/fcitx5-stage" cmake --install target/fcitx5
find target/fcitx5-stage -type f
```

安裝內容包括 addon、四個輸入法登錄檔、Rust 主程序、彩色／單色 SVG 與多尺寸 PNG 圖示及資料來源／授權聲明。
二進位檔依本機 Fcitx、libstdc++、glibc ABI 建置；其他 Linux 發行版建議從原始碼重建。

## 驗證範圍

Rust 測試覆蓋通訊版本、框架按鍵、焦點隔離、敏感欄位、周圍文字、學習與損壞檔案保護。
C++ 測試動態載入真實 addon factory，使用 Fcitx 原生 InputContext 測預編輯、上屏、
候選點選／翻頁、過期回呼、快捷鍵及敏感欄位；另測 IPC 退出、逾時、封包錯誤和故障放行。
這些測試不重啟桌面 Fcitx，也不等同於所有桌面、工具包與應用程式的端對端驗證。

## 移除

先在 Fcitx 設定中移除 SentiIME 項目，再依 `target/fcitx5/install_manifest.txt` 移除本次安裝檔案，
重新啟動 Fcitx。不要刪除整個 fcitx5 目錄；學習資料可保留，移除程式不需清除它。

## 個人記憶

在 Fcitx5 設定工具中開啟 SentiIME 設定：

- `Memory`：開關本機記憶；停用時不自動使用或新增此類記憶。
- `MemoryProfile`：留白時按應用程式保存已選用詞；填入如 `work:atlas` 可保存並跨重啟沿用自己的對話脈絡。
- `MemoryDays`：保存期限，預設 30 天。

狀態選單的「個人記憶」會列出目前範圍的條目；點選可刪除，並有「清除目前範圍的記憶」。
`ClearMemory` 設定也可在下次輸入時清除目前範圍，執行後自動取消勾選。
設定會在下次啟用或輸入時套用；敏感欄位不顯示記憶管理。
新記憶檔位於原生使用者資料目錄的 `personal_memory.json`，與舊詞庫分開。
它只保存本輸入法提交的內容，沒有取得聊天軟體完整對話的能力。

驗證操作契約：`python scripts/replay_input.py`。
含實際存檔與 512 筆合成記憶的效能測試：`python scripts/benchmark_native.py --persistent --memory-entries 512`。

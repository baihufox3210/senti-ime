# 注音資料來源與重建

詞庫由既有種子資料與小麥注音詞庫的高頻詞語與獨立單字組成；單字不再只限於高頻詞內出現的字，仍非 CNS 全字庫的全部字形。

| 統計方式 | 數量 |
|---|---:|
| 不重複多字詞語／片語 | 80104 |
| 不重複單字 | 21694 |
| 不重複文字詞形合計 | 101798 |
| 帶聲調注音讀音 | 109923 |
| 拼音讀音（去除聲調後再去重） | 105982 |

同一詞的異讀不重複計入詞語規模；純中文數字串不納入匯入詞語。
實際統計、輸出與來源 SHA-256 存於 `lexicon_manifest.json`。

## 原有種子資料與全字庫

`zhuyin.tsv` 是 746 筆原有種子讀音，字頻與情境標籤沿用本專案的
`pinyin_chars.tsv`、`pinyin_words.tsv`。

字音與拼音／注音對照取自數位發展部 CNS11643 中文標準交換碼全字庫：

- 資料集：https://data.gov.tw/dataset/5961
- 屬性：https://www.cns11643.gov.tw/opendata/Properties.zip
- Unicode 對照：https://www.cns11643.gov.tw/opendata/MapingTables.zip
- 授權：政府資料開放授權條款第 1 版：https://data.gov.tw/license
- 本專案修改：僅擷取既有詞彙、轉換為標準大千鍵碼、沿用本專案字頻與標籤，並補充詞語讀音。

下載日期：2026-11-20。下載檔案 SHA-256：

```
Properties.zip   3d56ef14cc8099893245dac58fe4718d2fa64812b9159352a98a4588ad3efa5c
MapingTables.zip 4502fcf7b433d679dee51127298929543ec7f4aa99be93cd219df1552bc3d2bf
```

重建（不會自動連網）：

```sh
python3 scripts/build_zhuyin.py /path/to/Properties.zip /path/to/MapingTables.zip
```

`zhuyin_overrides.tsv` 明確記錄多音字在詞中的選音、輕聲與台灣用詞對應，
以詞典本調為準（例如「你好」保留兩個三聲，不套用口語連讀變調）。
「休息」採台灣國語的 ㄒㄧㄡˇ ㄒㄧˊ，「星期／日期」採 ㄑㄧˊ。
「軟體、變數、模組、人工智慧、智慧」覆寫既有拼音詞庫中與文字不符的編碼。

第一欄是 ASCII 鍵碼，**尾端空白表示一聲，不可 trim**。
37 個注音鍵對應見 `senti_core::zhuyin`；聲調鍵是空白、6、3、4、7。
執行時直接嵌入 TSV，不依賴網路、外部字典或下載腳本。

## 台灣日常詞彙補充

另補充 29 個交通、生活與地名詞彙（如捷運、公車、便當、悠遊卡、新北、桃園、
高雄，以及部分「台／臺」異體詞形），同時提供拼音與注音。
詞語及本調明確列在 `pinyin_words.tsv`、`zhuyin_overrides.tsv`；
頻率是本專案設定的排序種子，並非來自外部語料的實測統計。
重新執行上述腳本會一併產生這些詞條，包含於 746 筆種子讀音中。

## 小麥注音高頻詞語擴充

來源：[McBopomofo 小麥注音](https://github.com/openvanilla/McBopomofo)，固定版本
`1b7fbb999a354ba80e5cf20820ddd4093c6ed77e`（2026-11-17）。
使用 `Source/Data/BPMFMappings.txt` 的詞語及讀音、`BPMFBase.txt` 的單字資料，
以及 `phrase.occ` 的詞頻。小麥注音為 MIT 授權，詞語映射承襲 libtabe 的 BSD 資料。
完整保留授權於 `licenses/McBopomofo-MIT.txt`、`licenses/libtabe-COPYING.txt`；
兩者適用於匯入子集及其衍生輸出。

篩選與轉換規則：

- 保留原有種子詞語，依上游詞頻補足至 80000 個不同多字詞語／片語。
- 匯入詞語限 2–6 個漢字（取消 Big5 編碼限制），排除純數字串、零詞頻及不支援的音節。
- 沿用上游完整詞語讀音，保留有記載的異讀與輕聲；不以單字讀音猜測多音字。
- 匯入 `BPMFBase.txt` 中可對應合法注音／拼音的獨立漢字讀音，包含少見字與 CJK 擴充字；一併補齊詞語所需讀音；詞中輕聲可能不在上游獨立單字表，仍依原詞讀音保留。
- 以全字庫拼音對照轉為本專案的漢語拼音（`v` 表示 `ü`），與注音提供相同詞語覆蓋。
- 匯入詞頻按比例縮放：多字詞最高 3000，單字最高 2000，最低 1；原有種子標籤保留。
- 以 `(編碼, 文字)` 去重；拼音去聲調後可能合併多種注音讀音。

`mcbopomofo_subset.tsv` 保留實際採用的上游文字、注音、詞頻，另附轉換後的拼音與排序頻率。
`zhuyin_extra.tsv`、`pinyin_extra.tsv` 為程式直接嵌入的衍生資料，執行時不連網。
擴充資料不修改舊學習資料與編碼格式。

一般離線重建與檢查（只需 Repo 內的子集檔）：

```sh
python3 scripts/import_mcbopomofo.py
python3 scripts/import_mcbopomofo.py --check
```

重新從完整上游檔案篩選：先從上述固定版本下載 `BPMFMappings.txt`、`BPMFBase.txt`、
`phrase.occ` 至同一資料夾，另備妥上文指定的 `Properties.zip`，再執行：

```sh
python3 scripts/import_mcbopomofo.py --source-dir /path/to/upstream --properties /path/to/Properties.zip
```

匯入器會驗證固定來源的 SHA-256，拒絕未核對的其他版本。上游檔案不需隨程式散布；
實際採用子集、授權與檔案雜湊均已保留。若先重建了種子資料，請接著重建擴充資料與清單。

## 本專案領域詞補充

`domain_terms.tsv` 明列 137 筆台灣軟體、資料與輸入法領域詞條的注音及拼音，
包含「補全、資料集」。其中部分與上游重疊；合併後共有 80,104 個不同多字詞。
頻率 500／1400 是人工排序種子，並非自然語料統計。匯入器合併、去重並記錄檔案 SHA-256。

`domain_sentences.txt` 是本專案撰寫模板產生的 11,564 句受控訓練句，
補充動作完成「完」與玩耍「玩」、推薦及資料領域的上下文。
它不是獨立自然語料或全場景測試集；完整回歸句「推薦之後」「補全完文字」刻意排除。
重建及檢查：`python scripts/build_domain_corpus.py`、`python scripts/build_domain_corpus.py --check`。
這兩項新增資料依本專案 MIT 授權。

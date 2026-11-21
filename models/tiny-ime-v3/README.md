# Senti Tiny IME v3 研究模型

v3 從 v2 擴大後繼續訓練。詞彙表維持 11,779 項，向量維度 32 → 48，隱藏層 96 → 160，順序上下文 6 → 8，主題視窗 12 → 16。可訓練參數從 1,542,837 增至 2,533,989。權重及詞性分布等資料共 11,031,192 bytes。

訓練採 v2 權重初始化，再以原訓練集訓練四輪，每輪抽樣 110,000 個位置。公開來源、詞庫快照與授權延續 [v2](../tiny-ime-v2/README.md)。語料使用 UD English EWT、Chinese GSD、Tatoeba CC0 與 repo 的詞庫和領域補充句。模型與其衍生權重適用 v2 的 CC BY-SA 4.0 授權與來源聲明。

| 評測 | v2 | v3 |
|---|---:|---:|
| 英文下一詞首選（留出語料） | 15.55% | 15.70% |
| 中文下一字首選（留出語料） | 12.70% | 12.00% |
| 中文同音字限制候選，僅神經分數（9,717 個位置） | 73.25% | 73.29% |
| 中文同音字限制候選，詞頻加神經分數 | 65.07% | 65.12% |

同音評測使用 UD GSD test 中有唯一詞庫讀音的單字，以相同聲調及讀音的固定候選進行對照。這兩種評測的範圍是單字或單詞位置。

試接入 v3 時，原有英文補全案例失去候選，另一個句子中的「他」改選成「她」。預設 Linux 版採用 v2。訓練報告記錄初始化模型和資料雜湊。其餘 JSON 檔保存各項詳細結果。

```sh
OPENBLAS_NUM_THREADS=2 OMP_NUM_THREADS=2 python scripts/train_neural.py \
  --output models/tiny-ime-v3 --embedding 48 --hidden 160 --context 8 --topic 16 \
  --warm-start models/tiny-ime-v2 --epochs 4 --examples-per-epoch 110000
python scripts/evaluate_neural.py --model-dir models/tiny-ime-v3
python scripts/evaluate_zhuyin_homophones.py models/tiny-ime-v2 models/tiny-ime-v3
```

訓練前須先準備 v2 文件列出的語料。若本機缺少 v2 的訓練 checkpoint，可使用該版文件中的重訓命令重建。

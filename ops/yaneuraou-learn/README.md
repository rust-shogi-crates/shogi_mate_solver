用意する物

- yaneuraou データセット TODO
- Ruby、Cargo（学習スクリプトには Bash、mkfifo、tee も必要）

以下はリポジトリのルートで実行する。実行ごとに変わるパスは、各スクリプトの引数で指定する。各スクリプトは `--help` で使い方を表示する。

# テストデータ作成

生成済みの教師データから、学習側の攻め方局面を詰み16件・不詰み16件、合計32件抽出する。これは学習データ上の性能評価用であり、未学習データでの評価用ではない。

```bash
# 今回使う教師データと、抽出結果の保存先
EXAMPLES='nnue_training/work/yaneuraou-examples-regenerated.jsonl'
SEARCH_DATA='nnue_training/work/compare/train-search.jsonl'

ruby ops/yaneuraou-learn/sample.rb "$EXAMPLES" "$SEARCH_DATA"
```

引数は `INPUT.jsonl OUTPUT.jsonl`。出力先の親フォルダは作成する。既存の出力ファイルは上書きするが、抽出件数が不足した場合は書き換えない。

source_idによる80/20分割とseedは学習スクリプトと同じ。抽出seedは20261101で固定し、同じ入力からは同じ32件を選ぶ。

# 学習

```bash
# 教師データと初期モデルを指定。保存先は自動で新規作成される
bash nnue_training/train.sh "$EXAMPLES" nnue_training/work/nnue-initial.nnue
```

`Output directory: XXXX` と表示される。保存先を自分で指定する場合は、第3引数にまだ存在しないフォルダを渡す。

```bash
bash nnue_training/train.sh "$EXAMPLES" nnue_training/work/nnue-initial.nnue nnue_training/work/my-run
```

# テスト

モデルフォルダ内の `nnue-initial.nnue` とすべての `nnue-epoch*.nnue` を評価する。探索上限は従来どおり20000局面。releaseビルド後に順次実行し、失敗時は停止する。

```bash
# 学習時に表示された保存先に置き換える
RUN='nnue_training/work/my-run'

# 別系統の5局面
bash ops/yaneuraou-learn/benchmark.sh \
  "$RUN" benchmark/issue16-ordering.jsonl "$RUN/benchmark/external"

# 教師データから抽出した32局面
bash ops/yaneuraou-learn/benchmark.sh \
  "$RUN" "$SEARCH_DATA" "$RUN/benchmark/train"
```

引数は `MODEL_DIR DATASET.jsonl OUTPUT_DIR`。結果は `OUTPUT_DIR/nnue-initial.jsonl`、`nnue-epoch03.jsonl` 等に、ログは同名の `.log` に保存する。同じ出力フォルダで再実行すると同名の結果・ログを上書きするため、比較条件ごとに出力フォルダを分ける。

# 表の作成

```bash
ruby ops/yaneuraou-learn/table.rb "$RUN/benchmark/external"
ruby ops/yaneuraou-learn/table.rb "$RUN/benchmark/train"
```

引数は `RESULTS_DIR`。フォルダ内の初期モデルと各epochの結果から、正解数・上限到達数・探索局面数・初期比・合計時間のMarkdown表を標準出力へ表示する。ファイルに保存する場合は次のように実行する。

```bash
ruby ops/yaneuraou-learn/table.rb "$RUN/benchmark/train" > "$RUN/benchmark/train/table.md"
```

時間は各ファイルに記録された測定値の合計。evalの時間は事前DFPNを含まず、探索局面数は含む。

# nnue_training

`nnue_training`には、初期モデルを作る`init`、benchmarkの検索結果からmove-ordering用の学習例を生成する`generate`、学習例からモデルを学習する`learn`、モデルのスコアを確認する`score`の4つのバイナリがある。`generate`は各局面を指定したevaluatorで再評価し、mateを`1`、nomateを`0`とする。学習用ファイルはworktree内の`nnue_training/work/`に置き、repositoryにはcommitしない。

ファイル形式の詳細は[`format.md`](format.md)にまとめてある。

## 使い方

```sh
# 学習用ファイルを置くworktree内のディレクトリを作る
mkdir -p nnue_training/work

# ベンチマーク問題を実行し、検索結果をJSONLに保存する
cargo run --release -p benchmark_harness -- run --strict --move-ordering=nnue-fixture --revision=fixture benchmark/issue16-ordering.jsonl > nnue_training/work/benchmark-results.jsonl

# ベンチマーク結果から学習例を生成する。左右反転と0〜2手先の局面も追加する
cargo run --release -p nnue_training --bin generate -- --positions benchmark/issue16-ordering.jsonl --results nnue_training/work/benchmark-results.jsonl --output nnue_training/work/nnue-examples.jsonl --evaluator=eval --mirror --plies=2

# 学習開始時点の空の仮置きNNUEモデルを作る
cargo run --release -p nnue_training --bin init -- --output nnue_training/work/nnue-initial.nnue

# 学習開始モデルを入力し、学習例からランタイム形式のモデルを学習して保存する
cargo run --release -p nnue_training --bin learn -- --model nnue_training/work/nnue-initial.nnue --examples nnue_training/work/nnue-examples.jsonl --output nnue_training/work/nnue-model.nnue

# モデルで各学習例を採点し、集計結果を標準出力に表示する
cargo run --release -p nnue_training --bin score -- --model nnue_training/work/nnue-model.nnue --examples nnue_training/work/nnue-examples.jsonl

# モデルで各学習例を採点し、個別の結果をJSONLファイルに保存する
cargo run --release -p nnue_training --bin score -- --model nnue_training/work/nnue-model.nnue --examples nnue_training/work/nnue-examples.jsonl --output-file nnue_training/work/nnue-scores.jsonl
```

`learn`の入力モデルは、学習開始時点の重みを指定する。`init`は学習開始用の空の仮置きモデルを作る。現在の`init`と`learn`は`512 -> 32 -> 32 -> 1`の`NNUE-FIXTURE 1`モデルを使い、solverは`mate_solver::nnue::NnueScorer::from_model`で読み込む。学習用の依存関係は独立した`nnue_training` crateに分離してあり、solverのランタイム依存関係には含まれない。

`learn`は学習例に記録された局面のposition featuresだけを入力にして、ラベル付き例を決定的な順序でオンラインSGDする。デフォルトのepoch数は10で、`--epochs=N`で変更できる。epochごとにweighted binary cross-entropyの平均lossを標準出力へ出す。出力にsigmoidを置いた勾配を、ReLUを挟む全ての層（入力重み、2つのhidden層、出力層）へ逆伝播し、最後に固定小数点の整数重みへ丸める。正例が少ないデータでも勾配が消えないよう、正例には固定のクラス重みを掛ける。
`--epochs=0`は受け付けない。各epochのlossは`epoch 1/10: loss=...`の形式で標準出力へ出る。

`--mirror`はSFENを左右対称に変換した例を追加する。`--plies=N`は0手先からN手先まで各位置を生成し、各位置でevaluatorを再実行してmate/nomateのlabelを付け、元のID、変換方法、進めた手数を記録する。偶数のオフセットでは攻め方、奇数のオフセットでは玉方の局面を生成する。各局面の探索はデフォルトで60秒の生成期限を共有し、`--timeout-ms=<ms>`で変更できる。`--max-positions=<n>`を指定すると、探索で検査した局面数でも打ち切れる。いずれかの期限に達した場合は完了済みの部分結果を書き出す。

`score --output-file=<path>`は各学習例の`id`、局面、label、raw score、sigmoid probability、変換方法、ply offsetを指定したファイルへJSONLで出力する。未指定時は集計結果だけを標準出力に出す。

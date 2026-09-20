# nnue_training

`nnue_training`には、benchmarkの検索結果からmove-ordering用の学習例を生成する`generate`、学習例をランタイム形式へ変換する`export`、モデルのスコアを確認する`score`の3つのバイナリがある。`generate`は候補手ごとの子局面を指定したevaluatorで再評価し、攻め方にとって詰みと判定された手を`1`、それ以外を`0`とする。生成物はrepositoryにcommitしない。

ファイル形式の詳細は[`format.md`](format.md)にまとめてある。

## 使い方

```sh
cargo run --release -p benchmark_harness -- run --strict --move-ordering=nnue-fixture --revision=fixture benchmark/issue16-ordering.jsonl > /tmp/benchmark-results.jsonl
cargo run --release -p nnue_training --bin generate -- --positions benchmark/issue16-ordering.jsonl --results /tmp/benchmark-results.jsonl --output /tmp/nnue-examples.jsonl --evaluator=eval --mirror --plies=2
cargo run --release -p nnue_training --bin export -- --examples /tmp/nnue-examples.jsonl --output /tmp/nnue-model.nnue
cargo run --release -p nnue_training --bin score -- --model /tmp/nnue-model.nnue --examples /tmp/nnue-examples.jsonl
cargo run --release -p nnue_training --bin score -- --model /tmp/nnue-model.nnue --examples /tmp/nnue-examples.jsonl --output-file /tmp/nnue-scores.jsonl
```

exportした`NNUE-FIXTURE 1`テキストモデルは`mate_solver::nnue::NnueScorer::from_model`で読み込む。学習用の依存関係は独立した`nnue_training` crateに分離してあり、solverのランタイム依存関係には含まれない。

`--mirror`はSFENと指し手を変換した左右対称の例を追加する。`--plies=N`は0手先からN手先まで各位置を生成し、各位置でevaluatorを再実行して新しいlabelを付け、元のID、変換方法、進めた手数を記録する。偶数のオフセットでは攻め方、奇数のオフセットでは玉方の候補手を生成する。候補手ごとの探索はデフォルトで60秒の生成期限を共有し、`--timeout-ms=<ms>`で変更できる。期限に達した場合は完了済みの部分結果を書き出す。

`score --output-file=<path>`は各学習例の`id`、局面、指し手、label、score、変換方法、ply offsetを指定したファイルへJSONLで出力する。未指定時は集計結果だけを標準出力に出す。

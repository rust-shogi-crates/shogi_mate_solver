# mate_solver

mate_solver ==> 詰将棋を解く (SFEN 文字列を標準入力から 1 行で与える)
-  `--verbose` ==> 詳細な情報 (探索ノード数・実行時間など) を出力
-  `--output=json` ==> 今風に JSON で出力
-  `--move-format=traditional|official|kif|usi|csa` ==> 手の表示方法を変える
-  `--move-ordering=current|fixture|nnue-fixture` ==> 手の順序付け方式を選ぶ

実行例
```
cargo run --bin mate_solver -- --verbose <<<"5kgnl/9/4+B1pp1/8p/9/9/9/9/9 b 2S2rb3g2s3n3l15p 1"
```

# to_sfen
to_sfen problem.kif ==> KIF ファイルを sfen に出力
- 与えられたファイルが初期局面から始まっている場合は最終局面を、そうでなければ開始局面を返す。

to_sfen URL ==> URL に書かれている将棋の盤面に対して同じことを行う

実行例
```
cargo run --bin to_sfen https://www.shogi.or.jp/tsume_shogi/mynavi/201812145_1.html
```

# benchmark_harness

benchmark_harness は JSONL の局面リストを読み、df-pn と eval の結果を JSONL で標準出力に出す。

入力は 1 行 1 オブジェクト:
```
{"id":"mate5","sfen":"3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1","expected":"mate"}
```

- `id` は省略可。省略時は `<path>:<line>` を使う。
- `expected` は `mate` または `nomate`。`--strict` では必須。
- `expected_plies` は詰み手数を確認したい場合だけ指定する。

実行例:
```
cargo run --release -p benchmark_harness -- run --strict --revision=current benchmark/issue13-ci.jsonl
cargo run --release -p benchmark_harness -- run --strict --revision=current benchmark/issue16-ordering.jsonl
```

`run` の結果には、手の順序付けを評価するためのフィールドを含む:

- `root_chosen_move_rank`: ルートで実際に選ばれた、または df-pn で証明された手が、現在の順序付けで何番目だったか。0 なら最初の候補。
- `root_first_candidate_chosen`: `root_chosen_move_rank == 0` の簡易指標。良い順序付けではこの割合が上がる。
- `root_candidate_moves`: ルートで生成された候補手数。これは文脈情報であり、単独では順序付け品質を表さない。
- `eval_positions_inspected`: eval 探索だけで調べた局面数。
- `df_pn_positions_inspected`: eval 探索中の df-pn 呼び出しで調べた局面数。

`benchmark/issue16-ordering.jsonl` は、順序付け評価用の小さな代表 fixture。局面は既存の solver tests と `benchmark/issue13-ci.jsonl` から採った。`ordering-mate5-multi-candidate` と `ordering-mate9-longer` は repo 内 test comment が `shogi-mate-problems` の `2022-05-18` を参照している。`ordering-df-pn-mate` は `2022-05-19/dpm.psn` を参照している。`ordering-mate3-short` と `ordering-nomate-rook-hand-empty-board` は repo 内の local/synthetic fixture。

`mate_solver::search` が返す branch 情報は、将来の NNUE で `Position -> mate probability` を学習するための mate/no-mate ラベル候補として使える。ただし、この benchmark harness は学習データ exporter ではなく、PR2 では root の順序付け品質と探索量を測る。

比較例:
```
cargo run -p benchmark_harness -- compare --base benchmark-base.jsonl --current benchmark-current.jsonl --html benchmark-report.html
```

`compare` の `ratio` は `current_elapsed_ms / base_elapsed_ms`。1.0 未満なら current の方が速い。比較結果には `mean`, `median`, `stddev`, `p90`, `p95`, `p99` を含む。df-pn の `proof_number` と `disproof_number` は実装中の phi/delta に対応する。順序付け品質は `root_chosen_move_rank`、`root_first_candidate_chosen`、および `root_chosen_move_rank / root_candidate_moves` の平均で見る。`root_candidate_moves` 自体は探索局面の文脈情報なので、base/current の良し悪し比較には使わない。base/current の片方にしかない result は `type: "warning"` として出力し、共通 result だけを比較する。ただし current 側の `correct: false` は、base に対応 result がなくても失敗として扱う。

`--html` を指定すると、同じ統計を人間が読みやすい HTML レポートにも出力する。

エラーも同じ JSONL ストリームに出力される。CI では標準出力を `benchmark-base.jsonl`, `benchmark-current.jsonl`, `benchmark-comparison.jsonl` にリダイレクトし、`benchmark-report.html` と一緒に artifacts として保存する。

# nnue_training

`nnue_training` には、benchmark の検索結果から move-ordering 用の学習例を生成する `generate`、学習例をランタイム形式へ変換する `export`、モデルのスコアを確認する `score` の3つのバイナリがある。`generate` は候補手ごとの子局面を指定した evaluator で再評価し、攻め方にとって詰みと判定された手を `1`、それ以外を `0` とする。生成物は repository に commit しない。

```
cargo run --release -p benchmark_harness -- run --strict --move-ordering=nnue-fixture --revision=fixture benchmark/issue16-ordering.jsonl > /tmp/benchmark-results.jsonl
cargo run --release -p nnue_training --bin generate -- --positions benchmark/issue16-ordering.jsonl --results /tmp/benchmark-results.jsonl --output /tmp/nnue-examples.jsonl --evaluator=eval --mirror --plies=2
cargo run --release -p nnue_training --bin export -- --examples /tmp/nnue-examples.jsonl --output /tmp/nnue-model.nnue
cargo run --release -p nnue_training --bin score -- --model /tmp/nnue-model.nnue --examples /tmp/nnue-examples.jsonl
```

export した `NNUE-FIXTURE 1` テキストモデルは `mate_solver::nnue::NnueScorer::from_model` で読み込む。学習用の依存関係は独立した `nnue_training` crate に分離してあり、solver のランタイム依存関係には含まれない。

`--mirror` は SFEN と指し手を変換した左右対称の例を追加する。`--plies=N` は 0 手先から N 手先まで各位置を生成し、各位置で evaluator を再実行して新しい label を付け、元の ID、変換方法、進めた手数を記録する。偶数のオフセットでは攻め方、奇数のオフセットでは玉方の候補手を生成する。候補手ごとの探索はデフォルトで 60 秒の生成期限を共有し、`--timeout-ms=<ms>` で変更できる。期限に達した場合は完了済みの部分結果を書き出す。

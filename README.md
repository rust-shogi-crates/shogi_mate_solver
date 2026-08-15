# mate_solver

mate_solver ==> 詰将棋を解く (SFEN 文字列を標準入力から 1 行で与える)
-  `--verbose` ==> 詳細な情報 (探索ノード数・実行時間など) を出力
-  `--output=json` ==> 今風に JSON で出力
-  `--move-format=traditional|official|kif|usi|csa` ==> 手の表示方法を変える

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
cargo run --bin benchmark_harness -- run --strict --revision=current benchmark/issue13-smoke.jsonl
```

比較例:
```
cargo run --bin benchmark_harness -- compare --base benchmark-base.jsonl --current benchmark-current.jsonl --html benchmark-report.html
```

`compare` の `ratio` は `current_elapsed_ms / base_elapsed_ms`。1.0 未満なら current の方が速い。比較結果には `mean`, `median`, `stddev`, `p90`, `p95`, `p99` を含む。df-pn の `proof_number` と `disproof_number` は実装中の phi/delta に対応する。

`--html` を指定すると、同じ統計を人間が読みやすい HTML レポートにも出力する。

エラーも同じ JSONL ストリームに出力される。CI では標準出力を `benchmark-base.jsonl`, `benchmark-current.jsonl`, `benchmark-comparison.jsonl` にリダイレクトし、`benchmark-report.html` と一緒に artifacts として保存する。

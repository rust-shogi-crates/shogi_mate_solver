# nnue_training file formats

`nnue_training`はJSONLと`NNUE-FIXTURE 1`テキストモデルを読む。JSONLは1行1オブジェクトで、未知のフィールドは読み飛ばすが、ここに記載した必須フィールドは省略できない。

## Position JSONL

`generate --positions`が読む局面ファイル。

```json
{"id":"mate5","sfen":"3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1"}
```

- `id`: 文字列。省略時は`<path>:<line>`を使う。
- `sfen`: 必須のSFEN文字列。

## Search-result JSONL

`generate --results`が読むbenchmark結果ファイル。`record_type`が`"result"`、`evaluator`が指定した`--evaluator`と一致し、`id`が局面ファイルのIDと一致するレコードだけを使う。その他のフィールドは`generate`では参照しない。

```json
{"type":"result","id":"mate5","evaluator":"eval","resolution":"mate"}
```

- `type`: 必須。`"result"`だけが対象。
- `id`: 必須の文字列。
- `evaluator`: 必須の文字列。通常は`"eval"`または`"df_pn"`。

## Training-example JSONL

`learn`と`score`が読む学習例ファイル。すべてのフィールドが必須。

```json
{"id":"mate5::identity::ply0","source_id":"mate5","sfen":"3g1ks2/6g2/4S4/7B1/9/9/9/9/9 b G2rbg2s4n4l18p 1","role":"attacker","move_usi":"5c4b+","label":1,"transform":"identity","ply_offset":0}
```

- `id`: 学習例のID。
- `source_id`: 元の局面のID。
- `sfen`: 候補手を生成した局面のSFEN。
- `role`: `"attacker"`または`"defender"`。
- `move_usi`: 候補手のUSI表記。
- `label`: `0`または`1`。
- `transform`: `"identity"`、`"mirror"`、`"replay"`、`"replay+mirror"`のいずれか。
- `ply_offset`: 元局面から進めた手数。0は元局面。

## NNUE-FIXTURE model

`init`と`learn`が書き、`mate_solver::nnue::NnueScorer::from_model`と`score`が読むテキスト形式。`learn`は入力モデルを初期値として使い、学習後のモデルを出力する。

```text
NNUE-FIXTURE 1
hidden_units 2
hidden_bias 0 0
output_weights 2 1
output_bias 0
output_shift 7
feature 30300 64 32
```

必須フィールドは`hidden_units`、`hidden_bias`、`output_weights`、`output_bias`、`output_shift`。各フィールドは1回だけ指定し、`hidden_units`は`2`、hidden/output weightsは2個の符号付き整数、biasは1個の符号付き整数、`output_shift`は32未満の符号なし整数にする。`feature`は0個以上指定でき、各行は`feature <id> <hidden-weight-0> <hidden-weight-1>`の形式にする。

## Per-example score JSONL

`score --output-file=<path>`が書くファイル。入力した各学習例について1行出力する。

```json
{"id":"mate5::identity::ply0","source_id":"mate5","sfen":"...","role":"attacker","move_usi":"5c4b+","label":1,"score":123,"transform":"identity","ply_offset":0}
```

入力学習例のフィールドに、モデルが計算した整数`score`を追加した形式になる。

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

## NNUE-FIXTURE 1 model

`init`と`learn`が書き、`mate_solver::nnue::NnueScorer::from_model`と`score`が読むテキスト形式。入力512ユニット、hidden 32ユニット、hidden 32ユニット、出力1ユニットの固定構成を使う。`input_weights`はFeature IDごとの512個の入力重みで、同じFeature IDは1回だけ指定する。`learn`はこの全結合層を逆伝播で更新する。

```text
NNUE-FIXTURE 1
hidden_units 512 32 32
output_shift 0
logit_scale 127
input_weights 30300 <512 values>
layer1_weights <32 * 512 values>
layer1_bias <32 values>
layer2_weights <32 * 32 values>
layer2_bias <32 values>
output_weights <32 values>
output_bias 0
```

`layer1_weights`と`layer2_weights`は行優先で並べる。各hidden層の出力にはReLUを適用し、最後に`output_shift`ビット右シフトする。`output_shift`は32未満、`logit_scale`は正の整数にする。確率を求めるときは`sigmoid(score / logit_scale)`を使う。

## Per-example score JSONL

`score --output-file=<path>`が書くファイル。入力した各学習例について1行出力する。

```json
{"id":"mate5::identity::ply0","source_id":"mate5","sfen":"...","role":"attacker","move_usi":"5c4b+","label":1,"score":123,"probability":773000,"transform":"identity","ply_offset":0}
```

入力学習例のフィールドに、モデルが計算した整数`score`と`probability`を追加した形式になる。

`score`は`NnueScorer::score`が返すrawな符号付き32ビット整数で、確率ではない。モデルの重みと`output_shift`によって値の大きさが決まり、固定された正規化範囲はない。`probability`は`sigmoid(score / logit_scale) * 1_000_000`を丸めた値で、範囲は`0..=1_000_000`。`logit_scale`の初期値は127。学習時の目的関数に対応する確率的な指標だが、データ量やクラス重みの影響を受けるため、校正済みの確率ではない。hidden層のReLU後に出力を右シフトするため、負のhidden値だけからなる候補の`score`は`0`になる。`0`は有効なスコアであり、未評価やエラーを意味しない。

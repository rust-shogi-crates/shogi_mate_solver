# YaneuraOu の詰将棋データから NNUE を作る

この文書では、やねうら王が公開している詰将棋データを取得し、`nnue_training`で学習例を作り、学習済みNNUEをsolverで評価するまでの手順をまとめる。

生成物は大きくなりやすいので、rawデータ、学習例、スコア、作業用モデルは`nnue_training/work/`に置く。このディレクトリの生成物はrepositoryにcommitしない。repositoryへ入れるモデルを作る場合だけ、最後に明示的に`nnue_training/models/`へコピーする。

## 1. 作業ディレクトリを作る

```sh
# YaneuraOuのrawデータと生成物を置くディレクトリを作る
mkdir -p nnue_training/work/yaneuraou
```

## 2. YaneuraOuのデータを取得する

データの配布元は[やねうら王の「クリスマスプレゼント」](https://yaneuraou.yaneu.com/2020/12/25/christmas-present/)。配布ページからアーカイブを取得し、次の5ファイルを`nnue_training/work/yaneuraou/`に展開する。

```text
mate3.sfen
mate5.sfen
mate7.sfen
mate9.sfen
mate11.sfen
```

配置を確認する。

```sh
# 5つのSFENファイルがあることと、おおよその問題数を確認する
ls -lh nnue_training/work/yaneuraou/mate3.sfen \
  nnue_training/work/yaneuraou/mate5.sfen \
  nnue_training/work/yaneuraou/mate7.sfen \
  nnue_training/work/yaneuraou/mate9.sfen \
  nnue_training/work/yaneuraou/mate11.sfen
wc -l nnue_training/work/yaneuraou/mate3.sfen \
  nnue_training/work/yaneuraou/mate5.sfen \
  nnue_training/work/yaneuraou/mate7.sfen \
  nnue_training/work/yaneuraou/mate9.sfen \
  nnue_training/work/yaneuraou/mate11.sfen
```

## 3. 学習例を生成する

まず短い試行で、入力の読み込みと出力形式を確認する。

```sh
# 各ファイルから少数だけ取り、左右反転と0〜1手先を含む例を60秒で作る
cargo run --release -p nnue_training --bin prepare_yaneuraou -- \
  --input-dir nnue_training/work/yaneuraou \
  --output nnue_training/work/yaneuraou-examples-smoke.jsonl \
  --samples-per-file 100 \
  --negatives-per-position 1 \
  --plies 1 \
  --root-role defender \
  --mirror \
  --seed 20261010 \
  --timeout-ms 60000
```

本番用の例を作る。`--plies=N`はオフセット0からNまでを生成する。YaneuraOuのraw局面は玉方番なので、`--root-role defender`を使う。この場合、偶数オフセットは玉方、奇数オフセットは攻め方の局面になる。元局面のlabelは配布元の詰み保証から`1`、派生局面のlabelはsolverの`Value::is_mate()`で付ける。

```sh
# 各ファイルから最大100万局面を取り、0〜4手先まで含む学習例を作る
# --timeout-msはこの実行全体で共有される。必要な時間まで増やす
cargo run --release -p nnue_training --bin prepare_yaneuraou -- \
  --input-dir nnue_training/work/yaneuraou \
  --output nnue_training/work/yaneuraou-examples.jsonl \
  --samples-per-file 1000000 \
  --negatives-per-position 1 \
  --plies 4 \
  --root-role defender \
  --mirror \
  --seed 20261010 \
  --timeout-ms 3600000
```

`--timeout-ms`または`--max-positions`に達すると、完了済みの部分結果を書き出して終了する。データ量を増やす場合は、同じseedで制限を増やすより、出力を分けて複数回生成し、重複を確認してから結合する。生成例の内容は次で確認できる。

出力は生成中も一定件数ごとにflushされる。そのため`Ctrl-C`などで手動停止した場合も、最後にflushされた位置までのJSONLが残る。ただし、停止直前の未flush分は失われる。

```sh
# 生成された例の件数と、offsetごとの件数を確認する
wc -l nnue_training/work/yaneuraou-examples.jsonl
jq -s 'group_by(.ply_offset) | map({ply_offset: .[0].ply_offset, count: length})' \
  nnue_training/work/yaneuraou-examples.jsonl
```

## 4. source_id単位でtrain/testに分ける

同じ元局面の左右反転例や派生局面をtrainとtestの両方に入れると、test lossが実際より良く見える。`source_id`単位で分割し、同じ元局面から作られた例が両方へ入らないようにする。

```sh
# 元局面単位で決定的に80%をtrain、20%をtestへ分ける
python3 - <<'PY'
import json
import random
from collections import defaultdict

input_path = "nnue_training/work/yaneuraou-examples.jsonl"
train_path = "nnue_training/work/yaneuraou-train.jsonl"
test_path = "nnue_training/work/yaneuraou-test.jsonl"

groups = defaultdict(list)
with open(input_path, encoding="utf-8") as source:
    for line in source:
        if line.strip():
            example = json.loads(line)
            groups[example["source_id"]].append(line)

source_ids = list(groups)
random.Random(20261010).shuffle(source_ids)
split = max(1, int(len(source_ids) * 0.8))
train_ids = set(source_ids[:split])

with open(train_path, "w", encoding="utf-8") as train, open(test_path, "w", encoding="utf-8") as test:
    for source_id, lines in groups.items():
        destination = train if source_id in train_ids else test
        destination.writelines(lines)

print(f"sources: train={len(train_ids)} test={len(source_ids) - len(train_ids)}")
PY

# train/testの件数を確認する
wc -l nnue_training/work/yaneuraou-train.jsonl nnue_training/work/yaneuraou-test.jsonl
```

## 5. 学習開始用モデルを作る

`learn`は初期モデルを入力として受け取り、学習後のモデルを別ファイルへ書く。初期モデルがなければ`init`で作る。

```sh
# 空の512 -> 32 -> 32 -> 1モデルを作る
cargo run --release -p nnue_training --bin init -- \
  --output nnue_training/work/nnue-initial.nnue
```

## 6. 学習する

`learn`はposition featuresだけを入力にする。各例についてsigmoidの出力とlabelのweighted binary cross-entropyを計算し、その勾配を全層へ逆伝播する。`--epochs`は追加で回すepoch数で、各epoch終了時にtrain lossを出す。`--test-examples`を指定すると、重みを更新せずtest lossも出す。

```sh
# まず50 epoch学習し、epochごとのtrain/test lossを表示する
cargo run --release -p nnue_training --bin learn -- \
  --model nnue_training/work/nnue-initial.nnue \
  --examples nnue_training/work/yaneuraou-train.jsonl \
  --test-examples nnue_training/work/yaneuraou-test.jsonl \
  --epochs 50 \
  --output nnue_training/work/yaneuraou-model.nnue
```

出力は次の形式になる。

```text
epoch 1/50: train_loss=... test_loss=...
epoch 2/50: train_loss=... test_loss=...
```

test lossが下がらず上がり続ける場合は過学習またはtrain/testの分布差が疑われる。現在の`learn`はbest checkpointの自動保存やearly stoppingをしないため、lossを見てepoch数を決め、必要なら初期モデルからepoch数を変えて再実行する。さらに学習する場合は、直前のモデルを入力にして出力を別名にする。

```sh
# 既存モデルから追加で20 epoch学習する
cargo run --release -p nnue_training --bin learn -- \
  --model nnue_training/work/yaneuraou-model.nnue \
  --examples nnue_training/work/yaneuraou-train.jsonl \
  --test-examples nnue_training/work/yaneuraou-test.jsonl \
  --epochs 20 \
  --output nnue_training/work/yaneuraou-model-70.nnue
```

## 7. 各例のスコアを確認する

`score`の標準出力は集計用。`--output-file`を付けると、各例のraw score、probability、labelなどをJSONLへ保存する。probabilityは`sigmoid(score)`を`0..=1_000_000`へ変換した値。

```sh
# train例を集計する
cargo run --release -p nnue_training --bin score -- \
  --model nnue_training/work/yaneuraou-model.nnue \
  --examples nnue_training/work/yaneuraou-train.jsonl

# test例を1例ずつJSONLへ出力する
cargo run --release -p nnue_training --bin score -- \
  --model nnue_training/work/yaneuraou-model.nnue \
  --examples nnue_training/work/yaneuraou-test.jsonl \
  --output-file nnue_training/work/yaneuraou-test-scores.jsonl
```

## 8. solverで使う

まず単一局面でモデルを指定して動かす。

```sh
# 学習済みモデルを使ってsolverを実行する
cargo run --release -p shogi_mate_solver --bin mate_solver -- \
  --move-ordering nnue-model \
  --nnue-model nnue_training/work/yaneuraou-model.nnue \
  --sfen '5gsk1/9/6B1+P/9/7Np/9/9/9/9 b BP2r3g3s3n4l15p 1'
```

ベンチマークでは、baseとcurrentを同じ問題集合で実行し、HTML比較を作る。

```sh
# 学習済みモデルでcurrent側のベンチマークを実行する
cargo run --release -p benchmark_harness -- run --strict \
  --move-ordering nnue-model \
  --nnue-model nnue_training/work/yaneuraou-model.nnue \
  --revision current \
  benchmark/.hidden-mate-problems.jsonl \
  > nnue_training/work/yaneuraou-current.jsonl

# base側の結果とcurrentを比較し、HTMLレポートを作る
cargo run --release -p benchmark_harness -- compare \
  --base base.jsonl \
  --current nnue_training/work/yaneuraou-current.jsonl \
  --html nnue_training/work/yaneuraou-report.html \
  > nnue_training/work/yaneuraou-comparison.jsonl
```

実際のCIで使うモデルに昇格させる場合だけ、作業用ファイルを追跡対象の名前へコピーする。

```sh
# CIで使うモデルを更新する場合だけ、repository内のmodelsへコピーする
cp nnue_training/work/yaneuraou-model.nnue nnue_training/models/nnue-trained.nnue
```

モデルをcommitするか、サイズを確認してgzipで配布するかは別途判断する。rawデータ、JSONL、スコア、HTMLレポートは`nnue_training/work/`に残し、通常はcommitしない。

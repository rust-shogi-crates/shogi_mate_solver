PROJECT_ROOT=$(dirname $0)
NAME=$1
URL=https://www.shogi.or.jp/tsume_shogi/everyday/${NAME}.html
${PROJECT_ROOT}/../target/debug/to_sfen ${URL} > ${PROJECT_ROOT}/${NAME}.sfen

TARGET=$1
# ENGINE=--engine-path=../YaneuraOu/source/YaneuraOu-by-gcc
ENGINE=
cargo build
target/debug/to_sfen ${TARGET} | target/debug/mate_solver --verbose --move-format=traditional ${ENGINE}

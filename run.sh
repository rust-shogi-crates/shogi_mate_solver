TARGET=$1
cargo build
target/debug/to_sfen ${TARGET} | target/debug/mate_solver --verbose --move-format=traditional

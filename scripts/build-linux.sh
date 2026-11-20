#!/bin/sh
set -eu
senti_script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
senti_repo_dir=$(dirname "$senti_script_dir")
cd "$senti_repo_dir"
cargo build -p senti-platform --release --locked
cmake -S platforms/linux/fcitx5 -B target/fcitx5 -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr \
    -DSENTI_HOST_EXECUTABLE="$senti_repo_dir/target/release/senti-host"
cmake --build target/fcitx5
printf '\nBuild complete. Test: ctest --test-dir target/fcitx5 --output-on-failure\n'
printf 'Install: sudo cmake --install target/fcitx5\n'

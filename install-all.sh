#!/usr/bin/env bash

cargo install --force --path ./src/clang
cargo install --force --path ./src/cmake
cargo install --force --path ./src/ld

cargo run --release --manifest-path ./src/sysroot/Cargo.toml -- --build=compiler-rt,libcxx,libdlmalloc,libc,libcxxabi

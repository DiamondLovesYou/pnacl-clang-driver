# WebAssembly C++/C compiler driver

Targets WebAssembly only. To use, you'll need Rust and Cargo installed, then run
`./install-all.sh` in the repo root. You can then use `wasm-clang`,
`wasm-clangxx`, and `wasm-ld` as your C, C++, and linker, respectively. If your
project uses CMake, `wasm-cmake` will set up CMake to target WebAssembly for
you.

#!/usr/bin/env bash

# libcxx has paths which cause path too long issues on windows.
# for dependees, these are unnecessary anyway. so just don't
# have them be submodules

cd system
git clone https://github.com/llvm-mirror/libcxx.git
cd libcxx && git checkout -B master 2495dabf93b1d8b9f1c3a18815d23da4b09a1d1f && cd ..
git clone https://github.com/llvm-mirror/compiler-rt.git
cd compiler-rt && git checkout -B master 4e8e8d6b18fccced6738aa85dfc28105c7add469 && cd ..
git clone https://github.com/llvm-mirror/libcxxabi.git
cd libcxxabi && git checkout -B master dd73082d02640d8677d585c8a48243dcdd93e195 && cd ..
git clone https://github.com/DiamondLovesYou/musl.git -b wasm-prototype-1
git clone https://github.com/madler/zlib.git
cd zlib && git checkout -B master cacf7f1d4e3d44d871b605da3b647f07d718623f && cd ..

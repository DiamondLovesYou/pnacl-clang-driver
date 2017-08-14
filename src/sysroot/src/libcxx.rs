
use super::{Invocation, link};
use util::{CommandQueue};

use tempdir::TempDir;
use clang_driver;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

const FILES: &'static [&'static str] = &[
  "algorithm.cpp",
  "any.cpp",
  "bind.cpp",
  "chrono.cpp",
  "condition_variable.cpp",
  "debug.cpp",
  "exception.cpp",
  "future.cpp",
  "hash.cpp",
  "ios.cpp",
  "iostream.cpp",
  "locale.cpp",
  "memory.cpp",
  "mutex.cpp",
  "new.cpp",
  "optional.cpp",
  "random.cpp",
  "regex.cpp",
  "shared_mutex.cpp",
  "stdexcept.cpp",
  "string.cpp",
  "strstream.cpp",
  "system_error.cpp",
  "thread.cpp",
  "typeinfo.cpp",
  "utility.cpp",
  "valarray.cpp",
];

pub fn build_cxx(invoc: &Invocation,
                 libcxxabi_include: &PathBuf,
                 file: &'static str,
                 queue: &mut &mut CommandQueue)
{
  let full_file = invoc.tc.emscripten
    .join("system/lib/libcxx")
    .join(file);

  let mut clang = clang_driver::Invocation::default();
  clang.driver_mode = clang_driver::DriverMode::CXX;
  let mut args = Vec::new();
  args.push("-c".to_string());
  args.push(format!("{}", full_file.display()));

  let out_file = format!("{}.obj", file);
  let out_file = Path::new(&out_file).to_path_buf();

  args.push("-DLIBCXX_BUILDING_LIBCXXABI=1".to_string());
  args.push("-Oz".to_string());
  args.push(format!("-I{}", libcxxabi_include.display()));
  args.push("-std=c++11".to_string());
  args.push("-D_LIBCPP_ABI_VERSION=2".to_string());

  let mut cmd = queue
    .enqueue_tool(Some("clang++"),
                  clang, args, false,
                  None::<Vec<::tempdir::TempDir>>)
    .expect("internal error: bad clang arguments");
  cmd.prev_outputs = false;
  cmd.output_override = true;
  cmd.intermediate_name = Some(out_file);
}

pub fn build(invoc: &Invocation,
             mut queue: &mut CommandQueue) -> Result<(), Box<Error>> {
  let libcxxabi_include = invoc.tc.emscripten
    .join("system/lib/libcxxabi/include")
    .to_path_buf();

  for file in FILES.iter() {
    build_cxx(invoc,
              &libcxxabi_include,
              file,
              &mut queue);
  }

  link(invoc, queue, "libcxx.so")
}

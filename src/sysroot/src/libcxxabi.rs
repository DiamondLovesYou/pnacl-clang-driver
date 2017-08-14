use super::{Invocation, link};
use util::{CommandQueue};

use tempdir::TempDir;
use clang_driver;

use std::error::Error;
use std::path::{Path, PathBuf};

// this is, like, almost the exact same as libc++.

const FILES: &'static [&'static str] = &[
  "abort_message.cpp",
  "cxa_aux_runtime.cpp",
  "cxa_default_handlers.cpp",
  "cxa_demangle.cpp",
  "cxa_exception_storage.cpp",
  "cxa_guard.cpp",
  "cxa_new_delete.cpp",
  "cxa_handlers.cpp",
  "cxa_virtual.cpp",
  "exception.cpp",
  "stdexcept.cpp",
  "typeinfo.cpp",
  "private_typeinfo.cpp",
];

pub fn build_cxx(invoc: &Invocation,
                 libcxxabi_include: &PathBuf,
                 file: &'static str,
                 queue: &mut &mut CommandQueue)
{
  let full_file = invoc.tc.emscripten
    .join("system/lib/libcxxabi/src")
    .join(file);

  let mut clang = clang_driver::Invocation::default();
  clang.driver_mode = clang_driver::DriverMode::CXX;
  let mut args = Vec::new();
  args.push("-c".to_string());
  args.push(format!("{}", full_file.display()));

  let out_file = format!("{}.obj", file);
  let out_file = Path::new(&out_file).to_path_buf();

  args.push("-Oz".to_string());
  args.push(format!("-I{}", libcxxabi_include.display()));
  args.push("-std=c++11".to_string());

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

  link(invoc, queue, "libcxxabi.so")
}

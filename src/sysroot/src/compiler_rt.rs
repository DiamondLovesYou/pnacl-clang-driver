use super::{Invocation, link};
use util::{CommandQueue, get_crate_root};

use clang_driver;

use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};

const BLACKLIST: &'static [&'static str] = &[
  "gcc_personality_v0.c",
  "apple_versioning.c",
];

pub fn build_cc(_invoc: &Invocation,
                full_file: &PathBuf,
                queue: &mut &mut CommandQueue<Invocation>)
  -> Result<(), Box<Error>>
{
  let file = Path::new(full_file.file_name().unwrap())
    .to_path_buf();

  let mut clang = clang_driver::Invocation::default();
  clang.driver_mode = clang_driver::DriverMode::CC;

  let mut args = Vec::new();
  args.push("-c".to_string());
  args.push(format!("{}", full_file.display()));

  let out_file = format!("{}.o", file.display());
  let out_file = Path::new(&out_file).to_path_buf();

  args.push("-Oz".to_string());

  let cmd = queue
    .enqueue_tool(Some("clang"),
                  clang, args, false,
                  None::<Vec<::tempdir::TempDir>>)?;
  cmd.prev_outputs = false;
  cmd.output_override = true;
  cmd.intermediate_name = Some(out_file);

  Ok(())
}

pub fn build(invoc: &Invocation,
             mut queue: &mut CommandQueue<Invocation>)
  -> Result<(), Box<Error>>
{
  use std::fs::read_dir;
  let compiler_rt_dir = get_crate_root()
    .join("system/compiler-rt");
  let builtins_dir = compiler_rt_dir
    .join("lib/builtins");

  let blacklist: HashSet<&'static str> = BLACKLIST
    .iter()
    .map(|&s| s )
    .collect();

  println!("{}", builtins_dir.display());

  let mut files = vec![];
  for entry in read_dir(builtins_dir).expect("read_dir") {
    let entry = entry.expect("entry");
    let ft = entry.file_type().expect("file_type");
    if ft.is_file() {
      let file = entry.path();
      if let Some(ext) = file.extension() {
        if ext != "c" { continue; }
        let name = file.file_name().unwrap();
        let name = name.to_str().unwrap();
        if !blacklist.contains(name) {
          files.push(file.to_path_buf());
        }
      }
    } else if ft.is_dir() {
      continue;
    } else {
      panic!("symlink unimplemented");
    }
  }

  for file in files.iter() {
    build_cc(invoc,
             file,
             &mut queue)?;
  }

  link(invoc, queue, &[], "libcompiler-rt.so")
}

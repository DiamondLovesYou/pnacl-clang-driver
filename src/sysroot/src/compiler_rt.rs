use super::{Invocation, link};
use util::{CommandQueue, get_crate_root};

use tempdir::TempDir;
use clang_driver;
use ld_driver;

use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

const FILES: &'static [&'static str] = &[
  "addtf3.c",
  "ashldi3.c",
  "ashlti3.c",
  "ashrdi3.c",
  "ashrti3.c",
  "atomic.c",
  "comparetf2.c",
  "divdi3.c",
  "divmoddi4.c",
  "divtf3.c",
  "divti3.c",
  "extenddftf2.c",
  "extendsftf2.c",
  "fixdfdi.c",
  "fixdfti.c",
  "fixsfti.c",
  "fixtfdi.c",
  "fixtfsi.c",
  "fixtfti.c",
  "fixunsdfdi.c",
  "fixunsdfti.c",
  "fixunssfti.c",
  "fixunstfdi.c",
  "fixunstfsi.c",
  "fixunstfti.c",
  "floatditf.c",
  "floatdidf.c",
  "floatsitf.c",
  "floattidf.c",
  "floattisf.c",
  "floatunditf.c",
  "floatunsitf.c",
  "floatuntidf.c",
  "floatuntisf.c",
  "lshrdi3.c",
  "lshrti3.c",
  "moddi3.c",
  "modti3.c",
  "muldi3.c",
  "muldc3.c",
  "mulsc3.c",
  "multc3.c",
  "multf3.c",
  "multi3.c",
  "subtf3.c",
  "trunctfdf2.c",
  "trunctfsf2.c",
  "udivdi3.c",
  "udivmoddi4.c",
  "udivmodti4.c",
  "udivti3.c",
  "umoddi3.c",
  "umodti3.c",
];

const BLACKLIST: &'static [&'static str] = &[
  "gcc_personality_v0.c",
  "apple_versioning.c",
];

pub fn build_cc(invoc: &Invocation,
                full_file: &PathBuf,
                queue: &mut &mut CommandQueue)
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

  let mut cmd = queue
    .enqueue_tool(Some("clang"),
                  clang, args, false,
                  None::<Vec<::tempdir::TempDir>>)?;
  cmd.prev_outputs = false;
  cmd.output_override = true;
  cmd.intermediate_name = Some(out_file);

  Ok(())
}

pub fn build(invoc: &Invocation,
             mut queue: &mut CommandQueue)
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

  link(invoc, queue, "libcompiler-rt.so")
}

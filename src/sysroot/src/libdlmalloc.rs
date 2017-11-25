use super::{Invocation};
use util::{CommandQueue};

use clang_driver;

use std::error::Error;
use std::path::{Path};

const FILES: &'static [&'static str] = &[
  "dlmalloc.c",
];

pub fn build_cc(invoc: &Invocation,
                file: &'static str,
                queue: &mut &mut CommandQueue<Invocation>)
{
  let full_file = invoc.tc.emscripten
    .join("system/lib")
    .join(file);

  let mut clang = clang_driver::Invocation::default();
  clang.driver_mode = clang_driver::DriverMode::CC;
  let mut args = Vec::new();
  args.push("-c".to_string());
  args.push(format!("{}", full_file.display()));

  let out_file = format!("{}.obj", file);
  let out_file = Path::new(&out_file).to_path_buf();

  args.push("-Oz".to_string());
  super::add_default_args(&mut args);

  let cmd = queue
    .enqueue_tool(Some("clang"),
                  clang, args, false,
                  None::<Vec<::tempdir::TempDir>>)
    .expect("internal error: bad clang arguments");
  cmd.prev_outputs = false;
  cmd.output_override = true;
  cmd.intermediate_name = Some(out_file);
}

pub fn build(invoc: &Invocation,
             mut queue: &mut CommandQueue<Invocation>)
  -> Result<(), Box<Error>>
{
  for file in FILES.iter() {
    build_cc(invoc,
              file,
              &mut queue);
  }
  Ok(())
}
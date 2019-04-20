use super::{Invocation, get_system_dir, };
use util::{CommandQueue};

use clang_driver;

use std::error::Error;
use std::path::{Path};

const FILES: &'static [&'static str] = &[
  "dlmalloc.c",
];

impl Invocation {
  fn build_cc(&self,
              file: &'static str,
              queue: &mut &mut CommandQueue<Invocation>)
    -> Result<(), Box<Error>>
  {
    let full_file = get_system_dir().join(file);

    let mut clang = clang_driver::Invocation::default();
    clang.driver_mode = clang_driver::DriverMode::CC;

    let mut args = Vec::new();
    args.push(format!("-isystem{}", self.musl_include_dir().display()));
    args.push("-c".to_string());
    args.push(format!("{}", full_file.display()));

    let out_file = self.dlmalloc_obj_output()?;

    args.push("-O3".to_string());
    super::add_default_args(&mut args);

    let cmd = queue
      .enqueue_tool(Some("clang"),
                    clang, args, false,
                    None::<Vec<::tempdir::TempDir>>)
      .expect("internal error: bad clang arguments");
    cmd.prev_outputs = false;
    cmd.output_override = true;
    cmd.intermediate_name = Some(out_file);

    Ok(())
  }

  pub fn build_dlmalloc(&self, mut queue: &mut CommandQueue<Invocation>)
    -> Result<(), Box<Error>>
  {
    assert_eq!(FILES.len(), 1, "XXX dlmalloc will probably be a single file forever tho");
    for file in FILES.iter() {
      self.build_cc(file, &mut queue)?;
    }
    Ok(())
  }
}

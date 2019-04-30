use super::{Invocation, link};
use util::{CommandQueue, get_crate_root, CreateIfNotExists, Tool, };

use clang_driver;

use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};

const BLACKLIST: &'static [&'static str] = &[
  "gcc_personality_v0.c",
  "apple_versioning.c",
  "emutls.c",
];

impl Invocation {
  pub fn compiler_rt_src(&self) -> PathBuf {
    self.srcs.join(self.compiler_rt_repo.name.as_ref())
  }
  pub fn compiler_rt_build(&self) -> PathBuf {
    self.srcs.join("compiler-rt-build")
  }
  pub fn checkout_compiler_rt(&mut self) -> Result<(), Box<Error>> {
    if self.compiler_rt_checkout { return Ok(()); }
    self.compiler_rt_checkout = true;

    self.compiler_rt_repo.checkout_thin(self.compiler_rt_src())
  }
}

pub fn build_cc(invoc: &Invocation,
                compiler_rt_prefix: &PathBuf,
                build_out: &PathBuf,
                full_file: &PathBuf,
                queue: &mut &mut CommandQueue<Invocation>)
  -> Result<(), Box<Error>>
{
  let file = Path::new(full_file.file_name().unwrap());

  let mut clang = clang_driver::Invocation::with_toolchain(invoc);
  clang.driver_mode = clang_driver::DriverMode::CC;
  clang.emit_wast = invoc.emit_wast;


  let mut args = Vec::new();
  args.push("-c".to_string());
  args.push(format!("{}", full_file.display()));

  // compiler-rt needs some libc headers:
  let include = invoc.get_musl_root().join("include");
  let arch_include = invoc.get_musl_root().join("arch/wasm32");
  let generic_include = invoc.get_musl_root().join("arch/generic");
  let config_include = invoc.get_musl_root().join("obj/include");
  clang.add_system_include_dir(config_include);
  clang.add_system_include_dir(generic_include);
  clang.add_system_include_dir(arch_include);
  clang.add_system_include_dir(include);


  let source_path = full_file.strip_prefix(compiler_rt_prefix)
    .expect("source not in src dir?");
  let output = build_out.join(source_path)
    .with_extension("o");
  output.parent().unwrap()
    .create_if_not_exists()?;
  clang.override_output(output);

  let out_file = format!("{}.o", file.display());
  let out_file = Path::new(&out_file);

  args.push("-Oz".to_string());
  super::add_default_args(&mut args);

  let cmd = queue
    .enqueue_tool(Some("clang"),
                  clang, args, false,
                  None::<Vec<::tempdir::TempDir> >)?;
  cmd.prev_outputs = false;
  cmd.output_override = false;
  cmd.intermediate_name = Some(out_file.into());

  Ok(())
}

pub fn build(invoc: &mut Invocation,
             mut queue: &mut CommandQueue<Invocation>)
  -> Result<(), Box<Error>>
{
  use std::fs::read_dir;
  let compiler_rt_dir = invoc.compiler_rt_src();
  let build_dir = invoc.compiler_rt_build();
  let builtins_dir = compiler_rt_dir
    .join("lib/builtins");

  let blacklist: HashSet<&'static str> = BLACKLIST
    .iter()
    .map(|&s| s )
    .collect();

  invoc.configure_musl(queue)?;

  let mut files = vec![];
  for entry in read_dir(builtins_dir).expect("read_dir") {
    let entry = entry.expect("entry");
    let ft = entry.file_type().expect("file_type");
    if !ft.is_dir() {
      let file = entry.path();
      if let Some(ext) = file.extension() {
        if ext != "c" { continue; }
        let name = file.file_name().unwrap();
        let name = name.to_str().unwrap();
        if !blacklist.contains(name) {
          files.push(file.to_path_buf());
        }
      }
    }
  }

  for file in files.iter() {
    build_cc(invoc,
             &compiler_rt_dir,
             &build_dir,
             file,
             &mut queue)?;
  }

  link(invoc, queue, &[], "libcompiler-rt")?;

  Ok(())
}

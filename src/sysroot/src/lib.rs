
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use util::{ToolArgs, Tool, ToolInvocation, CommandQueue,
           CreateIfNotExists};
use util::toolchain::{WasmToolchain, WasmToolchainTool, };
use std::fs::remove_file;

pub mod libc;
pub mod libcxx;
pub mod libcxxabi;
pub mod libunwind;
pub mod libdlmalloc;
pub mod compiler_rt;
pub mod compat;
pub mod zlib;

extern crate regex;
#[macro_use]
extern crate util;
#[macro_use]
extern crate lazy_static;
extern crate tempdir;
extern crate dirs;

extern crate clang_driver;
extern crate cmake_driver;
extern crate ld_driver;

const CRATE_ROOT: &'static str = env!("CARGO_MANIFEST_DIR");
fn get_cmake_modules_dir() -> PathBuf {
  let pwd = Path::new(CRATE_ROOT);
  pwd.join("../../cmake/Modules").to_path_buf()
}
fn get_system_dir() -> PathBuf {
  let pwd = Path::new(CRATE_ROOT);
  pwd.join("../../system")
}

#[derive(Debug)]
pub struct Invocation {
  pub tc: WasmToolchain,
  libraries: Vec<SystemLibrary>,
  libraries_set: HashSet<SystemLibrary>,

  start_dir: PathBuf,

  llvm_src: Option<PathBuf>,

  clobber_libunwind_build: bool,
  clobber_libcxxabi_build: bool,
  clobber_libcxx_build: bool,
  clobber_libc_build: bool,
  clobber_compiler_rt_build: bool,
  clobber_zlib_build: bool,

  pub emit_wast: bool,
  pub emit_wasm: bool,
}
impl Invocation {
  fn add_library(&mut self, lib: SystemLibrary) {
    if self.libraries_set.insert(lib.clone()) {
      self.libraries.push(lib);
    }
  }
  pub fn llvm_src(&self) -> &PathBuf {
    self.llvm_src.as_ref()
      .expect("Need `--llvm-src`")
  }
  pub fn c_cxx_linker_args(&self) -> Vec<Cow<'static, str>> {
    let mut v = vec![
      Cow::Borrowed("--growable-table-import"),
    ];

    if self.emit_wast {
      v.push(Cow::Borrowed("--emit-wast"));
    }

    v
  }
  pub fn c_cxx_linker_cflags(&self) -> String {
    let args = self.c_cxx_linker_args();
    if args.len() == 0 { return Default::default(); }

    let mut out = "-Wl,".to_string();
    for (idx, arg) in args.into_iter().enumerate() {
      if idx != 0 {
        out.push_str(",");
      }
      out.push_str(arg.as_ref());
    }

    out
  }

  pub fn cxx(&self) -> PathBuf {
    dirs::home_dir()
      .expect("need a $HOME")
      .join(".cargo/bin")
      .join("wasm-clangxx")
  }
  pub fn cc(&self) -> PathBuf {
    dirs::home_dir()
      .expect("need a $HOME")
      .join(".cargo/bin")
      .join("wasm-clang")
  }
}
impl Default for Invocation {
  fn default() -> Invocation {
    Invocation {
      libraries: vec!(),
      libraries_set: Default::default(),

      start_dir: ::std::env::current_dir().unwrap(),

      llvm_src: None,

      clobber_libunwind_build: false,
      clobber_libcxxabi_build: false,
      clobber_libcxx_build: false,
      clobber_libc_build: false,
      clobber_compiler_rt_build: false,
      clobber_zlib_build: false,

      tc: WasmToolchain::default(),

      emit_wast: false,
      emit_wasm: true,
    }
  }
}

#[derive(Debug, PartialEq, Ord, Eq, Hash, Clone, Copy)]
pub enum SystemLibrary {
  Compat,
  LibC,
  LibCxx,
  LibCxxAbi,
  LibUnwind,
  CompilerRt,
  DlMalloc,
  Zlib,
}
impl SystemLibrary { }

impl FromStr for SystemLibrary {
  type Err = Box<Error>;
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "libc" => Ok(SystemLibrary::LibC),
      "libcxx" => Ok(SystemLibrary::LibCxx),
      "libcxxabi" => Ok(SystemLibrary::LibCxxAbi),
      "libunwind" => Ok(SystemLibrary::LibUnwind),
      "compiler-rt" => Ok(SystemLibrary::CompilerRt),
      "compat" => Ok(SystemLibrary::Compat),
      "dlmalloc" => Ok(SystemLibrary::DlMalloc),
      "zlib" => Ok(SystemLibrary::Zlib),
      _ => {
        Err(format!("unknown system library: {}", s))?
      },
    }
  }
}
impl PartialOrd for SystemLibrary {
  fn partial_cmp(&self, other: &SystemLibrary) -> Option<Ordering> {
    let o = match (self, other) {
      (&SystemLibrary::CompilerRt, &SystemLibrary::CompilerRt) |
      (&SystemLibrary::LibC, &SystemLibrary::LibC) |
      (&SystemLibrary::LibCxx, &SystemLibrary::LibCxx) |
      (&SystemLibrary::LibCxxAbi, &SystemLibrary::LibCxxAbi) |
      (&SystemLibrary::LibUnwind, &SystemLibrary::LibUnwind) |
      (&SystemLibrary::Compat, &SystemLibrary::Compat) |
      (&SystemLibrary::DlMalloc, &SystemLibrary::DlMalloc) |
      (&SystemLibrary::Zlib, &SystemLibrary::Zlib) =>
        Ordering::Equal,

      (&SystemLibrary::Compat, _) => Ordering::Less,

      (&SystemLibrary::CompilerRt,
        &SystemLibrary::Compat) => Ordering::Greater,
      (&SystemLibrary::CompilerRt,
        _) => Ordering::Less,

      (&SystemLibrary::DlMalloc,
        &SystemLibrary::Compat) |
      (&SystemLibrary::DlMalloc,
        &SystemLibrary::CompilerRt) => Ordering::Greater,
      (&SystemLibrary::DlMalloc,
        _) => Ordering::Less,

      (&SystemLibrary::LibC,
        &SystemLibrary::CompilerRt) |
      (&SystemLibrary::LibC,
        &SystemLibrary::Compat) |
      (&SystemLibrary::LibC,
        &SystemLibrary::DlMalloc) => Ordering::Greater,
      (&SystemLibrary::LibC,
        _) => Ordering::Less,

      (&SystemLibrary::LibUnwind,
        &SystemLibrary::CompilerRt) |
      (&SystemLibrary::LibUnwind,
        &SystemLibrary::Compat) |
      (&SystemLibrary::LibUnwind,
        &SystemLibrary::LibC) => Ordering::Greater,
      (&SystemLibrary::LibUnwind, _) => Ordering::Less,

      (&SystemLibrary::LibCxxAbi,
        &SystemLibrary::LibUnwind) |
      (&SystemLibrary::LibCxxAbi,
        &SystemLibrary::LibC) |
      (&SystemLibrary::LibCxxAbi,
        &SystemLibrary::DlMalloc) |
      (&SystemLibrary::LibCxxAbi,
        &SystemLibrary::CompilerRt) |
      (&SystemLibrary::LibCxxAbi,
        &SystemLibrary::Compat) => Ordering::Greater,
      (&SystemLibrary::LibCxxAbi,
        _) => Ordering::Less,

      (&SystemLibrary::LibCxx,
        &SystemLibrary::Zlib) => Ordering::Less,
      (&SystemLibrary::LibCxx,
        _) => Ordering::Greater,

      (&SystemLibrary::Zlib,
        _) => Ordering::Greater,
    };

    Some(o)
  }
}

impl WasmToolchainTool for Invocation {
  fn wasm_toolchain(&self) -> &WasmToolchain { &self.tc }
  fn wasm_toolchain_mut(&mut self) -> &mut WasmToolchain { &mut self.tc }
}

impl Tool for Invocation {
  fn enqueue_commands(&mut self, queue: &mut CommandQueue<Invocation>)
    -> Result<(), Box<Error>>
  {
    let mut libraries = self.libraries.clone();
    libraries.sort();
    self.libraries.clear();
    self.libraries_set.clear();

    let mut dlmalloc_built = false;

    for syslib in libraries.into_iter() {
      match syslib {
        SystemLibrary::Compat => {
          self.build_compat(queue)?;
        },
        SystemLibrary::LibC => {
          self.build_musl(queue, &mut dlmalloc_built)?;
        },
        SystemLibrary::LibCxx => {
          self.build_libcxx(queue)?;
        },
        SystemLibrary::LibCxxAbi => {
          self.build_libcxxabi(queue)?;
        },
        SystemLibrary::LibUnwind => {
          self.build_libunwind(queue)?;
        },
        SystemLibrary::CompilerRt => {
          compiler_rt::build(self, queue)?;
        },
        SystemLibrary::DlMalloc => {
          self.build_dlmalloc(queue)?;
          dlmalloc_built = true;
        },
        SystemLibrary::Zlib => {
          self.build_zlib(queue)?;
        },
      }
    }

    Ok(())
  }

  fn get_name(&self) -> String {
    "wasm-sysroot".to_string()
  }

  fn add_tool_input(&mut self, _input: PathBuf)
    -> Result<(), Box<Error>>
  {
    unimplemented!()
  }

  fn get_output(&self) -> Option<&PathBuf> {
    Some(self.tc.sysroot_cache())
  }
  /// Unconditionally set the output file.
  fn override_output(&mut self, out: PathBuf) {
    self.tc.sysroot = out;
  }
}

impl ToolInvocation for Invocation {
  fn check_state(&mut self, iteration: usize, skip_inputs_check: bool)
    -> Result<(), Box<Error>>
  {
    match iteration {
      0 => { return Ok(()); }
      2 => {
        self.libraries.sort();
      },
      3 => {
        if self.libraries.binary_search(&SystemLibrary::LibCxx).is_ok() {
          if self.llvm_src.is_none() {
            return Err("Need --llvm-src".into());
          }
        }
      },
      _ => {},
    }

    Ok(())
  }

  /// Called until `None` is returned. Put args that override errors before
  /// the the args that can have those errors.
  fn args(&self, iteration: usize) -> Option<ToolArgs<Self>> {
    use util::ToolArg;
    use std::borrow::Cow;

    const C: &'static [ToolArg<Invocation>] = &[];
    let mut out = Cow::Borrowed(C);

    match iteration {
      0 => return tool_arguments!(Invocation => [
        EMIT_WAST_FLAG,
      ]),
      1 => {
        self.tc.args(&mut out);
      }
      2 => return tool_arguments!(Invocation => [
        LIBRARIES,
      ]),
      3 => return tool_arguments!(Invocation => [
        LLVM_SRC,
        CLOBBER_LIBUNWIND_BUILD,
        CLOBBER_LIBCXXABI_BUILD,
        CLOBBER_LIBCXX_BUILD,
        CLOBBER_LIBC_BUILD,
        CLOBBER_COMPILER_RT_BUILD,
        CLOBBER_ZLIB_BUILD,
        CLOBBER_ALL_BUILDS,
      ]),
      _ => return None,
    }

    Some(out)
  }
}

pub fn add_default_args(args: &mut Vec<String>) {
  args.push("-fno-slp-vectorize".to_string());
  args.push("-fno-vectorize".to_string());
  //args.push("-fPIC".to_string());
}

pub fn link(invoc: &Invocation,
            queue: &mut CommandQueue<Invocation>,
            s2wasm_libs: &[&str],
            out_name: &str)
  -> Result<PathBuf, Box<Error>>
{
  use std::process::Command;

  let reloc_out_name = format!("{}.so", out_name);
  let out = invoc.tc.sysroot_cache()
    .join("lib")
    .create_if_not_exists()?
    .join(&reloc_out_name);

  let out_f = out.clone();
  let out_name = out_name.to_string();
  let s2wasm_libs: Vec<String> = s2wasm_libs
    .iter()
    .map(|&s| s.to_owned() )
    .collect();
  let cmd = queue
    .enqueue_state_function(Some("link/archive"), move |invoc, state| {
      let out = out_f;
      let mut queue = CommandQueue::new(None);
      let prev_outputs = &state.prev_outputs[..];

      let mut args = Vec::new();
      args.push("-o".to_string());
      args.push(format!("{}", out.display()));

      let mut linker = ld_driver::Invocation::default();
      linker.emit_wast = invoc.emit_wast;
      linker.emit_wasm = invoc.emit_wasm;
      linker.optimize = Some(util::OptimizationGoal::Size);
      linker.relocatable = true;
      linker.import_memory = true;
      linker.import_table = true;
      linker.growable_table_import = true;
      let libname = out_name[..out_name.len() - 3].to_string();
      linker.s2wasm_libname = Some(libname);
      for input in prev_outputs.iter().cloned() {
        let input = ld_driver::Input::File(input);
        linker.add_input(input)?;
      }
      linker.add_search_path(invoc.tc.sysroot_cache().join("lib"));
      for lib in s2wasm_libs.iter() {
        linker.add_library(lib, false)?;
      }

      {
        let cmd = queue
          .enqueue_tool(Some("link"),
                        linker, args,
                        false,
                        None::<Vec<::tempdir::TempDir>>)?;

        cmd.prev_outputs = false;
        cmd.output_override = false;
      }

      let static_out_name = format!("{}.a", out_name);
      let out = invoc.tc.sysroot_cache()
        .join("lib")
        .create_if_not_exists()?
        .join(&static_out_name);

      remove_file(&out)?;

      let ar = invoc.tc.llvm_tool("llvm-ar");
      let mut ar = Command::new(ar);
      ar.arg("crs")
        .arg(out)
        .args(prev_outputs);

      {
        let cmd = queue
          .enqueue_simple_external(Some("archive"),
                                   ar, None);

        cmd.prev_outputs = false;
        cmd.output_override = false;
      }

      queue.run_all(*invoc)
    });
  cmd.prev_outputs = true;
  cmd.output_override = false;


  Ok(out)
}

argument!(impl LIBRARIES where { Some(r"^--build=(.*)$"), None } for Invocation {
    fn libraries_arg(this, _single, cap) {
      let args = cap.get(1)
        .unwrap().as_str();
      for arg in args.split(',') {
        let res: SystemLibrary = FromStr::from_str(arg)?;
        this.add_library(res);
      }
    }
});
tool_argument! {
  pub LLVM_SRC: Invocation = single_and_split_simple_path(path) "llvm-src" =>
  fn llvm_src_arg(this) {
    let path = this.start_dir.join(path);
    this.llvm_src = Some(path);
  }
}
tool_argument! {
  pub CLOBBER_LIBUNWIND_BUILD: Invocation = simple_no_flag(b) "clobber-libunwind-build" =>
  fn clobber_libunwind_build_arg(this) {
    this.clobber_libunwind_build = b;
  }
}
tool_argument! {
  pub CLOBBER_LIBCXXABI_BUILD: Invocation = simple_no_flag(b) "clobber-libcxxabi-build" =>
  fn clobber_libcxxabi_build_arg(this) {
    this.clobber_libcxxabi_build = b;
  }
}
tool_argument! {
  pub CLOBBER_LIBCXX_BUILD: Invocation = simple_no_flag(b) "clobber-libcxx-build" =>
  fn clobber_libcxx_build_arg(this) {
    this.clobber_libcxx_build = b;
  }
}
tool_argument! {
  pub CLOBBER_LIBC_BUILD: Invocation = simple_no_flag(b) "clobber-libc-build" =>
  fn clobber_libc_build_arg(this) {
    this.clobber_libc_build = b;
  }
}
tool_argument! {
  pub CLOBBER_COMPILER_RT_BUILD: Invocation = simple_no_flag(b) "clobber-compiler-rt-build" =>
  fn clobber_compiler_rt_build_arg(this) {
    this.clobber_compiler_rt_build = b;
  }
}
tool_argument! {
  pub CLOBBER_ZLIB_BUILD: Invocation = simple_no_flag(b) "clobber-zlib-build" =>
  fn clobber_zlib_build_arg(this) {
    this.clobber_zlib_build = b;
  }
}

tool_argument! {
  pub CLOBBER_ALL_BUILDS: Invocation = simple_no_flag(b) "clobber-all-builds" =>
  fn clobber_all_builds_arg(this) {
    this.clobber_libunwind_build = b;
    this.clobber_libcxxabi_build = b;
    this.clobber_libcxx_build = b;
    this.clobber_libc_build = b;
    this.clobber_compiler_rt_build = b;
    this.clobber_zlib_build = b;
  }
}

argument!(impl EMIT_WAST_FLAG where { Some(r"^--emit-wast$"), None } for Invocation {
    fn emit_wast_flag(this, _single, _cap) {
      this.emit_wast = true;
    }
});


use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use util::{ToolArgs, Tool, ToolInvocation, CommandQueue,
           CreateIfNotExists, ToolArgAccessor, regex, };
use util::toolchain::{WasmToolchain, WasmToolchainTool, };
use util::repo::Repo;
use std::fs::remove_file;
use std::alloc::System;
use std::collections::btree_set::BTreeSet;

pub mod libc;
pub mod libcxx;
pub mod libcxxabi;
pub mod libunwind;
pub mod libdlmalloc;
pub mod compiler_rt;
pub mod compat;
pub mod zlib;

#[macro_use]
extern crate wasm_driver_utils as util;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
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

#[derive(Debug, Clone)]
pub struct Invocation {
  pub tc: Option<WasmToolchain>,
  libraries: BTreeSet<SystemLibrary>,

  start_dir: PathBuf,

  musl_inited: bool,
  musl_configured: bool,

  pub llvm_src: Option<PathBuf>,
  pub srcs: PathBuf,

  pub clobber_libunwind_build: bool,
  pub clobber_libcxxabi_build: bool,
  pub clobber_libcxx_build: bool,
  pub clobber_libc_build: bool,
  pub clobber_compiler_rt_build: bool,
  pub clobber_zlib_build: bool,

  pub compiler_rt_repo: Repo,
  pub musl_repo: Repo,
  pub libcxx_repo: Repo,
  pub libcxxabi_repo: Repo,
  pub zlib_repo: Repo,
  pub libunwind_repo: Repo,

  compiler_rt_checkout: bool,
  musl_checkout: bool,
  libcxx_checkout: bool,
  libcxxabi_checkout: bool,
  zlib_checkout: bool,
  libunwind_checkout: bool,

  pub emit_wast: bool,
  pub emit_wasm: bool,
}
impl Invocation {
  pub fn add_all_libraries(&mut self) {
    self.add_library(SystemLibrary::Compat);
    self.add_library(SystemLibrary::DlMalloc);
    self.add_library(SystemLibrary::CompilerRt);
    self.add_library(SystemLibrary::LibC);
    self.add_library(SystemLibrary::LibCxx);
    self.add_library(SystemLibrary::LibCxxAbi);
    self.add_library(SystemLibrary::Zlib);
  }
  pub fn add_library(&mut self, lib: SystemLibrary) {
    self.libraries.insert(lib);
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
  pub fn tc(&self) -> &WasmToolchain {
    self.tc.as_ref()
      .expect("tc uninitialized")
  }
  pub fn init_wasm_tc(&mut self) {
    if self.tc.is_none() {
      self.tc = Some(Default::default());
    }
  }

  // compiler-rt and dlmalloc needs some libc installed headers:
  // but compiler-rt and dlmalloc must be built before musl.
  fn musl_includes(&self, clang: &mut clang_driver::Invocation) {
    let include = self.get_musl_root().join("include");
    let arch_include = self.get_musl_root().join("arch/wasm32");
    let generic_include = self.get_musl_root().join("arch/generic");
    let config_include = self.get_musl_root().join("obj/include");
    clang.add_system_include_dir(config_include);
    clang.add_system_include_dir(generic_include);
    clang.add_system_include_dir(arch_include);
    clang.add_system_include_dir(include);
  }
}
impl Default for Invocation {
  fn default() -> Invocation {
    Invocation {
      tc: None,

      libraries: Default::default(),

      start_dir: ::std::env::current_dir().unwrap(),
      musl_inited: false,
      musl_configured: false,

      llvm_src: None,
      srcs: get_system_dir(),

      clobber_libunwind_build: false,
      clobber_libcxxabi_build: false,
      clobber_libcxx_build: false,
      clobber_libc_build: false,
      clobber_compiler_rt_build: false,
      clobber_zlib_build: false,

      compiler_rt_repo: Repo::new_git_commit("compiler-rt", COMPILER_RT_REPO, "master",
                                             COMPILER_RT_COMMIT),
      musl_repo: Repo::new_git("musl", MUSL_REPO, MUSL_BRANCH),
      libcxx_repo: Repo::new_git_commit("libcxx", LIBCXX_REPO,
                                        "master", LIBCXX_COMMIT),
      libcxxabi_repo: Repo::new_git_commit("libcxxabi", LIBCXXABI_REPO,
                                           "master", LIBCXXABI_COMMIT),
      zlib_repo: Repo::new_git_commit("zlib", ZLIB_REPO, "master",
                                      ZLIB_COMMIT),
      libunwind_repo: Repo::new_git_commit("libunwind", LIBUNWIND_REPO, "master",
                                           LIBUNWIND_COMMIT),

      compiler_rt_checkout: false,
      musl_checkout: false,
      libcxx_checkout: false,
      libcxxabi_checkout: false,
      zlib_checkout: false,
      libunwind_checkout: false,

      emit_wast: false,
      emit_wasm: true,
    }
  }
}
const COMPILER_RT_REPO: &'static str = "https://github.com/llvm-mirror/compiler-rt.git";
const COMPILER_RT_COMMIT: &'static str = "4e8e8d6b18fccced6738aa85dfc28105c7add469";
const MUSL_REPO: &'static str = "https://github.com/DiamondLovesYou/musl.git";
const MUSL_BRANCH: &'static str = "wasm-prototype-1";
const LIBCXX_REPO: &'static str = "https://github.com/llvm-mirror/libcxx.git";
const LIBCXX_COMMIT: &'static str = "2495dabf93b1d8b9f1c3a18815d23da4b09a1d1f";
const LIBCXXABI_REPO: &'static str = "https://github.com/llvm-mirror/libcxxabi.git";
const LIBCXXABI_COMMIT: &'static str = "dd73082d02640d8677d585c8a48243dcdd93e195";
const ZLIB_REPO: &'static str = "https://github.com/madler/zlib.git";
const ZLIB_COMMIT: &'static str = "cacf7f1d4e3d44d871b605da3b647f07d718623f";
const LIBUNWIND_REPO: &'static str = "https://github.com/llvm-mirror/libunwind.git";
const LIBUNWIND_COMMIT: &'static str = "1e1c6b739595098ba5c466bfe9d58b993e646b48";

#[derive(Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Clone, Copy)]
pub enum SystemLibrary {
  Compat,
  CompilerRt,
  DlMalloc,
  LibC,
  LibUnwind,
  LibCxxAbi,
  LibCxx,
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

impl WasmToolchainTool for Invocation {
  fn wasm_toolchain(&self) -> &WasmToolchain {
    self.tc.as_ref()
      .expect("tc uninitialized")
  }
  fn wasm_toolchain_mut(&mut self) -> &mut WasmToolchain {
    self.tc.as_mut()
      .expect("tc uninitialized")
  }
}

impl Tool for Invocation {
  fn enqueue_commands(&mut self, queue: &mut CommandQueue<Invocation>)
    -> Result<(), Box<Error>>
  {
    let libraries = self.libraries.clone();
    self.libraries.clear();

    info!("sysroot build order: {:#?}", libraries);

    let mut dlmalloc_built = false;

    for &syslib in libraries.iter() {
      match syslib {
        SystemLibrary::LibC => {
          self.checkout_musl()?;
        },
        SystemLibrary::LibCxx => {
          self.checkout_libcxx()?;
        },
        SystemLibrary::LibCxxAbi => {
          self.checkout_libcxxabi()?;
        },
        SystemLibrary::CompilerRt => {
          self.checkout_compiler_rt()?;
        },
        SystemLibrary::Zlib => {
          self.checkout_zlib()?;
        },
        SystemLibrary::LibUnwind => {
          self.checkout_libunwind()?;
        }
        _ => {},
      }
    }

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
    Some(self.tc().sysroot_cache())
  }
  /// Unconditionally set the output file.
  fn override_output(&mut self, _out: PathBuf) {
    // ignore
  }
}

impl ToolInvocation for Invocation {
  fn check_state(&mut self, iteration: usize, skip_inputs_check: bool)
    -> Result<(), Box<Error>>
  {
    self.init_wasm_tc();
    match iteration {
      3 => {
        if self.libraries.contains(&SystemLibrary::LibCxx) {
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
        WasmToolchain::args(&mut out);
      },
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
  let out = invoc.tc().sysroot_cache()
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

      let mut linker = ld_driver::Invocation::new_with_toolchain(invoc.wasm_toolchain().clone());
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
      linker.add_search_path(invoc.tc().sysroot_lib());
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
      let out = invoc.tc().sysroot_lib()
        .create_if_not_exists()?
        .join(&static_out_name);

      let ar = invoc.tc().llvm_tool("llvm-ar");
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

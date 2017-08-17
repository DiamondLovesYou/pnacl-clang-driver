
use std::collections::HashSet;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use util::{ToolArgs, Tool, ToolInvocation, CommandQueue};
use util::toolchain::WasmToolchain;

pub mod libc;
pub mod libcxx;
pub mod libcxxabi;
pub mod libdlmalloc;
pub mod compiler_rt;

extern crate regex;
#[macro_use]
extern crate util;
#[macro_use]
extern crate lazy_static;
extern crate tempdir;

extern crate clang_driver;
extern crate ld_driver;

const CRATE_ROOT: &'static str = env!("CARGO_MANIFEST_DIR");
fn get_cmake_modules_dir() -> PathBuf {
  let pwd = Path::new(CRATE_ROOT);
  pwd.join("../../cmake/Modules").to_path_buf()
}

#[derive(Debug)]
pub struct Invocation {
  tc: WasmToolchain,
  libraries: Vec<SystemLibrary>,
  libraries_set: HashSet<SystemLibrary>,

  pub emit_llvm: bool,
  pub emit_asm: bool,
  pub emit_wast: bool,
  pub emit_wasm: bool,
}
impl Invocation {
  fn add_library(&mut self, lib: SystemLibrary) {
    if self.libraries_set.insert(lib.clone()) {
      self.libraries.push(lib);
    }
  }
}
impl Default for Invocation {
  fn default() -> Invocation {
    Invocation {
      tc: Default::default(),
      libraries: vec!(),
      libraries_set: Default::default(),

      emit_llvm: false,
      emit_asm: false,
      emit_wast: false,
      emit_wasm: true,
    }
  }
}

#[derive(Debug, PartialOrd, PartialEq, Ord, Eq, Hash, Clone, Copy)]
pub enum SystemLibrary {
  LibC,
  LibCxx,
  LibCxxAbi,
  LibDlMalloc,
  CompilerRt,
}
impl SystemLibrary { }

impl FromStr for SystemLibrary {
  type Err = Box<Error>;
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s {
      "libc" => Ok(SystemLibrary::LibC),
      "libcxx" => Ok(SystemLibrary::LibCxx),
      "libcxxabi" => Ok(SystemLibrary::LibCxxAbi),
      "libdlmalloc" => Ok(SystemLibrary::LibDlMalloc),
      "compiler-rt" => Ok(SystemLibrary::CompilerRt),
      _ => {
        Err(format!("unknown system library: {}", s))?
      },
    }
  }
}

impl Tool for Invocation {
  fn enqueue_commands(&mut self, queue: &mut CommandQueue)
    -> Result<(), Box<Error>>
  {
    let libraries = self.libraries.clone();
    self.libraries.clear();
    self.libraries_set.clear();
    for syslib in libraries.into_iter() {
      match syslib {
        SystemLibrary::LibC => {
          libc::build(self, queue)?;
        },
        SystemLibrary::LibCxx => {
          libcxx::build(self, queue)?;
        },
        SystemLibrary::LibCxxAbi => {
          libcxxabi::build(self, queue)?;
        },
        SystemLibrary::LibDlMalloc => {
          libdlmalloc::build(self, queue)?;
        },
        SystemLibrary::CompilerRt => {
          compiler_rt::build(self, queue)?;
        }
      }
    }

    Ok(())
  }

  fn get_name(&self) -> String {
    "wasm-sysroot".to_string()
  }

  fn add_tool_input(&mut self, input: PathBuf)
    -> Result<(), Box<Error>>
  {
    unimplemented!()
  }

  fn get_output(&self) -> Option<&PathBuf> {
    None
  }
  /// Unconditionally set the output file.
  fn override_output(&mut self, out: PathBuf) {
    panic!();
  }
}

impl ToolInvocation for Invocation {
  fn check_state(&mut self, iteration: usize, _skip_inputs_check: bool)
    -> Result<(), Box<Error>>
  {
    Ok(())
  }

  /// Called until `None` is returned. Put args that override errors before
  /// the the args that can have those errors.
  fn args(&self, iteration: usize) -> Option<ToolArgs<Self>> {
    match iteration {
      0 => tool_arguments!(Invocation => [
        EMIT_LLVM_FLAG,
        EMIT_ASM_FLAG,
        EMIT_WAST_FLAG,
      ]),
      1 => tool_arguments!(Invocation => [
        ARGS,
      ]),
      _ => None,
    }
  }
}

pub fn link(invoc: &Invocation, queue: &mut CommandQueue,
            out_name: &str)
  -> Result<(), Box<Error>>
{
  let out = invoc.tc.emscripten_cache().join(out_name);
  let mut args = Vec::new();
  args.push("-o".to_string());
  args.push(format!("{}", out.display()));

  let mut linker = ld_driver::Invocation::default();
  linker.emit_llvm = invoc.emit_llvm;
  linker.emit_asm  = invoc.emit_asm;
  linker.emit_wast = invoc.emit_wast;
  linker.emit_wasm = invoc.emit_wasm;
  linker.optimize = util::OptimizationGoal::Size;
  let libname = out_name[..out_name.len() - 3].to_string();
  linker.s2wasm_libname = Some(libname);

  let mut cmd = queue
    .enqueue_tool(Some("link"),
                  linker, args,
                  false,
                  None::<Vec<::tempdir::TempDir>>)?;

  cmd.prev_outputs = true;
  cmd.output_override = false;

  Ok(())
}

argument!(impl ARGS where { Some(r"^--build=(.*)$"), None } for Invocation {
    fn args(this, _single, cap) {
      let args = cap.get(1)
        .unwrap().as_str();
      for arg in args.split(',') {
        let res: SystemLibrary = FromStr::from_str(arg)?;
        this.add_library(res);
      }
    }
});
argument!(impl EMIT_LLVM_FLAG where { Some(r"^--emit-llvm$"), None } for Invocation {
    fn emit_llvm_flag(this, _single, _cap) {
      this.emit_llvm = true;
    }
});

argument!(impl EMIT_ASM_FLAG where { Some(r"^--emit-S$"), None } for Invocation {
    fn emit_asm_flag(this, _single, _cap) {
      this.emit_asm = true;
    }
});

argument!(impl EMIT_WAST_FLAG where { Some(r"^--emit-wast$"), None } for Invocation {
    fn emit_wast_flag(this, _single, _cap) {
      this.emit_wast = true;
    }
});

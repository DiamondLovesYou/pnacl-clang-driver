
use std::error::Error;
use std::path::{Path, PathBuf};

use util::{ToolArgs, Tool, ToolInvocation, CommandQueue};
use util::toolchain::WasmToolchain;

extern crate regex;
#[macro_use]
extern crate util;
#[macro_use]
extern crate lazy_static;
extern crate tempdir;

const CRATE_ROOT: &'static str = env!("CARGO_MANIFEST_DIR");
fn get_cmake_modules_dir() -> PathBuf {
  let pwd = Path::new(CRATE_ROOT);
  pwd.join("../../cmake/Modules").to_path_buf()
}

#[derive(Debug)]
pub struct Invocation {
  tc: WasmToolchain,
  args: Vec<String>,
  pub output_dir: PathBuf,
}

impl Invocation {
}
impl Default for Invocation {
  fn default() -> Self {
    Invocation {
      tc: Default::default(),
      args: vec![],
      output_dir: std::env::current_dir()
        .expect("current_dir failed?"),
    }
  }
}

impl Tool for Invocation {
  fn enqueue_commands(&mut self, queue: &mut CommandQueue<Self>)
    -> Result<(), Box<Error>>
  {
    use std::process::Command;
    use tempdir::TempDir;

    let mut cmd = Command::new("cmake");
    cmd.current_dir(self.output_dir.as_path());

    let module_dir = get_cmake_modules_dir();
    let toolchain_file = module_dir.join("Platform/WebAssembly.cmake");
    cmd.arg(format!("-DCMAKE_TOOLCHAIN_FILE={}",
                    toolchain_file.display()));
    cmd.arg(format!("-DCMAKE_CROSSCOMPILING_EMULATOR={}",
                    self.tc.binaryen_tool("wasm-shell").display()));
    cmd.args(self.args.iter());
    cmd.arg("-DCMAKE_VERBOSE_MAKEFILE:BOOL=ON");
    cmd.env("WASM_TC_CMAKE_MODULE_PATH", toolchain_file);

    queue.enqueue_external(Some("cmake"), cmd,
                           None, false, None::<Vec<TempDir>>);

    Ok(())
  }

  fn get_name(&self) -> String {
    "wasm-cmake".to_string()
  }

  fn add_tool_input(&mut self, _input: PathBuf)
    -> Result<(), Box<Error>>
  {
    unimplemented!()
  }

  fn get_output(&self) -> Option<&PathBuf> {
    None
  }
  /// Unconditionally set the output file.
  fn override_output(&mut self, _out: PathBuf) {
    panic!();
  }
}

impl ToolInvocation for Invocation {
  fn check_state(&mut self, _iteration: usize, _skip_inputs_check: bool)
    -> Result<(), Box<Error>>
  {
    Ok(())
  }

  /// Called until `None` is returned. Put args that override errors before
  /// the the args that can have those errors.
  fn args(&self, iteration: usize) -> Option<ToolArgs<Self>> {
    match iteration {
      0 => tool_arguments!(Invocation => [
        ARGS,
      ]),
      _ => None,
    }
  }
}

argument!(impl ARGS where { Some(r"^(.*)$"), None } for Invocation {
    fn args(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.args.push(arg.to_string());
    }
});

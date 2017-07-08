
use std::error::Error;
use std::path::{Path, PathBuf};

use util::{ToolArgs, Tool, ToolInvocation, CommandQueue};
use util::toolchain::WasmToolchain;

extern crate regex;
#[macro_use]
extern crate util;
#[macro_use]
extern crate lazy_static;

const PWD: &'static str = env!("PWD");
fn get_cmake_modules_dir() -> PathBuf {
  let pwd = Path::new(PWD);
  pwd.join("../../cmake/Modules").to_path_buf()
}

#[derive(Debug, Default)]
pub struct Invocation {
  tc: WasmToolchain,
  args: Vec<String>,
}

impl Invocation {
}

impl Tool for Invocation {
  fn enqueue_commands(&mut self, queue: &mut CommandQueue)
    -> Result<(), Box<Error>>
  {
    use std::process::Command;
    let mut cmd = Command::new("cmake");

    let toolchain_file = get_cmake_modules_dir()
      .join("Platform/WebAssembly.cmake");
    cmd.arg(format!("-DCMAKE_TOOLCHAIN_FILE={}",
                    toolchain_file.display()));
    cmd.arg(format!("-DCMAKE_CROSSCOMPILING_EMULATOR={}",
                    self.tc.binaryen_tool("wasm-shell").display()));
    cmd.args(self.args.iter());

    queue.enqueue_external(Some("cmake"), cmd,
                           None, false, None);

    Ok(())
  }

  fn get_name(&self) -> String {
    "wasm-cmake".to_string()
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
  fn check_state(&mut self, iteration: usize)
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


use std::env::{var_os};
use std::path::{Path, PathBuf};
use std::process::Command;

use {CreateIfNotExists, ToolArgs, ToolArg, };

const BINARYEN_ROOT_ENV: &'static str = "BINARYEN";
const EMSCRIPTEN_ROOT_ENV: &'static str = "EMSCRIPTEN";
const LLVM_ROOT_ENV: &'static str = "LLVM_ROOT";
const SYSROOT_ENV: &'static str = "WASM_SYSROOT";

#[derive(Clone, Debug)]
pub struct WasmToolchain {
  pub binaryen: PathBuf,
  pub emscripten: PathBuf,
  pub llvm: PathBuf,

  pub sysroot: PathBuf,
}
impl WasmToolchain {
  pub fn new() -> WasmToolchain {
    fn get_var(var: &str) -> PathBuf {
      let o = var_os(var)
        .unwrap_or_else(|| {
          panic!("need `{}`!", var);
        });

      Path::new(&o).to_path_buf()
    }
    let binaryen = get_var(BINARYEN_ROOT_ENV);
    let emscripten = get_var(EMSCRIPTEN_ROOT_ENV);
    let llvm = get_var(LLVM_ROOT_ENV);

    WasmToolchain {
      binaryen: binaryen,
      emscripten: emscripten,
      llvm,
      sysroot: Self::default_sysroot(),
    }
  }

  pub fn default_sysroot() -> PathBuf {
    use dirs::home_dir;

    var_os(SYSROOT_ENV)
      .map(PathBuf::from)
      .unwrap_or_else(|| {
        home_dir().unwrap()
          .join(".wasm-toolchain")
          .join("sysroot")
          .create_if_not_exists()
          .expect("creating sysroot dir")
      })
  }

  pub fn llvm_tool<T>(&self, tool: T) -> PathBuf
    where T: AsRef<Path> + Sized
  {
    self.llvm
      .join("bin")
      .join(tool)
  }

  pub fn binaryen_tool<T>(&self, tool: T) -> PathBuf
    where T: AsRef<Path> + Sized
  {
    self.binaryen
      .join("bin")
      .join(tool)
      .to_path_buf()
  }
  // we use no emscripten tools

  pub fn emscripten_cache(&self) -> PathBuf {
    use dirs::home_dir;
    home_dir().unwrap()
      .join(".emscripten_cache/wasm")
      .create_if_not_exists()
      .expect("creating emscripten cache dir")
  }
  pub fn sysroot(&self) -> &PathBuf { &self.sysroot }
  pub fn sysroot_cache(&self) -> &PathBuf { &self.sysroot }
  pub fn sysroot_lib(&self) -> PathBuf { self.sysroot.join("lib") }

  pub fn set_envs(&self, cmd: &mut Command) {
    cmd.env(BINARYEN_ROOT_ENV, &self.binaryen)
      .env(EMSCRIPTEN_ROOT_ENV, &self.emscripten)
      .env(LLVM_ROOT_ENV, &self.llvm)
      .env(SYSROOT_ENV, &self.sysroot);
  }

  pub fn args<T>(into: &mut ToolArgs<T>)
    where T: WasmToolchainTool,
  {
    let o = ToolArg {
      name: "sysroot-override".into(),
      single: expand_style_single!(single_and_split_abs_path(doesnt_matter) => "sysroot"),
      split: expand_style_split!(single_and_split_abs_path(doesnt_matter) => "sysroot"),
      help: None,
      action: Some(|this: &mut T, single, cap| {
        let tc = this.wasm_toolchain_mut();
        expand_style!(single_and_split_abs_path(path) => single, cap);
        tc.sysroot = path.create_if_not_exists()?;
        Ok(())
      }),
    };
    into.to_mut().push(o);
  }
}
impl Default for WasmToolchain {
  fn default() -> Self {
    WasmToolchain::new()
  }
}

pub trait WasmToolchainTool {
  fn wasm_toolchain(&self) -> &WasmToolchain;
  fn wasm_toolchain_mut(&mut self) -> &mut WasmToolchain;
}

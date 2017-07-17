
use std::env::{var_os};
use std::path::{Path, PathBuf};

const BINARYEN_ROOT_ENV: &'static str = "BINARYEN";
const EMSCRIPTEN_ROOT_ENV: &'static str = "EMSCRIPTEN";
const LLVM_ROOT_ENV: &'static str = "LLVM_ROOT";

#[derive(Clone, Debug)]
pub struct WasmToolchain {
  pub binaryen: PathBuf,
  pub emscripten: PathBuf,
  pub llvm: PathBuf,
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
      llvm: llvm,
    }
  }

  pub fn llvm_tool<T>(&self, tool: T) -> PathBuf
    where T: AsRef<Path> + Sized
  {
    self.llvm
      .join("bin")
      .join(tool)
      .to_path_buf()
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
    use std::env::home_dir;
    home_dir().unwrap()
      .join(".emscripten_cache/wasm")
  }
}
impl Default for WasmToolchain {
  fn default() -> Self {
    WasmToolchain::new()
  }
}

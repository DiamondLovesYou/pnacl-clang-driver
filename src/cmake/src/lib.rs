
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use std::borrow::Cow;
use std::fmt;

use util::{ToolArgs, Tool, ToolInvocation, CommandQueue,
           CreateIfNotExists, };
use util::toolchain::{WasmToolchain, WasmToolchainTool, };

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

  pub defines: HashMap<String, Var>,

  pub output_dir: PathBuf,
}

impl Invocation {
  pub fn new<T>(tc: WasmToolchain, out: T) -> Result<Self, Box<Error>>
    where T: Into<PathBuf>,
  {
    Ok(Invocation {
      tc,
      args: vec![],
      defines: Default::default(),
      output_dir: out.into().create_if_not_exists()?,
    })
  }
  pub fn with_toolchain<T, U>(tool: &T, out: U) -> Result<Self, Box<Error>>
    where T: WasmToolchainTool,
          U: Into<PathBuf>,
  {
    let tc = tool.wasm_toolchain().clone();
    Self::new(tc, out)
  }
  fn cmake_args_mut(&mut self) -> &mut HashMap<String, Var> {
    &mut self.defines
  }
  pub fn cmake_bool<T>(&mut self, key: T, value: bool) -> &mut Self
    where T: Into<String>,
  {
    self.cmake_args_mut().insert(key.into(), Var::Bool(value));
    self
  }
  pub fn cmake_on<T>(&mut self, key: T) -> &mut Self
    where T: Into<String>,
  {
    self.cmake_bool(key, true)
  }
  pub fn cmake_off<T>(&mut self, key: T) -> &mut Self
    where T: Into<String>,
  {
    self.cmake_bool(key, false)
  }

  pub fn cmake_str<T, U>(&mut self, key: T, str: U) -> &mut Self
    where T: Into<String>,
          U: Into<String>,
  {
    self.cmake_args_mut().insert(key.into(),
                                 Var::str(str.into()));
    self
  }

  pub fn cmake_file<T, U>(&mut self, key: T, path: U) -> &mut Self
    where T: Into<String>,
          U: Into<PathBuf>,
  {
    self.cmake_args_mut().insert(key.into(),
                                 Var::File(path.into()));
    self
  }

  pub fn cmake_path<T, U>(&mut self, key: T, path: U) -> &mut Self
    where T: Into<String>,
          U: Into<PathBuf>,
  {
    self.cmake_args_mut().insert(key.into(),
                                 Var::Path(path.into()));
    self
  }

  pub fn append_str<T, U>(&mut self, key: T, value: U)
    where T: Into<String>,
          U: AsRef<str>,
  {
    use std::collections::hash_map::Entry;
    match self.defines.entry(key.into()) {
      Entry::Occupied(mut o) => {
        let mut str = o.get_mut().force_as_str_mut();
        str.push_str(" ");
        str.push_str(value.as_ref());
      },
      Entry::Vacant( v) => {
        v.insert(Var::String(value.as_ref().to_string().into()));
      },
    }
  }

  pub fn c_cxx_flag<U>(&mut self, value: U) -> &mut Self
    where U: AsRef<str>,
  {
    self.append_str("CMAKE_C_FLAGS", value.as_ref());
    self.append_str("CMAKE_CXX_FLAGS", value);
    self
  }
  pub fn shared_ld_flag<U>(&mut self, value: U) -> &mut Self
    where U: AsRef<str>,
  {
    self.append_str("CMAKE_SHARED_LINKER_FLAGS",
                    value.as_ref());
    self
  }
  pub fn static_ld_flag<U>(&mut self, value: U) -> &mut Self
    where U: AsRef<str>,
  {
    self.append_str("CMAKE_STATIC_LINKER_FLAGS",
                    value.as_ref());
    self
  }
  pub fn exe_ld_flag<U>(&mut self, value: U) -> &mut Self
    where U: AsRef<str>,
  {
    self.append_str("CMAKE_EXE_LINKER_FLAGS",
                    value.as_ref());
    self
  }

  pub fn generator<K>(&mut self, gen: K) -> &mut Self
    where K: Into<String>,
  {
    self.args.push("-G".into());
    self.args.push(gen.into());
    self
  }
}
impl Default for Invocation {
  fn default() -> Self {
    Invocation {
      tc: Default::default(),
      args: vec![],
      defines: Default::default(),
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
    self.tc.set_envs(&mut cmd);
    cmd.current_dir(self.output_dir.as_path());

    let module_dir = get_cmake_modules_dir();
    let toolchain_file = module_dir.join("Platform/WebAssembly.cmake");
    cmd.arg(format!("-DCMAKE_TOOLCHAIN_FILE={}",
                    toolchain_file.display()));
    cmd.arg(format!("-DCMAKE_CROSSCOMPILING_EMULATOR={}",
                    self.tc.binaryen_tool("wasm-shell").display()));
    cmd.args(self.args.iter());
    cmd.arg("-DCMAKE_VERBOSE_MAKEFILE:BOOL=ON");
    cmd.arg("-DWASM:BOOL=ON");
    cmd.env("WASM_TC_CMAKE_MODULE_PATH", toolchain_file);

    for (key, value) in self.defines.iter() {
      let arg = Display(key, value);
      cmd.arg(format!("{}", arg));
    }

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
  fn override_output(&mut self, out: PathBuf) {
    self.output_dir = out;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Var {
  String(Cow<'static, str>),
  File(PathBuf),
  Path(PathBuf),
  Bool(bool),
}

impl Var {
  pub fn static_str(s: &'static str) -> Var {
    Var::String(Cow::Borrowed(s))
  }
  pub fn str(s: String) -> Var {
    Var::String(Cow::Owned(s))
  }
  pub fn file<T>(f: T) -> Var
    where T: Into<PathBuf>,
  {
    Var::File(f.into())
  }
  pub fn path<T>(f: T) -> Var
    where T: Into<PathBuf>,
  {
    Var::Path(f.into())
  }

  pub fn as_str(&self) -> Option<&str> {
    match self {
      &Var::String(ref str) => Some(str.as_ref()),
      _ => None,
    }
  }
  pub fn as_str_mut(&mut self) -> Option<&mut String> {
    match self {
      &mut Var::String(ref mut str) => Some(str.to_mut()),
      _ => None,
    }
  }
  pub fn force_as_str_mut(&mut self) -> &mut String {
    if self.as_str_mut().is_none() {
      *self = Var::String("".into());
    }

    self.as_str_mut().unwrap()
  }

  pub fn type_str(&self) -> &'static str {
    match self {
      &Var::String(..) => "STRING",
      &Var::File(..) => "FILEPATH",
      &Var::Path(..) => "PATH",
      &Var::Bool(..) => "BOOL",
    }
  }
}

impl From<bool> for Var {
  fn from(v: bool) -> Var {
    Var::Bool(v)
  }
}

pub struct Display<'a>(&'a String, &'a Var);
impl<'a> fmt::Display for Display<'a> {
  fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
    write!(fmt, "-D{}:{}=", self.0,
           self.1.type_str())?;
    match self.1 {
      &Var::String(ref s) => fmt.pad(s.as_ref()),
      &Var::File(ref f) |
      &Var::Path(ref f) => write!(fmt, "{}", f.display()),
      &Var::Bool(true) => fmt.pad("ON"),
      &Var::Bool(false) => fmt.pad("OFF"),
    }
  }
}
pub trait ArgDisplay {
  fn display(&self) -> Display;
}
impl<'a> ArgDisplay for (&'a String, &'a Var) {
  fn display(&self) -> Display {
    Display(self.0, self.1)
  }
}

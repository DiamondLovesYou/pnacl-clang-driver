
#![allow(dead_code)]

use std::borrow::ToOwned;
use std::default::Default;
use std::env::{self};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use util::{EhMode, OptimizationGoal, Tool, ToolInvocation,
           CommandQueue, ToolArgs};
use util::{need_nacl_toolchain};
use util::toolchain::WasmToolchain;

#[macro_use]
extern crate util;
#[macro_use]
extern crate lazy_static;
extern crate regex;

extern crate ld_driver;

#[cfg(any(target_os = "nacl", test))]
fn get_inc_path() -> PathBuf {
  static INCLUDE_ROOT: &'static str = "/include";
  Path::new(INCLUDE_ROOT)
    .to_path_buf()
}

#[cfg(all(not(target_os = "nacl"), not(test)))]
fn get_inc_path() -> PathBuf {
  need_nacl_toolchain()
    .join("le32-nacl")
    .join("include")
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum DriverMode {
  CC,
  CXX,
}

impl DriverMode {
  fn new() -> DriverMode {
    use std::env::consts::EXE_SUFFIX;
    let current_exe = env::current_exe();
    let current_exe = current_exe
      .ok()
      .expect("couldn't get the current exe name!");
    let current_exe = current_exe.into_os_string();
    let current_exe = current_exe.into_string()
      .ok()
      .expect("this driver must be nested within utf8 paths");

    let cer: &str = current_exe.as_ref();

    let cer = &cer[..cer.len() - EXE_SUFFIX.len()];

    if cer.ends_with("++") || cer.ends_with("xx") {
      DriverMode::CXX
    } else {
      DriverMode::CC
    }
  }


  fn get_clang_name(&self) -> &'static str {
    match self {
      &DriverMode::CC => "clang",
      &DriverMode::CXX => "clang++",
    }
  }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum GccMode {
  Dashc,
  DashE,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum FileLang {
  C,
  CHeader,
  CppOut,
  Cxx,
  CxxHeader,
  CxxCppOut,
}

impl FileLang {
  fn from_path<P: AsRef<Path>>(p: P) -> Option<FileLang> {
    p.as_ref().extension()
      .and_then(|os_str| os_str.to_str() )
      .and_then(|ext| {
        let r = match ext {
          "c" => FileLang::C,
          "i" => FileLang::CppOut,
          "ii" => FileLang::CxxCppOut,

          "cc" => FileLang::Cxx,
          "cp" => FileLang::Cxx,
          "cxx" => FileLang::Cxx,
          "cpp" => FileLang::Cxx,
          "CPP" => FileLang::Cxx,
          "c++" => FileLang::Cxx,
          "C" => FileLang::Cxx,

          "h" => FileLang::CHeader,

          "hh" => FileLang::CxxHeader,
          "H" => FileLang::CxxHeader,
          "hp" => FileLang::CxxHeader,
          "hxx" => FileLang::CxxHeader,
          "hpp" => FileLang::CxxHeader,
          "HPP" => FileLang::CxxHeader,
          "h++" => FileLang::CxxHeader,
          "tcc" => FileLang::CxxHeader,

          _ => return None,
        };
        Some(r)
      })
  }
}
impl fmt::Display for FileLang {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      &FileLang::C => write!(f, "c"),
      &FileLang::CHeader => write!(f, "c-header"),
      &FileLang::CppOut => write!(f, "cpp-output"),
      &FileLang::Cxx => write!(f, "c++"),
      &FileLang::CxxHeader => write!(f, "c++-header"),
      &FileLang::CxxCppOut => write!(f, "c++-cpp-output"),
    }
  }
}

#[derive(Debug)]
pub struct Invocation {
  tc: WasmToolchain,
  driver_mode: DriverMode,
  gcc_mode: Option<GccMode>,
  eh_mode: EhMode,

  optimization: OptimizationGoal,

  no_default_libs: bool,
  no_std_lib: bool,
  no_default_std_inc: bool,
  no_default_std_incxx: bool,

  shared: bool,

  inputs: Vec<(PathBuf, Option<FileLang>)>,
  header_inputs: Vec<PathBuf>,

  linker_args: Vec<String>,
  driver_args: Vec<String>,

  output: Option<PathBuf>,

  verbose: bool,
  run_queue: Vec<Command>,

  print_version: bool,
}

impl Default for Invocation {
  fn default() -> Invocation {
    Invocation::new()
  }
}

impl Invocation {
  fn new() -> Invocation {
    Invocation::new_driver(DriverMode::new())
  }
  fn new_driver(mode: DriverMode) -> Invocation {
    Invocation {
      tc: WasmToolchain::new(),
      driver_mode: mode,
      gcc_mode: Default::default(),
      eh_mode: Default::default(),

      optimization: Default::default(),

      no_default_libs: false,
      no_std_lib: false,
      no_default_std_inc: false,
      no_default_std_incxx: false,

      shared: false,

      inputs: Default::default(),
      header_inputs: Default::default(),

      linker_args: Default::default(),
      driver_args: Default::default(),

      output: Default::default(),

      verbose: false,
      run_queue: Default::default(),
      print_version: false,
    }
  }

  fn print_help(&self) {
    // TODO print more info about what *this* driver does and doesn't support.
    print!("
This is a \"GCC-compatible\" driver using clang under the hood.
Usage: {} [options] <inputs> ...
BASIC OPTIONS:
  -o <file>             Output to <file>.
  -E                    Only run the preprocessor.
  -S                    Generate bitcode assembly.
  -c                    Generate bitcode object.
  -I <dir>              Add header search path.
  -L <dir>              Add library search path.
  -D<key>[=<val>]       Add definition for the preprocessor.
  -W<id>                Toggle warning <id>.
  -f<feature>           Enable <feature>.
  -Wl,<arg>             Pass <arg> to the linker.
  -Xlinker <arg>        Pass <arg> to the linker.
  -Wp,<arg>             Pass <arg> to the preprocessor.
  -Xpreprocessor,<arg>  Pass <arg> to the preprocessor.
  -x <language>         Treat subsequent input files as having type <language>.
  -static               Produce a static executable (the default).
  -Bstatic              Link subsequent libraries statically (ignored).
  -Bdynamic             Link subsequent libraries dynamically (ignored).
  -fPIC                 Ignored (only used by translator backend)
                        (accepted for compatibility).
  -pipe                 Ignored (for compatibility).
  -O<n>                 Optimation level <n>: 0, 1, 2, 3, 4 or s.
  -g                    Generate complete debug information.
  -gline-tables-only    Generate debug line-information only
                        (allowing for stack traces).
  -flimit-debug-info    Generate limited debug information.
  -save-temps           Keep intermediate compilation results.
  -v                    Verbose output / show commands.
  -h | --help           Show this help.
  --help-full           Show underlying clang driver's help message
                        (warning: not all options supported).
",
           env::args().next().unwrap());
  }

  fn print_clang_help(&self) {
    unimplemented!()
  }

  fn set_verbose(&mut self) {
    self.verbose = true;
  }

  /// Gets the C or CXX std includes, unless self.no_default_std_inc is true
  fn get_std_inc_args(&self) -> Vec<String> {
    let mut isystem = Vec::new();
    let system = self.tc.emscripten.join("system/include");
    if !self.no_default_std_inc {
      if !self.no_default_std_incxx {
        let cxx_inc = system
          .join("libcxx")
          .to_path_buf();
        isystem.push(cxx_inc);
      }
      isystem.push(system.join("compat").to_path_buf());
      isystem.push(system.join("libc").to_path_buf());
      for clang_ver in ["5.0.0"].iter() {
        let c_inc = self.tc.llvm
          .join("lib")
          .join("clang")
          .join(clang_ver)
          .join("include");
        isystem.push(c_inc.to_path_buf());
      }
    }
    isystem
      .into_iter()
      .map(|p| format!("-isystem{}", p.display()) )
      .collect()
  }

  fn get_default_lib_args(&self) -> Vec<PathBuf> {
    let mut libs = Vec::new();
    libs.push(PathBuf::from("-L"));
    libs.push(self.tc.emscripten_cache());
    if self.no_default_libs || self.no_std_lib {
      libs
    } else {
      match self.driver_mode {
        DriverMode::CXX => {
          libs.push(PathBuf::from("-lc++"));
        },
        _ => {}
      }
      libs.push(PathBuf::from("-lc"));
      libs.push(PathBuf::from("-ldlmalloc"));
      libs
    }
  }

  fn set_gcc_mode(&mut self, mode: GccMode) {
    if self.gcc_mode.is_some() {
      panic!("`-c` or `-E` was already specified");
    } else {
      self.gcc_mode = Some(mode);
    }
  }
  fn set_output<T: AsRef<Path>>(&mut self, out: T) {
    if self.output.is_some() {
      panic!("an output is already set: `{}`",
             self.output.clone().unwrap().display());
    } else {
      self.output = Some(out.as_ref().to_path_buf());
    }
  }

  fn get_output(&self) -> PathBuf {
    let out = match self.output {
      Some(ref out) => out.to_path_buf(),
      None => Path::new("a.out").to_path_buf(),
    };

    if self.is_pch_mode() {
      Path::new(&format!("{}.pch", out.display()))
        .to_path_buf()
    } else {
      out
    }
  }

  fn is_pch_mode(&self) -> bool {
    self.header_inputs.len() > 0 && self.gcc_mode != Some(GccMode::DashE)
  }

  fn should_link_output(&self) -> bool {
    self.gcc_mode == None
  }

  #[cfg(all(not(target_os = "nacl"), not(windows)))]
  fn set_ld_library_path(cmd: &mut Command) {
    let lib = {
      let mut tc = need_nacl_toolchain();
      tc.push("lib");
      tc
    };
    let local = env::var_os("LD_LIBRARY_PATH")
      .map(|mut v| {
        v.push(":");
        v.push(lib.clone());
        v
      })
      .unwrap_or_else(|| {
        lib.as_os_str()
          .to_os_string()
      });

    cmd.env("LD_LIBRARY_PATH", local);
  }
  #[cfg(any(target_os = "nacl", windows))]
  fn set_ld_library_path(_cmd: &mut Command) { }

  fn clang_base_cmd(&self) -> Command {
    let clang = self.driver_mode.get_clang_name();
    let mut cmd = Command::new(self.tc.llvm_tool(clang));
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    //Invocation::set_ld_library_path(&mut cmd);

    cmd
  }

  fn clang_add_std_args(&self, cmd: &mut Command) {
    cmd.args(&[
      "-target", "wasm32-unknown-unknown",
      "-mthread-model", "single",
    ]);

    if self.print_version {
      return;
    }

    self.optimization.check();
    cmd.arg(format!("{}", self.optimization));
    cmd.args(&[
      "-nostdinc",
      "-D__EMSCRIPTEN__",
      "-Dasm=ASM_FORBIDDEN",
      "-D__asm__=ASM_FORBIDDEN",
    ]);
    if !self.is_pch_mode() {
      cmd.arg("-emit-llvm");
      match self.gcc_mode {
        None => {},
        Some(GccMode::DashE) => {
          cmd.arg("-E");
        },
        Some(GccMode::Dashc) => {
          cmd.arg("-c");
        },
      }
    }

    cmd.args(&self.get_std_inc_args()[..]);
    cmd.args(&self.driver_args[..]);
  }
  fn clang_add_input_args(&self, cmd: &mut Command) {
    let mut last = None;

    if self.inputs.len() == 0 { panic!("missing inputs!"); }

    for &(ref filename, ref filetype) in self.inputs.iter() {
      match filetype {
        &Some(lang) => {
          cmd.arg("-x");
          cmd.arg(&format!("{}", lang)[..]);
          last = filetype.clone();
        },
        &None => {
          if last.is_some() {
            cmd.args(&["-x", "none"]);
          }
          last = None;
        },
      }
      cmd.arg(filename);
    }
  }

  fn queue_clang(&mut self, queue: &mut CommandQueue) {
    // build the cmd:
    if !self.is_pch_mode() {
      let mut cmd = self.clang_base_cmd();
      self.clang_add_std_args(&mut cmd);
      self.clang_add_input_args(&mut cmd);

      queue.enqueue_external(Some("clang"), cmd,
                             Some("-o"), false,
                             None);
    } else {
      let header_inputs = self.header_inputs.clone();
      let output = self.output.as_ref();
      if header_inputs.len() != 1 &&
        output.is_some() {
        panic!("cannot have -o <out> with multiple header file inputs");
      }

      // TODO: what if `-` is provided?
      for input in header_inputs.into_iter() {
        let mut cmd = self.clang_base_cmd();
        self.clang_add_std_args(&mut cmd);

        let out = output.map(|_| "-o" );
        cmd.arg(input);

        queue.enqueue_external(Some("clang"), cmd,
                               out, false,
                               None);
      }
    }
  }

  fn queue_ld(&mut self, queue: &mut CommandQueue) -> Result<(), Box<Error>> {
    let ld = ld_driver::Invocation::default();

    let mut args = self.linker_args.clone();
    let inputs = self.inputs.iter()
      .map(|&(ref f, _)| format!("{}", f.display()) );
    args.extend(inputs);
    args.push("-target".to_string());
    args.push("wasm32-unknown-unknown".to_string());
    Ok(queue.enqueue_tool(Some("linker"),
                          ld, args, false,
                          None)?)
  }

  fn add_driver_arg<T: AsRef<str>>(&mut self, arg: T) {
    self.driver_args.push(arg.as_ref().to_owned());
  }
  fn add_linker_arg<T: AsRef<str>>(&mut self, arg: T) {
    self.linker_args.push(arg.as_ref().to_owned());
  }
  fn add_input_file<T: AsRef<Path>>(&mut self, file: T,
                                    file_lang: Option<FileLang>) {
    let file = file.as_ref().to_path_buf();
    self.inputs.push((file.clone(), file_lang.clone()));
    let file_lang = file_lang.or_else(|| {
      FileLang::from_path(file.clone())
    });
    let is_header_input = match file_lang {
      Some(FileLang::CHeader) | Some(FileLang::CxxHeader) => true,
      _ => false,
    };

    if is_header_input {
      self.header_inputs.push(file.clone());
    }
  }
}

impl Tool for Invocation {
  fn enqueue_commands(&mut self, queue: &mut CommandQueue) -> Result<(), Box<Error>> {
    if self.print_version {
      let mut clang_ver = self.clang_base_cmd();
      self.clang_add_std_args(&mut clang_ver);
      clang_ver.arg("-v");
      queue.enqueue_external(Some("clang"), clang_ver, None, true, None);
      return Ok(());
    }

    if self.gcc_mode.is_some() {
      self.queue_clang(queue);
    }

    if self.should_link_output() {
      self.queue_ld(queue)?;


    }

    Ok(())
  }

  fn get_name(&self) -> String {
    "wasm-clang".to_string()
  }

  fn add_tool_input(&mut self, input: PathBuf) -> Result<(), Box<Error>> {
    self.add_input_file(input, None);
    Ok(())
  }

  fn get_output(&self) -> Option<&PathBuf> {
    if self.print_version {
      None
    } else {
      self.output.as_ref()
    }
  }
  fn override_output(&mut self, out: PathBuf) {
    self.output = Some(out);
  }
}
impl ToolInvocation for Invocation {
  fn check_state(&mut self, _iteration: usize, _skip_inputs_check: bool) -> Result<(), Box<Error>> {
    Ok(())
  }
  fn args(&self, iteration: usize) -> Option<ToolArgs<Self>> {
    match iteration {
      0 => tool_arguments!(Invocation => [
        VERSION,
        F_POSITION_INDEPENDENT_CODE,
        IGNORED0,
        IGNORED1,
        IGNORED2,
        IGNORED3,
        IGNORED4,
        IGNORED5,
        IGNORED6,
        IGNORED7,
        IGNORED8,
        IGNORED9,
      ]),
      1 => tool_arguments!(Invocation => [
        TARGET,
        INCLUDE_DIR,
        SYSTEM_INCLUDE,
        SYSROOT_INCLUDE,
        QUOTE_INCLUDE,
        DIR_AFTER_INCLUDE,
        M_FLOAT_ABI,
        F_FLAGS,
        D_FLAGS,
        W_FLAGS,
        CAP_M_FLAGS,
        CAP_MF_FLAGS,
        CAP_MT_FLAGS,
        PEDANTIC,
        LINKER_FLAGS0,
        LINKER_FLAGS1,
        SHARED,
        SEARCH_PATH,
        LIBRARY,
        STD_VERSION,
        OPTIMIZE_FLAG,
        DEBUG_FLAGS,
        COMPILE, PREPROCESS,
        OUTPUT,
        UNSUPPORTED,
      ]),
      2 => tool_arguments!(Invocation => [INPUTS,]),
      _ => None,
    }
  }
}

argument!(impl F_POSITION_INDEPENDENT_CODE where { Some(r"^-fPIC$"), None } for Invocation {
    fn f_pos_indep_code(_this, _single, _cap) {
        // ignore
    }
});
argument!(impl IGNORED0 where { Some(r"^-Qy$"), None } for Invocation {
    fn ignored0(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED1 where { Some(r"^--traditional-format$"), None } for Invocation {
    fn ignored1(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED2 where { Some(r"^-(gstabs|gdwarf2)$"), None } for Invocation {
    fn ignored2(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED3 where { Some(r"^--fatal-warnings$"), None } for Invocation {
    fn ignored3(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED4 where { Some(r"^-meabi=(.*)$"), None } for Invocation {
    fn ignored4(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED5 where { Some(r"^-mfpu=(.*)$"), None } for Invocation {
    fn ignored5(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED6 where { Some(r"^-m32$"), None } for Invocation {
    fn ignored6(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED7 where { Some(r"^-emit-llvm$"), None } for Invocation {
    fn ignored7(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED8 where { Some(r"^-msse$"), None } for Invocation {
    fn ignored8(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl IGNORED9 where { Some(r"^-pipe$"), None } for Invocation {
    fn ignored9(_this, _single, _cap) {
      // ignore
    }
});
argument!(impl TARGET where { Some(r"^--?target=(.+)$"), Some(r"^-target$") } for Invocation {
    fn target_arg(_this, single, cap) {
      let target = cap.get(if single { 1 } else { 0 }).unwrap().as_str();
      if target != "wasm32-unknown-unknown" {
        Err("unknown target triple")?;
      }
    }
});
argument!(impl INCLUDE_DIR where { Some(r"^-I(.+)$"), Some(r"^-I$") } for Invocation {
    fn include_dir_arg(this, single, cap) {
      let dir = cap.get(if single { 1 } else { 0 })
        .unwrap().as_str();

      let arg = format!("-I{}", dir);
      this.add_driver_arg(arg);
    }
});
argument!(impl SYSTEM_INCLUDE where { Some(r"^-isystem(.+)$"), Some(r"^-isystem$") } for Invocation {
    fn system_include_arg(this, single, cap) {
      let dir = cap.get(if single { 1 } else { 0 })
        .unwrap().as_str();

      let arg = format!("-isystem{}", dir);
      this.add_driver_arg(arg);
    }
});
argument!(impl SYSROOT_INCLUDE where { Some(r"^-isysroot(.+)$"), Some(r"^-isysroot$") } for Invocation {
    fn sysroot_include_arg(this, single, cap) {
      let dir = cap.get(if single { 1 } else { 0 })
        .unwrap().as_str();

      let arg = format!("-isysroot{}", dir);
      this.add_driver_arg(arg);
    }
});
argument!(impl QUOTE_INCLUDE where { Some(r"^-iquote(.+)$"), Some(r"^-iqoute$") } for Invocation {
    fn quote_include_arg(this, single, cap) {
      let dir = cap.get(if single { 1 } else { 0 })
        .unwrap().as_str();

      let arg = format!("-iqoute{}", dir);
      this.add_driver_arg(arg);
    }
});
argument!(impl DIR_AFTER_INCLUDE where { Some(r"^-idirafter(.+)$"), Some(r"^-idirafter$") } for Invocation {
    fn dir_after_include_arg(this, single, cap) {
      let dir = cap.get(if single { 1 } else { 0 })
        .unwrap().as_str();

      let arg = format!("-idirafter{}", dir);
      this.add_driver_arg(arg);
    }
});
argument!(impl STD_VERSION where { Some(r"^-std=(.+)$"), None } for Invocation {
    fn std_version_arg(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg(arg);
    }
});
argument!(impl M_FLOAT_ABI where { Some(r"^-mfloat-abi=(.+)$"), Some(r"^-mfloat-abi$") } for Invocation {
    fn m_float_abi(this, single, cap) {
      let dir = cap.get(if single { 1 } else { 0 })
        .unwrap().as_str();

      let arg = format!("-mfloat-abi={}", dir);
      this.add_driver_arg(arg);
    }
});
argument!(impl F_FLAGS where { Some(r"^-f(.+)$"), None } for Invocation {
    fn f_flags(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg(arg.to_string());
    }
});
argument!(impl D_FLAGS where { Some(r"^-D(.+)$"), None } for Invocation {
    fn define_flags(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg(arg.to_string());
    }
});
argument!(impl W_FLAGS where { Some(r"^-W(.*)$"), None } for Invocation {
    fn warning_flags(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg(arg.to_string());
    }
});
argument!(impl CAP_M_FLAGS where { Some(r"^-M([^FTQ]?|MD)$"), None } for Invocation {
    fn cap_m_args(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg(arg.to_string());
    }
});
argument!(impl CAP_MF_FLAGS where { None, Some(r"^-MF$") } for Invocation {
    fn cap_mf_args(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg("-MF");
      this.add_driver_arg(arg);
    }
});
argument!(impl CAP_MT_FLAGS where { None, Some(r"^-MT$") } for Invocation {
    fn cap_mt_args(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg("-MT");
      this.add_driver_arg(arg);
    }
});
argument!(impl PEDANTIC where { Some(r"^-(no-)?pedantic$"), None } for Invocation {
    fn pedantic_arg(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg(arg.to_string());
    }
});
argument!(impl LINKER_FLAGS0 where { Some(r"^-Wl,(.+)$"), None } for Invocation {
    fn linker_flags0(this, _single, cap) {
      let args = cap.get(1)
        .unwrap().as_str();
      for arg in args.split(',').filter(|v| v.len() != 0 ) {
        this.add_linker_arg(arg.to_string());
      }
    }
});
argument!(impl LINKER_FLAGS1 where { Some(r"^-Xlinker=(.+)$"), Some(r"^-Xlinker") } for Invocation {
    fn linker_flags1(this, _single, cap) {
      let arg = cap.get(1).unwrap().as_str();
      this.add_linker_arg(arg.to_string());
    }
});
argument!(impl SHARED where { Some(r"^-shared$"), None } for Invocation {
    fn shared_arg(this, _single, _cap) {
      this.shared = true;
    }
});
argument!(impl COMPILE where { Some(r"^-c$"), None } for Invocation {
  fn compile_flag(this, _single, _cap) {
    this.gcc_mode = Some(GccMode::Dashc);
  }
});
argument!(impl PREPROCESS where { Some(r"^-E$"), None } for Invocation {
  fn preprocess_flag(this, _single, _cap) {
    this.gcc_mode = Some(GccMode::DashE);
  }
});
tool_argument!(SEARCH_PATH: Invocation = { Some(r"^-L(.+)$"), Some(r"^-(L|-library-path)$") };
               fn add_search_path(this, _single, cap) {
                 this.add_linker_arg(cap.get(0).unwrap().as_str().to_string());
                 Ok(())
               });
tool_argument!(LIBRARY: Invocation = { Some(r"^-l(.+)$"), Some(r"^-(l|-library)$") };
               fn add_library(this, _single, cap) {
                 this.add_linker_arg(cap.get(0).unwrap().as_str().to_string());
                 Ok(())
               });

tool_argument!(OPTIMIZE_FLAG: Invocation = { Some(r"^-O([0-4sz]?)$"), None };
               fn set_optimize(this, _single, cap) {
                   this.optimization = cap.get(1)
                       .and_then(|str| util::OptimizationGoal::parse(str.as_str()) )
                       .unwrap();
                   Ok(())
               });
argument!(impl DEBUG_FLAGS where { Some(r"^-g$"), None } for Invocation {
    fn debug_flags(this, _single, cap) {
      let arg = cap.get(0)
        .unwrap().as_str();
      this.add_driver_arg(arg.to_string());
    }
});
tool_argument!(OUTPUT: Invocation = { Some(r"^-o(.+)$"), Some(r"^-(o|-output)$") };
               fn set_output(this, single, cap) {
                   if this.output.is_some() {
                       Err("more than one output specified")?;
                   }

                   let out = if single { cap.get(1).unwrap() }
                             else      { cap.get(0).unwrap() };
                   let out = Path::new(out.as_str());
                   let out = out.to_path_buf();
                   this.output = Some(out);
                   Ok(())
               });
argument!(impl UNSUPPORTED where { Some(r"^-.+$"), None } for Invocation {
    fn unsupported_flag(_this, _single, _cap) {
        Err("unsupported argument")?;
    }
});
argument!(impl VERSION where { Some(r"^-v$"), None } for Invocation {
  fn version_flag(this, _single, _cap) {
    this.print_version = true;
  }
});
tool_argument!(INPUTS: Invocation = { Some(r"^(.+)$"), None };
               fn add_input(this, _single, cap) {
                 let p = cap.get(0).unwrap().as_str();
                 let p = Path::new(p).to_path_buf();
                 this.add_input_file(p, None);
                 Ok(())
               });


use std::error::Error;
use std::path::{Path, PathBuf};

use util::{Arch, CommandQueue, regex, };
use util::toolchain::{WasmToolchain, WasmToolchainTool, };

pub use util::ldtools::{Input, };

#[macro_use] extern crate wasm_driver_utils as util;
#[macro_use] extern crate lazy_static;

#[derive(Clone, Debug)]
pub struct Invocation {
  pub tc: WasmToolchain,
  pub allow_native: bool,
  pub use_irt: bool,
  pub abi_check: bool,
  pub run_passes_separately: bool,
  pub relocatable: bool,
  pub use_stdlib: bool,
  pub use_defaultlibs: bool,
  pub pic: bool,
  pub allow_nexe_build_id: bool,

  static_input: bool,

  pub emit_llvm: bool,
  pub emit_asm: bool,
  pub emit_wast: bool,
  pub emit_wasm: bool,

  pub s2wasm_libname: Option<String>,
  pub s2wasm_needed_libs: Vec<String>,

  pub optimize: Option<util::OptimizationGoal>,
  pub lto: bool,
  pub strip: util::StripMode,

  pub eh_mode: util::EhMode,

  pub arch: Option<Arch>,

  pub disabled_passes: Vec<String>,

  bitcode_inputs: Vec<Input>,
  native_inputs: Vec<Input>,
  has_native_inputs: bool,
  has_bitcode_inputs: bool,

  output: Option<PathBuf>,

  pub entry: Option<String>,
  /// symbols which should be force-exported.
  pub exports: Vec<String>,
  global_base: Option<usize>,
  pub import_memory: bool,
  pub import_table: bool,
  pub growable_table_import: bool,

  pub trace: bool,
  pub verbose: bool,

  pub search_paths: Vec<PathBuf>,

  pub soname: Option<String>,

  pub ld_flags: Vec<String>,
  ld_flags_native: Vec<String>,

  trans_flags: Vec<String>,

  // detect mismatched --start-group && --end-group
  grouped: usize,
}

impl Default for Invocation {
  fn default() -> Invocation {
    Self::new_with_toolchain(Default::default())
  }
}

impl Invocation {
  pub fn new_with_toolchain(tc: WasmToolchain) -> Self {
    Invocation {
      tc,
      allow_native: false,
      use_irt: true,
      abi_check: true,
      run_passes_separately: false,
      relocatable: false,
      use_stdlib: true,
      use_defaultlibs: true,
      pic: false,
      allow_nexe_build_id: false,

      static_input: false,

      emit_llvm: false,
      emit_asm: false,
      emit_wast: false,
      emit_wasm: true,

      s2wasm_libname: None,
      s2wasm_needed_libs: vec![],

      optimize: Default::default(),
      lto: false,
      strip: Default::default(),

      eh_mode: Default::default(),

      arch: Default::default(),

      disabled_passes: Default::default(),

      bitcode_inputs: Default::default(),
      native_inputs: Default::default(),
      has_native_inputs: false,
      has_bitcode_inputs: false,

      output: Default::default(),

      entry: None,
      exports: Default::default(),
      global_base: None,
      import_memory: false,
      import_table: false,
      growable_table_import: false,

      trace: false,
      verbose: false,

      search_paths: Default::default(),

      soname: Default::default(),

      ld_flags: Default::default(),
      ld_flags_native: Default::default(),

      trans_flags: Default::default(),

      grouped: 0,
    }
  }
  pub fn with_toolchain<T>(tool: &T) -> Self
    where T: WasmToolchainTool,
  {
    let tc = tool.wasm_toolchain().clone();
    Self::new_with_toolchain(tc)
  }
  pub fn get_static(&self) -> bool {
    !self.relocatable
  }
  pub fn is_portable(&self) -> bool {
    self.get_arch().is_portable()
  }

  pub fn get_arch(&self) -> util::Arch {
    self.arch.unwrap_or_default()
  }

  pub fn has_bitcode_inputs(&self) -> bool {
    self.has_bitcode_inputs
  }
  pub fn has_native_inputs(&self) -> bool {
    self.has_native_inputs
  }

  pub fn get_output(&self) -> PathBuf {
    self.output
      .clone()
      .unwrap_or_else(|| From::from("a.out") )
  }

  pub fn llvm_output_only(&self) -> bool {
    self.emit_llvm && !self.emit_asm &&
      !self.emit_wast && !self.emit_wasm
  }
  pub fn get_llvm_output(&self) -> Option<PathBuf> {
    if self.emit_llvm {
      let mut out = self.get_output();
      if !self.llvm_output_only() {
        let name = format!("{}.bc", {
          out
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
        });
        out.set_file_name(name);
      }
      Some(out)
    } else {
      None
    }
  }
  pub fn asm_output_only(&self) -> bool {
    self.emit_asm && !self.emit_llvm &&
      !self.emit_wast && !self.emit_wasm
  }
  pub fn get_asm_output(&self) -> Option<PathBuf> {
    if self.emit_asm {
      let mut out = self.get_output();
      if !self.asm_output_only() {
        let name = format!("{}.s", {
          out
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
        });
        out.set_file_name(name);
      }
      Some(out)
    } else {
      None
    }
  }
  pub fn wast_output_only(&self) -> bool {
    self.emit_wast && !self.emit_llvm &&
      !self.emit_asm && !self.emit_wasm
  }
  pub fn get_wast_output(&self) -> Option<PathBuf> {
    if self.emit_wast {
      let mut out = self.get_output();
      if !self.llvm_output_only() {
        let name = format!("{}.wast", {
          out
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
        });
        out.set_file_name(name);
      }
      Some(out)
    } else {
      None
    }
  }

  pub fn add_search_path<T>(&mut self, p: T)
    where T: Into<PathBuf>,
  {
    self.search_paths.push(p.into());
  }

  pub fn add_library<T>(&mut self, p: T, abs: bool)
    -> Result<(), Box<Error>>
    where T: Into<PathBuf>,
  {
    let input = Input::Library(abs, p.into());
    self.add_input(input)
  }

  /// Add a non-flag input.
  pub fn add_input(&mut self, input: Input) -> Result<(), Box<Error>> {
    use util::ldtools::*;
    let expanded = expand_input(input, &self.search_paths[..],
                                self.static_input)?;
    'outer: for input in expanded.into_iter() {
      let into = 'inner: loop {
        let _file: &PathBuf = match &input {
          &Input::Library(_, ref p) => p,
          &Input::Flag(ref flag) => {
            let flags = vec![flag.clone()];
            util::process_invocation_args(self, flags,
                                          false)?;
            continue 'outer;
          }
          &Input::File(ref path) => path,
        };

        self.has_bitcode_inputs = true;
        break 'inner &mut self.bitcode_inputs;
      };

      into.push(input);
    }
    Ok(())
  }

  fn check_native_allowed(&self) -> Result<(), Box<Error>> {
    Err("native code is never allowed".into())
  }

  pub fn add_native_ld_flag(&mut self, flag: &str) -> Result<(), Box<Error>> {
    self.check_native_allowed()?;

    self.ld_flags_native.push(flag.to_string());
    Ok(())
  }
  pub fn add_trans_flag(&mut self, flag: &str) -> Result<(), Box<Error>> {
    self.check_native_allowed()?;

    self.trans_flags.push(flag.to_string());
    Ok(())
  }
}

impl util::ToolInvocation for Invocation {
  fn check_state(&mut self, iteration: usize, skip_inputs_check: bool) -> Result<(), Box<Error>> {
    match iteration {
      0 => {
        if self.allow_native && self.arch.is_none() {
          Err("`--pnacl-allow-native` given, but translation is not happening (missing `-target`?)")?;
        }

        if self.use_stdlib {
          // add stdlib locations:
          // lol
        }
      },
      3 if !skip_inputs_check => {
        if !self.has_native_inputs() && !self.has_bitcode_inputs() {
          Err("no inputs")?;
        }
      },

      _ => {},
    }

    Ok(())
  }
  fn args(&self, iteration: usize) -> Option<util::ToolArgs<Invocation>> {
    match iteration {
      0 => {
        tool_arguments!(Invocation => [TARGET, SEARCH_PATH, NO_STDLIB, LLD_FLAVOR_WASM, ])
      },
      1 => tool_arguments!(Invocation => [
        EMIT_LLVM_FLAG,
        EMIT_ASM_FLAG,
        EMIT_WAST_FLAG,
      ]),
      2 => tool_arguments!(Invocation => [
          OUTPUT,
          STATIC,
          RPATH,
          RPATH_LINK,
          SONAME,
          Z_FLAGS,
          PIC_FLAG,
          OPTIMIZE_FLAG,
          LTO_FLAG,
          STRIP_ALL_FLAG,
          STRIP_DEBUG_FLAG,
          LIBRARY,
          GC_SECTIONS,
          MERGE_DATA_SEGMENTS,
          AS_NEEDED_FLAG,
          GROUP_FLAG,
          WHOLE_ARCHIVE_FLAG,
          LINKAGE_FLAG,
          ENTRY,
          IMPORT_TABLE,
          IMPORT_MEMORY,
          GLOBAL_BASE,
          TRACE,
          RELOCATABLE,
          VERBOSE,
          GROWABLE_TABLE_IMPORT,
          VERSION_SCRIPT,
          EXPORT,
          UNDEFINED,
          UNSUPPORTED,
        ]),
      3 => tool_arguments!(Invocation => [
        INPUTS,
      ]),
      _ => None,
    }
  }
}
impl util::Tool for Invocation {
  fn enqueue_commands(&mut self,
                      queue: &mut CommandQueue<Self>) -> Result<(), Box<Error>> {
    use std::process::Command;

    let mut cmd = Command::new(self.tc.llvm_tool("wasm-ld"));
    cmd.arg("--modkit-loader");
    if self.trace {
      cmd.arg("--trace");
    }

    if self.relocatable {
      cmd.arg("--relocatable");
    } else if self.entry.is_none() {
      cmd.arg("--no-entry");
    }
    cmd.args(&self.ld_flags);
    if let Some(ref entry) = self.entry {
      cmd.arg("--entry")
        .arg(entry);
    }
    if let Some(base) = self.global_base {
      cmd.arg(format!("--global-base={}", base));
    }
    if self.import_memory {
      cmd.arg("--import-memory");
    }
    if self.import_table {
      cmd.arg("--import-table");
    }
    if self.verbose {
      cmd.arg("--verbose");
    }
    if self.growable_table_import {
      cmd.arg("--growable-table-import");
    }
    match self.strip {
      util::StripMode::None => {},
      util::StripMode::Debug => {
        cmd.arg("--strip-debug");
      },
      util::StripMode::All => {
        cmd.arg("--strip-all");
      },
    }
    if self.lto {
      let lvl = self.optimize.unwrap();
      cmd.arg(format!("-lto{}", lvl));
    }
    for export in self.exports.iter() {
      cmd.arg(format!("--export={}", export));
    }
    for input in self.bitcode_inputs.iter() {
      match input {
        &Input::Library(false, ref p) => {
          cmd.arg("-L")
            .arg(p.parent().unwrap());

          let name = p.file_name().unwrap()
            .to_str()
            .unwrap();

          let s = if name.starts_with("lib") {
            &name[3..]
          } else {
            &name[..]
          };

          let s = if s.ends_with(".so") {
            &s[..s.len() - 3]
          } else if s.ends_with(".a") {
            &s[..s.len() - 2]
          } else {
            &s[..]
          };


          cmd.arg(format!("-l{}", s));
          continue;
        },
        &Input::Library(true, ref p) => {
          cmd.arg(p);
          continue;
        },
        _ => {},
      }
      cmd.arg(format!("{}", input));
    }

    if !self.relocatable {
      // even in static mode, there will be functions which are provided by
      // the runner.
      cmd.arg("--allow-undefined");
    }

    let output = if self.emit_wast { self.output.take() } else { None };

    queue.enqueue_simple_external(Some("lld"), cmd,
                                  Some("-o".into()))
      .copy_output_to = output.clone();

    if self.emit_wast {
      let wasm_dis = self.tc.binaryen_tool("wasm-dis");
      let mut cmd = Command::new("sh");
      cmd.arg("-c")
        .arg(r#"echo "Writing wast to ${1%.*}.wast"; $0 $1 | c++filt > ${1%.*}.wast"#)
        .arg(wasm_dis)
        .arg(output.as_ref().unwrap());

      queue.enqueue_simple_external(Some("--emit-wast"),
                                    cmd, None)
        .prev_outputs = false;
    }

    Ok(())
  }

  fn add_tool_input(&mut self, input: PathBuf) -> Result<(), Box<Error>> {
    self.add_input(Input::File(input))
  }

  fn get_name(&self) -> String { From::from("wasm-ld") }

  fn get_output(&self) -> Option<&PathBuf> { self.output.as_ref() }
  fn override_output(&mut self, out: PathBuf) { self.output = Some(out); }
}


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

tool_argument!(TARGET: Invocation = { Some(r"^--?target=(.+)$"), Some(r"^--?target$") };
               fn set_target(this, single, cap) {
                   if this.arch.is_some() {
                       Err("the target has already been set")?;
                   }
                   let arch = if single { cap.get(0).unwrap() }
                              else      { cap.get(1).unwrap() };
                   let arch = try!(util::Arch::parse_from_triple(arch.as_str()));
                   this.arch = Some(arch);
                   Ok(())
               });
tool_argument!(LLD_FLAVOR_WASM: Invocation = { None, Some(r#"^-flavor$"#) };
               fn lld_flavor_wasm_arg(_this, _single, cap) {
                   let flavor = cap.get(0).unwrap().as_str();
                   match flavor {
                     "wasm" => Ok(()),
                     _ => {
                       Err(format!("flavor `{}` unsupported, flavor must be `wasm`",
                                   flavor).into())
                     }
                   }
               });
tool_argument!(OUTPUT: Invocation = { Some(r"^-o(.+)$"), Some(r"^-(o|-output)$") };
               fn set_output(this, single, cap) {
                   if this.output.is_some() {
                       Err("more than one output specified")?;
                   }

                   let out = if single { cap.get(0).unwrap() }
                             else      { cap.get(1).unwrap() };
                   let out = Path::new(out.as_str());
                   let out = out.to_path_buf();
                   this.output = Some(out);
                   Ok(())
               });
tool_argument!(STATIC: Invocation = { Some(r"-static"), None };
               fn set_static(this, _single, _cap) {
                   if !this.relocatable {
                       this.static_input = true;
                   } else {
                       this.static_input = false;
                   }
                   Ok(())
               });

tool_argument! {
  pub TRACE: Invocation = simple_no_flag(b) "trace" =>
  fn trace_arg1(this) {
    this.trace = b;
  }
}
tool_argument! {
  pub RELOCATABLE: Invocation = simple_no_flag(b) "relocatable" =>
  fn relocatable_arg1(this) {
    this.relocatable = b;
  }
}
tool_argument! {
  pub IMPORT_TABLE: Invocation = simple_no_flag(b) "import-table" =>
  fn import_table_arg1(this) {
    this.import_table = b;
  }
}
tool_argument! {
  pub VERBOSE: Invocation = simple_no_flag(b) "verbose" =>
  fn verbose_flag(this) {
    this.verbose = b;
  }
}
tool_argument! {
  pub GROWABLE_TABLE_IMPORT: Invocation = simple_no_flag(b) "growable-table-import" =>
  fn growable_table_import_flag(this) {
    this.growable_table_import = b;
  }
}
tool_argument! {
  pub VERSION_SCRIPT: Invocation = single_and_split_abs_path(_path) "version-script" =>
  fn verion_script(_this) {
    // ignore this
  }
}
tool_argument! {
  pub EXPORT: Invocation = single_and_split_from_str(symbol) "export" =>
  fn force_export_arg(this) {
    this.exports.push(symbol);
  }
}
tool_argument! {
  pub GC_SECTIONS: Invocation = simple_no_flag(b) "gc-sections" =>
  fn gc_sections_flag(this) {
    let input = if b {
      "--gc-sections"
    } else {
      "--no-gc-sections"
    };
    this.bitcode_inputs.push(Input::Flag(input.into()));
  }
}
tool_argument! {
  pub MERGE_DATA_SEGMENTS: Invocation = simple_no_flag(b) "merge-data-segments" =>
  fn merge_data_segments_flag(this) {
    let input = if b {
      "--merge-data-segments"
    } else {
      "--no-merge-data-segments"
    };
    this.bitcode_inputs.push(Input::Flag(input.into()));
  }
}

tool_argument!(SEARCH_PATH: Invocation = { Some(r"^-L(.+)$"), Some(r"^-(L|-library-path)$") };
               fn add_search_path(this, single, cap) {
                   let path = if single { cap.get(1).unwrap() }
                              else      { cap.get(0).unwrap() };
                   let path = Path::new(path.as_str());
                   this.search_paths.push(path.to_path_buf());
                   Ok(())
               });
tool_argument!(RPATH: Invocation = { Some(r"^-rpath=(.*)$"), Some(r"^-rpath$") });
tool_argument!(RPATH_LINK: Invocation = { Some(r"^-rpath-link=(.*)$"), Some(r"^-rpath-link$") });

tool_argument!(SONAME: Invocation = { Some(r"-?-soname=(.+)"), Some(r"-?-soname") };
               fn set_soname(this, single, cap) {
                   if this.soname.is_some() {
                       Err("the shared object name has already been set")?;
                   }

                   if single {
                     this.soname = Some(cap.get(0).unwrap().as_str().to_string());
                   } else {
                     this.soname = Some(cap.get(1).unwrap().as_str().to_string());
                   }
                   Ok(())
               });

argument!(impl Z_FLAGS where { None, Some(r"^-z$") } for Invocation {
    fn z_flags(_this, _single, _cap) {
      // TODO
    }
});

tool_argument!(PIC_FLAG: Invocation = { Some(r"^-fPIC$"), None };
               fn set_pic(this, _single, _cap) {
                   this.pic = true;
                   Ok(())
               });

tool_argument!(OPTIMIZE_FLAG: Invocation = { Some(r"^-O([0-4sz]?)$"), None };
               fn set_optimize(this, _single, cap) {
                   let optimize = cap.get(1)
                       .and_then(|str| util::OptimizationGoal::parse(str.as_str()) )
                       .unwrap();
                   this.optimize = Some(optimize);
                   Ok(())
               });
tool_argument!(ENTRY: Invocation = { None, Some(r"^-(e|-entry)$") };
               fn entry_arg(this, _single, cap) {
                   this.entry = Some(cap.get(0).unwrap().as_str().to_owned());
                   Ok(())
               });


tool_argument! {
  pub IMPORT_MEMORY: Invocation = simple_no_flag(yes) "import-memory" =>
  fn import_memory_arg(this) {
    this.import_memory = yes;
  }
}
tool_argument! {
  pub GLOBAL_BASE: Invocation = single_and_split_int(usize, i) "global-base" =>
  fn global_base_arg(this) {
    this.global_base = Some(i);
  }
}

tool_argument!(STRIP_ALL_FLAG: Invocation = { Some(r"^(-s|--strip-all)$"), None };
               fn set_strip_all(this, _single, _cap) {
                   this.strip = util::StripMode::All;
                   Ok(())
               });

tool_argument!(STRIP_DEBUG_FLAG: Invocation = { Some(r"^(-S|--strip-debug)$"), None };
               fn set_strip_debug(this, _single, _cap) {
                   this.strip = util::StripMode::Debug;
                   Ok(())
               });

tool_argument!(LIBRARY: Invocation = { Some(r"^-l([^:]+)$"), Some(r"^-(l|-library)$") };
               fn add_library(this, single, cap) {
                 let i = if single {
                   1
                 } else {
                   0
                 };
                 let path = Path::new(cap.get(i).unwrap().as_str()).to_path_buf();
                 this.add_input(Input::Library(false, path))
               });
tool_argument!(ABS_LIBRARY: Invocation = { Some(r"^-l:(.+)$"), None };
               fn add_abs_library(this, _single, cap) {
                 let path = Path::new(cap.get(1).unwrap().as_str()).to_path_buf();
                 this.add_input(Input::Library(true, path))
               });

fn add_input_flag<'str>(this: &mut Invocation,
                        _single: bool,
                        cap: regex::Captures) -> Result<(), Box<Error>> {
  this.add_input(Input::Flag(From::from(cap.get(0).unwrap().as_str())))?;
  Ok(())
}

tool_argument!(GROUP_FLAG: Invocation = { Some(r"^(--(start|end)-group)$"), None };
               fn add_group_flag(this, single, cap) { add_input_flag(this, single, cap) });
tool_argument!(LINKAGE_FLAG: Invocation = { Some(r"^(-B(static|dynamic))$"), None };
               fn add_linkage_flag(this, single, cap) { add_input_flag(this, single, cap) });

tool_argument! {
  pub AS_NEEDED_FLAG: Invocation = simple_no_flag(b) "as-needed" =>
  fn as_needed_arg(this) {
    let flag = if b {
      "--as-needed"
    } else {
      "--no-as-needed"
    };
    this.add_input(Input::Flag(flag.into()))?;
  }
}
tool_argument! {
  pub WHOLE_ARCHIVE_FLAG: Invocation = simple_no_flag(b) "whole-archive" =>
  fn whole_archive_arg(this) {
    let flag = if b {
      "--whole-archive"
    } else {
      "--no-whole-archive"
    };
    this.add_input(Input::Flag(flag.into()))?;
  }
}

tool_argument!(UNDEFINED: Invocation = { Some(r"^-(-undefined=|u)(.+)$"), Some(r"^-u$") };
               fn add_undefined(_this, single, cap) {
                   let _sym = if single { cap.get(0).unwrap() }
                             else { cap.get(1).unwrap() };

                   unimplemented!();
               });


tool_argument!(LTO_FLAG: Invocation = { Some(r"^-flto$"), None };
               fn set_lto(this, _single, _cap) {
                   this.lto = true;
                   if this.optimize.is_none() {
                     this.optimize = Some(util::OptimizationGoal::Size);
                   }
                   Ok(())
               });

argument!(impl NO_STDLIB where { Some(r"^-nostdlib$"), None } for Invocation {
    fn no_stdlib(this, _single, _cap) {
        this.use_stdlib = false;
    }
});

argument!(impl NO_DEFAULTLIBS where { Some(r"^-nodefaultlibs$"), None } for Invocation {
    fn no_defaultlib(this, _single, _cap) {
        this.use_defaultlibs = false;
    }
});
argument!(impl UNSUPPORTED where { Some(r"^-.+$"), None } for Invocation {
    fn unsupported_flag(_this, _single, _cap) {
        Err("unsupported argument")?;
    }
});

tool_argument!(INPUTS: Invocation = { Some(r"^(.+)$"), None };
               fn add_input(this, _single, cap) {
                 let p = cap.get(0).unwrap().as_str();
                 let p = Path::new(p).to_path_buf();
                 this.add_input(Input::File(p))
               });

#[cfg(test)] #[allow(unused_imports)]
mod tests {
  use util;
  use util::*;
  use util::filetype::*;
  use super::*;

  use std::path::{PathBuf, Path};

  #[test]
  fn unsupported_flag() {
    let args = vec!["-unsupported-flag".to_string()];
    let mut i: Invocation = Default::default();

    assert!(util::process_invocation_args(&mut i, args, false).is_err());
  }

  #[test]
  fn group_flags0() {
    use util::filetype::*;

    override_filetype("libsome.a", Type::Archive(Subtype::Bitcode));
    override_filetype("input.bc", Type::Object(Subtype::Bitcode));

    let args = vec!["input.bc".to_string(),
                    "--start-group".to_string(),
                    "-lsome".to_string(),
                    "--end-group".to_string()];
    let mut i: Invocation = Default::default();
    i.search_paths.push(From::from("."));
    let res = util::process_invocation_args(&mut i, args, false);

    println!("{:?}", i);

    res.unwrap();

    assert!(i.search_paths.contains(&From::from(".")));
    assert!(&i.bitcode_inputs[1..] == &[Path::new("--start-group").to_path_buf(),
      Path::new("-lsome").to_path_buf(),
      Path::new("--end-group").to_path_buf()]);
    assert!(&i.native_inputs[..] == &[Path::new("--start-group").to_path_buf(),
      Path::new("--end-group").to_path_buf()]);

  }

  #[test]
  fn group_flags1() {
    override_filetype("libsome.a", Type::Archive(Subtype::ELF(elf::types::Machine(0))));
    override_filetype("input.o", Type::Object(Subtype::ELF(elf::types::Machine(0))));

    let args = vec!["input.o".to_string(),
                    "--start-group".to_string(),
                    "-lsome".to_string(),
                    "--end-group".to_string()];
    let mut i: Invocation = Default::default();
    i.allow_native = true;
    i.arch = Some(util::Arch::X8664);
    i.search_paths.push(From::from("."));
    let res = util::process_invocation_args(&mut i, args, false);

    println!("{:?}", i);

    res.unwrap();

    assert_eq!(&i.bitcode_inputs[1..], &[Path::new("--start-group").to_path_buf(),
      Path::new("--end-group").to_path_buf()]);

    assert_eq!(&i.native_inputs[..], &[Path::new("--start-group").to_path_buf(),
      Path::new("-lsome").to_path_buf(),
      Path::new("--end-group").to_path_buf()]);

  }

  #[test]
  fn input_arguments_bitcode() {
    override_filetype("input0.bc", Type::Object(Subtype::Bitcode));
    override_filetype("input1.bc", Type::Object(Subtype::Bitcode));

    let args = vec!["input0.bc".to_string(),
                    "input1.bc".to_string()];
    let mut i: Invocation = Default::default();
    util::process_invocation_args(&mut i, args, false).unwrap();

    println!("{:?}", i);

    assert!(&i.bitcode_inputs[..] == &[Path::new("input0.bc").to_path_buf(),
      Path::new("input1.bc").to_path_buf()]);
  }

  #[test]
  fn native_needs_targets() {
    let args = vec!["--pnacl-allow-native".to_string()];
    let mut i: Invocation = Default::default();
    let res = util::process_invocation_args(&mut i, args, false);
    println!("{:?}", i);
    assert!(res.is_err());


    override_filetype("input.o", Type::Object(Subtype::Bitcode));
    let args = vec!["input.o".to_string(),
                    "--pnacl-allow-native".to_string(),
                    "--target=arm-nacl".to_string()];
    let mut i: Invocation = Default::default();
    let res = util::process_invocation_args(&mut i, args, false);
    println!("{:?}", i);
    res.unwrap();

  }

  #[test]
  fn native_disallowed() {
    override_filetype("input.o", Type::Object(Subtype::ELF(elf::types::Machine(0))));

    let args = vec!["input.o".to_string()];
    let mut i: Invocation = Default::default();

    let res = util::process_invocation_args(&mut i, args, false);
    println!("{:?}", i);
    assert!(res.is_err());
  }
  #[test]
  fn no_inputs() {
    let args = vec![];
    let mut i: Invocation = Default::default();
    let res = util::process_invocation_args(&mut i, args, false);
    println!("{:?}", i);
    assert!(res.is_err());
  }
}

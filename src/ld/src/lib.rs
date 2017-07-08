
use std::error::Error;
use std::path::{Path, PathBuf};

use util::{Arch, CommandQueue};

pub use util::ldtools::{Input, AllowedTypes};
use util::toolchain::WasmToolchain;

extern crate regex;
#[macro_use] extern crate util;
#[macro_use] extern crate lazy_static;
extern crate ar;
extern crate tempdir;

const BASE_UNRESOLVED: &'static [&'static str] = &[
  // The following functions are implemented in the native support library.
  // Before a .pexe is produced, they get rewritten to intrinsic calls.
  // However, this rewriting happens after bitcode linking - so gold has
  // to be told that these are allowed to remain unresolved.
  "--allow-unresolved=memcpy",
  "--allow-unresolved=memset",
  "--allow-unresolved=memmove",
  "--allow-unresolved=setjmp",
  "--allow-unresolved=longjmp",

  // These TLS layout functions are either defined by the ExpandTls
  // pass or (for non-ABI-stable code only) by PNaCl's native support
  // code.
  "--allow-unresolved=__nacl_tp_tls_offset",
  "--allow-unresolved=__nacl_tp_tdb_offset",

  // __nacl_get_arch() is for non-ABI-stable code only.
  "--allow-unresolved=__nacl_get_arch",
];

const SJLJ_UNRESOLVED: &'static [&'static str] = &[
  // These symbols are defined by libsupc++ and the PNaClSjLjEH
  // pass generates references to them.
  "--undefined=__pnacl_eh_stack",
  "--undefined=__pnacl_eh_resume",

  // These symbols are defined by the PNaClSjLjEH pass and
  // libsupc++ refers to them.
  "--allow-unresolved=__pnacl_eh_type_table",
  "--allow-unresolved=__pnacl_eh_action_table",
  "--allow-unresolved=__pnacl_eh_filter_table",
];

const ZEROCOST_UNRESOLVED: &'static [&'static str] =
  &["--allow-unresolved=_Unwind_Backtrace",
    "--allow-unresolved=_Unwind_DeleteException",
    "--allow-unresolved=_Unwind_GetCFA",
    "--allow-unresolved=_Unwind_GetDataRelBase",
    "--allow-unresolved=_Unwind_GetGR",
    "--allow-unresolved=_Unwind_GetIP",
    "--allow-unresolved=_Unwind_GetIPInfo",
    "--allow-unresolved=_Unwind_GetLanguageSpecificData",
    "--allow-unresolved=_Unwind_GetRegionStart",
    "--allow-unresolved=_Unwind_GetTextRelBase",
    "--allow-unresolved=_Unwind_PNaClSetResult0",
    "--allow-unresolved=_Unwind_PNaClSetResult1",
    "--allow-unresolved=_Unwind_RaiseException",
    "--allow-unresolved=_Unwind_Resume",
    "--allow-unresolved=_Unwind_Resume_or_Rethrow",
    "--allow-unresolved=_Unwind_SetGR",
    "--allow-unresolved=_Unwind_SetIP",
  ];

const SPECIAL_LIBS: &'static [(&'static str, (&'static str, bool))] =
  &[("-lnacl", ("nacl_sys_private", true)),
    ("-lpthread", ("pthread_private", false)),
  ];

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
  pub static_: bool,

  pub optimize: util::OptimizationGoal,
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

  pub search_paths: Vec<PathBuf>,

  pub soname: Option<String>,

  ld_flags: Vec<String>,
  ld_flags_native: Vec<String>,

  trans_flags: Vec<String>,

  // detect mismatched --start-group && --end-group
  grouped: usize,
}

impl Default for Invocation {
  fn default() -> Invocation {
    Invocation {
      tc: WasmToolchain::new(),
      allow_native: false,
      use_irt: true,
      abi_check: true,
      run_passes_separately: false,
      relocatable: false,
      use_stdlib: true,
      use_defaultlibs: true,
      pic: false,
      allow_nexe_build_id: false,
      static_: true,

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

      search_paths: Default::default(),

      soname: Default::default(),

      ld_flags: Default::default(),
      ld_flags_native: Default::default(),

      trans_flags: Default::default(),

      grouped: 0,
    }
  }
}

impl Invocation {
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

  /// Add a non-flag input.
  pub fn add_input(&mut self, input: Input) -> Result<(), Box<Error>> {
    use util::ldtools::*;
    use util::filetype::is_file_native;
    let expanded = expand_input(input, &self.search_paths[..], false)?;
    for input in expanded.into_iter() {
      let into = 'outer: loop {
        let file = match &input {
          &Input::Library(_, _, AllowedTypes::Any) => unreachable!(),
          &Input::Library(_, _, ty) => {
            if ty == AllowedTypes::Native {
              self.check_native_allowed()?;
              break &mut self.native_inputs;
            } else {
              break &mut self.bitcode_inputs;
            }
          },
          &Input::Flag(ref _flag) => {
            panic!("TODO: linker scripts");
          }
          &Input::File(ref path) => path,
        };

        if is_file_native(file) {
          self.check_native_allowed()?;
          break &mut self.native_inputs;
        } else {
          break &mut self.bitcode_inputs;
        }
      };

      into.push(input);
    }
    Ok(())
  }

  fn check_native_allowed(&self) -> Result<(), Box<Error>> {
    Ok(Err("native code is never allowed")?)
  }

  pub fn add_native_ld_flag(&mut self, flag: &str) -> Result<(), Box<Error>> {
    try!(self.check_native_allowed());

    self.ld_flags_native.push(flag.to_string());
    Ok(())
  }
  pub fn add_trans_flag(&mut self, flag: &str) -> Result<(), Box<Error>> {
    try!(self.check_native_allowed());

    self.trans_flags.push(flag.to_string());
    Ok(())
  }
}

impl util::ToolInvocation for Invocation {
  fn check_state(&mut self, iteration: usize) -> Result<(), Box<Error>> {
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
      1 => {
        if !self.has_native_inputs() && !self.has_bitcode_inputs() {
          Err("no inputs")?;
        }
      },

      _ => unreachable!(),
    }

    Ok(())
  }
  fn args(&self, iteration: usize) -> Option<util::ToolArgs<Invocation>> {
    match iteration {
      0 => {
        tool_arguments!(Invocation => [TARGET, SEARCH_PATH, NO_STDLIB, ])
      },
      1 => {
        tool_arguments!(Invocation => [
          OUTPUT,
          STATIC,
          RPATH,
          RPATH_LINK,
          SONAME,
          PIC_FLAG,
          OPTIMIZE_FLAG,
          LTO_FLAG,
          STRIP_ALL_FLAG,
          STRIP_DEBUG_FLAG,
          LIBRARY,
          AS_NEEDED_FLAG,
          GROUP_FLAG,
          WHOLE_ARCHIVE_FLAG,
          LINKAGE_FLAG,
          UNDEFINED,
          UNSUPPORTED, // must be before INPUTS.
          INPUTS,
        ])
      },
      _ => None,
    }
  }
}
impl util::Tool for Invocation {
  fn enqueue_commands(&mut self, queue: &mut CommandQueue) -> Result<(), Box<Error>> {
    use std::fs::File;
    use std::env::home_dir;
    use std::io::{copy, Write};
    use std::process::Command;

    use tempdir::TempDir;

    if self.has_bitcode_inputs() {
      /// all inputs will be give in absolute path form.

      let mut cmd = Command::new(self.tc.llvm_tool("llvm-link"));
      let mut tmpdirs = vec![];
      for input in self.bitcode_inputs.iter() {
        match input {
          &Input::Library(_, ref path, _) => {
            let tmp = TempDir::new("wasm-ld-archive")?;
            let mut ar = ar::Archive::new(File::open(path)?);
            while let Some(entry) = ar.next_entry() {
              let mut entry = entry?;
              let out = tmp
                .path()
                .join(entry.header().identifier())
                .to_path_buf();

              {
                let mut out = File::create(out.as_path())?;
                copy(&mut entry, &mut out)?;
                out.flush()?;
              }
              cmd.arg(out);
            }

            tmpdirs.push(tmp);
          },
          &Input::File(ref file) => {
            cmd.arg(file);
          },
          &Input::Flag(_) => unreachable!(),
        }
      }

      queue.enqueue_external(Some("link"), cmd, Some("-o"), false,
                             Some(tmpdirs));

      let mut cmd = Command::new(self.tc.llvm_tool("llc"));
      cmd.arg(format!("-march={}", self.get_arch().llvm_arch()))
        .arg("-filetype=asm")
        .arg("-asm-verbose=true")
        .arg("-thread-model=single")
        .arg("-combiner-global-alias-analysis=false");

      queue.enqueue_external(Some("llc"), cmd, Some("-o"), false,
                             None);

      let emscripten_cache = home_dir().unwrap()
        .join(".emscripten_cache/wasm");
      let mut cmd = Command::new(self.tc.binaryen_tool("s2wasm"));
      cmd.arg("--binary")
        .arg("--dylink")
        .arg("-l")
        .arg(emscripten_cache.join("wasm_compiler_rt.a"))
        .arg("-l")
        .arg(emscripten_cache.join("wasm_libc_rt.a"));

      queue.enqueue_external(Some("s2wasm"), cmd, Some("-o"), false,
                             None);
    }

    assert!(!self.has_native_inputs());

    Ok(())
  }

  fn add_tool_input(&mut self, input: PathBuf) -> Result<(), Box<Error>> {
    self.add_input(Input::File(input))
  }

  fn get_name(&self) -> String { From::from("wasm-ld") }

  fn get_output(&self) -> Option<&PathBuf> { self.output.as_ref() }
  fn override_output(&mut self, out: PathBuf) { self.output = Some(out); }
}

tool_argument!(TARGET: Invocation = { Some(r"--target=(.+)"), Some(r"-target") };
               fn set_target(this, single, cap) {
                   if this.arch.is_some() {
                       Err("the target has already been set")?;
                   }
                   let arch = if single { cap.get(1).unwrap() }
                              else      { cap.get(0).unwrap() };
                   let arch = try!(util::Arch::parse_from_triple(arch.as_str()));
                   this.arch = Some(arch);
                   Ok(())
               });
tool_argument!(OUTPUT: Invocation = { Some(r"-o(.+)"), Some(r"-(o|-output)") };
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
tool_argument!(STATIC: Invocation = { Some(r"-static"), None };
               fn set_static(this, _single, _cap) {
                   if !this.relocatable {
                       this.static_ = true;
                   } else {
                       this.static_ = false;
                   }
                   Ok(())
               });
/*static RELOCATABLE1: ToolArg = util::ToolArg {
  single: Some(regex!(r"-r")),
  split: None,
  action: Some(set_relocatable as ToolArgActionFn),
};
static RELOCATABLE2: ToolArg = util::ToolArg {
  single: Some(regex!(r"-relocatable")),
  split: None,
  action: Some(set_relocatable as ToolArgActionFn),
};
static RELOCATABLE3: ToolArg = util::ToolArg {
  single: Some(regex!(r"-i")),
  split: None,
  action: Some(set_relocatable as ToolArgActionFn),
};
fn set_relocatable<'str>(this: &mut Invocation, _single: bool,
                         _: regex::Captures) -> Result<(), String> {
  this.relocatable = true;
  this.static_ = false;
  Ok(())
}*/

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

fn add_to_native_link_flags(this: &mut Invocation, _single: bool,
                            cap: regex::Captures) -> Result<(), Box<Error>> {
  this.add_native_ld_flag(cap.get(0).unwrap().as_str())
}
fn add_to_bc_link_flags(this: &mut Invocation, _single: bool,
                        cap: regex::Captures) -> Result<(), Box<Error>> {
  this.ld_flags.push(cap.get(0).unwrap().as_str().to_string());
  Ok(())
}
fn add_to_both_link_flags(this: &mut Invocation, _single: bool,
                          cap: regex::Captures) -> Result<(), Box<Error>> {
  let flag = cap.get(0).unwrap().as_str().to_string();
  this.ld_flags.push(flag.clone());
  this.add_native_ld_flag(&flag[..])
}

/*static LINKER_SCRIPT: ToolArg = util::ToolArg {
  single: None,
  split: Some(regex!(r"^(-T)$")),
  action: Some(add_to_native_link_flags as ToolArgActionFn),
};
/// TODO(pdox): Allow setting an alternative _start symbol in bitcode
static HYPHIN_E: ToolArg = util::ToolArg {
  single: None,
  split: Some(regex!(r"^(-e)$")),
  action: Some(add_to_both_link_flags as ToolArgActionFn),
};

/// TODO(pdox): Support GNU versioning.
tool_argument!(VERSION_SCRIPT: Invocation = { r"^--version-script=.*$", None });

static SEGMENT: ToolArg = util::ToolArg {
  single: Some(regex!(r"^(-T(text|rodata)-segment=.*)$")),
  split: None,
  action: Some(add_to_native_link_flags as ToolArgActionFn),
};
static SECTION_START: ToolArg = util::ToolArg {
  single: None,
  split: Some(regex!(r"^--section-start$")),
  action: Some(section_start as ToolArgActionFn),
};
fn section_start(this: &mut Invocation,
                 _single: bool,
                 cap: regex::Captures) -> Result<(), String> {
  try!(this.add_native_ld_flag("--section-start"));
  this.add_native_ld_flag(cap.at(1).unwrap())
}
static BUILD_ID: ToolArg = util::ToolArg {
  single: None,
  split: Some(regex!(r"^--build-id$")),
  action: Some(build_id as ToolArgActionFn),
};
fn build_id<'str>(this: &mut Invocation,
                  _single: bool,
                  _cap: regex::Captures) -> Result<(), String> {
  this.add_native_ld_flag("--build-id")
}

/// NOTE: -export-dynamic doesn't actually do anything to the bitcode link
/// right now. This is just in case we do want to record that in metadata
/// eventually, and have that influence the native linker flags.
static EXPORT_DYNAMIC: ToolArg = util::ToolArg {
  single: Some(regex!(r"(-export-dynamic)")),
  split: None,
  action: Some(add_to_bc_link_flags as ToolArgActionFn),
};*/

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
/*
argument!(impl PASSTHROUGH_BC_LINK_FLAGS1 where { Some(r"(-M|--print-map|-t|--trace)"), None } for Invocation
          => Some(add_to_bc_link_flags));

static PASSTHROUGH_BC_LINK_FLAGS2: ToolArg = util::ToolArg {
  single: None,
  split: Some(regex!(r"-y")),
  action: Some(passthrough_bc_link_flags2 as ToolArgActionFn),
};
fn passthrough_bc_link_flags2<'str>(this: &mut Invocation,
                                    _single: bool,
                                    cap: regex::Captures) -> Result<(), String> {
  this.ld_flags.push("-y".to_string());
  this.ld_flags.push(cap.at(1).unwrap().to_string());
  Ok(())
}
static PASSTHROUGH_BC_LINK_FLAGS3: ToolArg = util::ToolArg {
  single: None,
  split: Some(regex!(r"-defsym")),
  action: Some(passthrough_bc_link_flags3 as ToolArgActionFn),
};
fn passthrough_bc_link_flags3<'str>(this: &mut Invocation,
                                    _single: bool,
                                    cap: regex::Captures) -> Result<(), String> {
  this.ld_flags.push("-defsym".to_string());
  this.ld_flags.push(cap.at(0).unwrap().to_string());
  Ok(())
}
static PASSTHROUGH_BC_LINK_FLAGS4: ToolArg = util::ToolArg {
  single: Some(regex!(r"^-?-wrap=(.+)$")),
  split: Some(regex!(r"^-?-wrap$")),
  action: Some(passthrough_bc_link_flags4 as ToolArgActionFn),
};
fn passthrough_bc_link_flags4<'str>(this: &mut Invocation,
                                    _single: bool,
                                    cap: regex::Captures) -> Result<(), String> {
  this.ld_flags.push("-wrap".to_string());
  this.ld_flags.push(cap.at(0).unwrap().to_string());
  Ok(())
}*/

tool_argument!(PIC_FLAG: Invocation = { Some(r"^-fPIC$"), None };
               fn set_pic(this, _single, _cap) {
                   this.pic = true;
                   Ok(())
               });

tool_argument!(OPTIMIZE_FLAG: Invocation = { Some(r"^-O([0-4sz]?)$"), None };
               fn set_optimize(this, _single, cap) {
                   this.optimize = cap.get(0)
                       .and_then(|str| util::OptimizationGoal::parse(str.as_str()) )
                       .unwrap();
                   Ok(())
               });

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

tool_argument!(LIBRARY: Invocation = { Some(r"^-l(.+)$"), Some(r"^-(l|-library)$") };
               fn add_library(this, single, cap) {
                 let i = if single {
                   0
                 } else {
                   1
                 };
                 let path = Path::new(cap.get(i).unwrap().as_str()).to_path_buf();
                 this.add_input(Input::Library(false, path, AllowedTypes::Any))
               });

fn add_input_flag<'str>(this: &mut Invocation,
                        _single: bool,
                        cap: regex::Captures) -> Result<(), Box<Error>> {
  this.add_input(Input::Flag(From::from(cap.get(0).unwrap().as_str())))?;
  Ok(())
}

argument!(impl AS_NEEDED_FLAG where { Some(r"^(-(-no)?-as-needed)$"), None } for Invocation => Some(add_input_flag));
argument!(impl GROUP_FLAG where { Some(r"^(--(start|end)-group)$"), None } for Invocation => Some(add_input_flag));
argument!(impl WHOLE_ARCHIVE_FLAG where { Some(r"^(-?-(no-)whole-archive)$"), None } for Invocation => Some(add_input_flag));
argument!(impl LINKAGE_FLAG where { Some(r"^(-B(static|dynamic))$"), None } for Invocation => Some(add_input_flag));

tool_argument!(UNDEFINED: Invocation = { Some(r"^-(-undefined=|u)(.+)$"), Some(r"^-u$") };
               fn add_undefined(_this, single, cap) {
                   let sym = if single { cap.get(0).unwrap() }
                             else { cap.get(1).unwrap() };

                   unimplemented!();
                   Ok(())
               });


tool_argument!(LTO_FLAG: Invocation = { Some(r"^-flto$"), None };
               fn set_lto(this, _single, _cap) {
                   this.lto = true;
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

    assert!(util::process_invocation_args(&mut i, args).is_err());
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
    let res = util::process_invocation_args(&mut i, args);

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
    let res = util::process_invocation_args(&mut i, args);

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
    util::process_invocation_args(&mut i, args).unwrap();

    println!("{:?}", i);

    assert!(&i.bitcode_inputs[..] == &[Path::new("input0.bc").to_path_buf(),
      Path::new("input1.bc").to_path_buf()]);
  }

  #[test]
  fn native_needs_targets() {
    let args = vec!["--pnacl-allow-native".to_string()];
    let mut i: Invocation = Default::default();
    let res = util::process_invocation_args(&mut i, args);
    println!("{:?}", i);
    assert!(res.is_err());


    override_filetype("input.o", Type::Object(Subtype::Bitcode));
    let args = vec!["input.o".to_string(),
                    "--pnacl-allow-native".to_string(),
                    "--target=arm-nacl".to_string()];
    let mut i: Invocation = Default::default();
    let res = util::process_invocation_args(&mut i, args);
    println!("{:?}", i);
    res.unwrap();

  }

  #[test]
  fn native_disallowed() {
    override_filetype("input.o", Type::Object(Subtype::ELF(elf::types::Machine(0))));

    let args = vec!["input.o".to_string()];
    let mut i: Invocation = Default::default();

    let res = util::process_invocation_args(&mut i, args);
    println!("{:?}", i);
    assert!(res.is_err());
  }
  #[test]
  fn no_inputs() {
    let args = vec![];
    let mut i: Invocation = Default::default();
    let res = util::process_invocation_args(&mut i, args);
    println!("{:?}", i);
    assert!(res.is_err());
  }
}

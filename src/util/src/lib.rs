#![feature(trace_macros)]
#![feature(box_syntax)]
#![feature(fnbox, fn_traits, unboxed_closures)]
#![cfg_attr(test, feature(set_stdio))]

use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self};
use std::io::{Write};
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::process;

pub use command_queue::{CommandQueueError, CommandQueue,
                        Command};

pub extern crate regex;
extern crate tempdir;
extern crate ctrlc;
extern crate dirs;
extern crate git2;
#[macro_use]
extern crate log;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate maplit;

#[macro_export] macro_rules! tool_arguments {
  ($ty:ty => [ $( $arg:expr, )* ]) => ({
    Some(vec![
      $(
        ($arg).clone()
      ),*
    ].into())
  });
}

#[macro_export] macro_rules! expand_style (
  (single_and_split_simple_path($path_name:ident) => $single:ident, $cap:ident) => {
    let path = if !$single {
      $cap.get(0)
        .unwrap()
        .as_str()
    } else {
      $cap.get(1)
        .unwrap()
        .as_str()
    };
    let $path_name = ::std::path::Path::new(path);
  };
  (single_and_split_abs_path($path_name:ident) => $single:ident, $cap:ident) => {
    expand_style!(single_and_split_simple_path($path_name) => $single, $cap);
    let $path_name = ::std::env::current_dir()?.join($path_name);
  };
  (single_and_split_abs_path($path_name:ident, $hyphen:ident) => $single:ident, $cap:ident) => {
    expand_style!(single_and_split_abs_path($path_name) => $single, $cap);
  };
  (single_and_split_str($name:ident) => $single:ident, $cap:ident) => {
    let $name = if !$single {
      $cap.get(0)
        .unwrap()
        .as_str()
    } else {
      $cap.get(1)
        .unwrap()
        .as_str()
    };
  };
  (single_and_split_int($ity:ident, $out_name:ident) => $single:ident, $cap:ident) => {
    let str = if !$single {
      $cap.get(0)
        .unwrap()
        .as_str()
    } else {
      $cap.get(1)
        .unwrap()
        .as_str()
    };
    let $out_name = $ity::from_str_radix(str, 10)?;
  };
  (simple_able_boolean($boolean_name:ident) => $single:ident, $cap:ident) => {
    let sw = $cap.get(1).unwrap().as_str();
    let $boolean_name = match sw {
      "enable" => true,
      "disable" => false,
      _ => unreachable!(),
    };
  };
  (simple_no_flag($boolean_name:ident) => $single:ident, $cap:ident) => {
    let $boolean_name = match $cap.get(1) {
      Some(v) if v.as_str() == "no-" => false,
      _ => true,
    };
  };
  (single_and_split_from_str($out_name:ident) => $single:ident, $cap:ident) => {
    let str = if !$single {
      $cap.get(0)
        .unwrap()
        .as_str()
    } else {
      $cap.get(1)
        .unwrap()
        .as_str()
    };
    let $out_name = ::std::str::FromStr::from_str(str)?;
  };
  (single_and_split_from_str($out_name:ident, $hyphen_mode:ident) => $single:ident, $cap:ident) => {
    expand_style!(single_and_split_from_str($out_name) => $single, $cap)
  };
  (short_flag($out_name:ident, $hyphen_is_required:ident) => $single:ident, $cap:ident) => {
     let $out_name = true;
  };
);

#[macro_export] macro_rules! expand_style_single (
  (single_and_split_simple_path($path_name:ident) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--",$param,"=(.*)$")))
  };
  (single_and_split_abs_path($path_name:ident) => $param:expr) => {
    expand_style_single!(single_and_split_simple_path($path_name) => $param)
  };
  (single_and_split_abs_path($path_name:ident, no_hyphen) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^-",$param,"=(.*)$")))
  };
  (single_and_split_int($ity:ident, $int_name:ident) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--",$param,"=([0-9]*)$")))
  };
  (simple_able_boolean($boolean_name:ident) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--(enable|disable)-",
                                              $param, "$")))
  };
  (simple_no_flag($boolean_name:ident) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--(no-)?",
                                              $param, "$")))
  };
  (single_and_split_from_str($out:ident) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--",$param,"=(.*)$")))
  };
  (single_and_split_from_str($out:ident, optional_hyphen) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--?",$param,"=(.*)$")))
  };
  (short_flag($out:ident, disallowed) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^-", $param, "$")))
  };
  (short_flag($out:ident, optional) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--?", $param, "$")))
  };
  (short_flag($out:ident, required) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--", $param, "$")))
  };
);
#[macro_export] macro_rules! expand_style_split (
  (single_and_split_simple_path($path_name:ident) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--",$param,"$")))
  };
  (single_and_split_abs_path($path_name:ident) => $param:expr) => {
    expand_style_split!(single_and_split_simple_path($path_name) => $param)
  };
  (single_and_split_abs_path($path_name:ident, no_hyphen) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^-",$param,"$")))
  };
  (single_and_split_int($ity:ident, $int_name:ident) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--",$param,"$")))
  };
  (simple_able_boolean($boolean_name:ident) => $param:expr) => {
    None
  };
  (simple_no_flag($boolean_name:ident) => $param:expr) => {
    None
  };
  (single_and_split_from_str($out:ident) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--",$param,"$")))
  };
  (single_and_split_from_str($out:ident, optional_hyphen) => $param:expr) => {
    Some(::std::borrow::Cow::Borrowed(concat!("^--?",$param,"$")))
  };
  (short_flag($out:ident, $hyphen_is_required:ident) => $param:expr) => {
    None
  };
);

/// TODO create a proc macro to handle the explosion of options
#[macro_export] macro_rules! tool_argument(
    (pub $name:ident: $ty:ty = $style:ident($($style_args:ident),*) $param_name:expr =>
     fn $fn_name:ident($this_name:ident) $fn_body:block) =>
  {
    #[allow(non_snake_case)]
    pub const $name: $crate::ToolArg<$ty> = $crate::ToolArg {
      name: ::std::borrow::Cow::Borrowed(stringify!($name)),
      single: expand_style_single!($style($($style_args),*) => $param_name),
      split:  expand_style_split!($style($($style_args),*) => $param_name),
      action: Some(|this: &mut $ty, single: bool, cap: $crate::regex::Captures| {
        $fn_name(this, single, cap)
      }),
      help: None,
    };
    #[allow(unused_variables)]
    fn $fn_name($this_name: &mut $ty, single: bool, cap: $crate::regex::Captures)
                -> ::std::result::Result<(), Box<::std::error::Error>>
    {
      expand_style!($style($($style_args),*) => single, cap);
      Ok($fn_body)
    }
  };
  (pub $name:ident<$first_ty:ident $(,$tys:ident)*>: $ty:ty = $style:ident($($style_args:ident),*) $param_name:expr =>
   fn $fn_name:ident($this_name:ident)
   where $($where_tys:path : $where_clauses:path,)+
   $fn_body:block) =>
  {
    #[allow(non_snake_case)]
    let $name: $crate::ToolArg<$ty> = $crate::ToolArg {
      name: ::std::borrow::Cow::Borrowed(stringify!($name)),
      single: expand_style_single!($style($($style_args),*) => $param_name),
      split:  expand_style_split!($style($($style_args),*) => $param_name),
      action: Some(|this: &mut $ty, single: bool, cap: $crate::regex::Captures| {
        $fn_name(this, single, cap)
      }),
      help: None,
    };
    #[allow(unused_variables)]
    fn $fn_name<$first_ty $(,$tys)*>($this_name: &mut $first_ty, single: bool, cap: $crate::regex::Captures)
                                     -> ::std::result::Result<(), Box<::std::error::Error>>
      where $($where_tys : $where_clauses,)+
    {
      expand_style!($style($($style_args),*) => single, cap);
      Ok($fn_body)
    }
  };

  ($name:ident: $ty:ty = { $single_regex:expr, $split:expr };
   fn $fn_name:ident($this:ident, $single:ident, $cap:ident) $fn_body:block) => {
    lazy_static! {
      pub static ref $name: ::util::ToolArg<$ty> = {
        ::util::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: ($single_regex).map(|v: &str| From::from(v) ),
          split: ($split).map(|v: &str| From::from(v) ),
          help: None,
          action: Some($fn_name as util::ToolArgActionFn<$ty>),
        }
      };
    }

    fn $fn_name($this: &mut $ty, $single: bool, $cap: $crate::regex::Captures) ->
      ::std::result::Result<(), Box<Error>>
    {
      $fn_body
    }
  };
  ($name:ident: $ty:ty = { $single_regex:expr, $split:expr }) => {
    lazy_static! {
      pub static ref $name: ::util::ToolArg<$ty> = {
        ::util::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: ($single_regex).map(|v: &str| From::from(v) ),
          split: ($split).map(|v: &str| From::from(v) ),
          action: None,
          help: None,
        }
      };
    }
  }
);

#[macro_export] macro_rules! argument(
  (impl $name:ident where { Some($single:expr), None } for $this:ty {
    fn $fn_name:ident($this_name:ident, $single_name:ident, $cap_name:ident) $fn_body:block
  }) => (
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: Some(From::from($single)),
          split:  None,
          help: None,

          action: Some($fn_name as $crate::ToolArgActionFn<$this>),
        }
      };
    }
    #[allow(unreachable_code)]
    fn $fn_name($this_name: &mut $this, $single_name: bool, $cap_name: $crate::regex::Captures) ->
      ::std::result::Result<(), Box<Error>>
    {
      $fn_body;
      Ok(())
    }
  );
  (impl $name:ident where { None, Some($split:expr) } for $this:ty {
    fn $fn_name:ident($this_name:ident, $single_name:ident, $cap_name:ident) $fn_body:block
  }) => {
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: None,
          split: Some(From::from($split)),
          help: None,
          action: Some($fn_name as $crate::ToolArgActionFn<$this>),
        }
      };
    }
    #[allow(unreachable_code)]
    fn $fn_name($this_name: &mut $this, $single_name: bool, $cap_name: $crate::regex::Captures) ->
      ::std::result::Result<(), Box<Error>>
    {
      $fn_body;
      Ok(())
    }
  };
  (impl $name:ident where { Some($single:expr), Some($split:expr) } for $this:ty {
    fn $fn_name:ident($this_name:ident, $single_name:ident, $cap_name:ident) $fn_body:block
  }) => {
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: Some(From::from($single)),
          split: Some(From::from($split)),
          help: None,

          action: Some($fn_name as $crate::ToolArgActionFn<$this>),
        }
      };
    }
    #[allow(unreachable_code)]
    fn $fn_name($this_name: &mut $this, $single_name: bool, $cap_name: $crate::regex::Captures) ->
      ::std::result::Result<(), Box<Error>>
    {
      $fn_body;
      Ok(())
    }
  };



  (impl $name:ident where { Some($single:expr), None } for $this:ty => Some($fn_name:ident)) => {
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: Some(From::from($single)),
          split: None,
          help: None,
          action: Some($fn_name as $crate::ToolArgActionFn<$this>),
        }
      };
    }
  };
  (impl $name:ident where { None, Some($split:expr) } for $this:ty => Some($fn_name:ident)) => {
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: None,
          split: Some(From::from($split)),
          help: None,
          action: Some($fn_name as $crate::ToolArgActionFn<$this>),
        }
      };
    }
  };
  (impl $name:ident where { Some($single:expr), Some($split:expr) } for $this:ty => Some($fn_name:ident)) => {
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: Some(From::from($single)),
          split: Some(From::from($split)),
          help: None,

          action: Some($fn_name as $crate::ToolArgActionFn<$this>),
        }
      };
    }
  };


  (impl $name:ident where { Some($single:expr), None } for $this:ty => None) => {
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: Some(From::from($single)),
          split: None,
          help: None,
          action: None,
        }
      };
    }
  };
  (impl $name:ident where { None, Some($split:expr) } for $this:ty => None) => {
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: None,
          split: Some(From::from($split)),
          help: None,
          action: None,
        }
      };
    }
  };
  (impl $name:ident where { Some($single:expr), Some($split:expr) } for $this:ty => None) => {
    lazy_static! {
      pub static ref $name: $crate::ToolArg<$this> = {
        $crate::ToolArg {
          name: ::std::borrow::Cow::Borrowed(stringify!($name)),
          single: Some(From::from($single)),
          split: Some(From::from($split)),
          help: None,
          action: None,
        }
      };
    }
  };
);


pub mod filetype;
pub mod ldtools;
pub mod toolchain;
pub mod command_queue;
pub mod git;
pub mod repo;

pub trait CreateIfNotExists: Sized + AsRef<Path> {
  fn create_if_not_exists(self) -> std::io::Result<Self> {
    if !self.as_ref().exists() {
      std::fs::create_dir_all(&self)?;
    }

    Ok(self)
  }
}
impl CreateIfNotExists for PathBuf { }
impl<'a> CreateIfNotExists for &'a Path { }
impl<'a> CreateIfNotExists for &'a PathBuf { }

#[cfg(feature = "nacl")]
pub const SDK_VERSION: &'static str = include_str!(concat!(env!("OUT_DIR"),
                                                           "/REV"));
pub const CLANG_VERSION: &'static str = "5.0.0";

#[cfg(not(any(feature = "sdk", target_os = "nacl")))]
pub fn need_nacl_toolchain() -> PathBuf {
  use std::env::var_os;
  #[cfg(target_os = "linux")]
  fn host_os() -> &'static str { "linux" }
  #[cfg(target_os = "macos")]
  fn host_os() -> &'static str { "mac" }
  #[cfg(target_os = "windows")]
  fn host_os() -> &'static str { "win" }
  #[cfg(all(not(target_os = "linux"), not(target_os = "macos"), not(target_os = "windows")))]
  fn host_os() -> &'static str { unimplemented!() }

  match var_os("NACL_SDK_ROOT")
    .or_else(|| {
      option_env!("NACL_SDK_ROOT")
        .map(|f| From::from(f) )
    })
    {
      Some(sdk) => {
        let tc = format!("{}_pnacl", host_os());
        Path::new(&sdk)
          .join("toolchain")
          .join(&tc[..])
          .to_path_buf()
      },
      None => panic!("need `NACL_SDK_ROOT`"),
    }
}

#[cfg(all(feature = "nacl", not(target_os = "nacl")))]
pub fn need_nacl_toolchain() -> PathBuf {
  use std::env::current_exe;

  current_exe()
    .map(|p| p.join("..") )
    .unwrap()
}

#[cfg(test)]
pub fn get_bin_path<T: AsRef<Path>>(bin: T) -> PathBuf {
    assert!(bin.as_ref().is_relative());
    bin.as_ref().to_path_buf()
}
 #[cfg(all(target_os = "nacl", not(test)))]
pub fn get_bin_path<T: AsRef<Path>>(bin: T) -> PathBuf {
  use std::env::consts::EXE_SUFFIX;
  assert!(bin.as_ref().is_relative());
  let bin = format!("{}{}{}",
                    prefix,
                    bin.as_ref().display(),
                    EXE_SUFFIX);
  Path::new("/bin")
    .join(&bin[..])
    .to_path_buf()
}
#[cfg(all(not(target_os = "nacl"), not(test)))]
pub fn get_bin_path<T: AsRef<Path>>(bin: T) -> PathBuf {
  use std::env::consts::EXE_SUFFIX;

  assert!(bin.as_ref().is_relative());

  let mut toolchain = need_nacl_toolchain();
  toolchain.push("bin");

  let bin = format!("{}{}", bin.as_ref().display(),
                    EXE_SUFFIX);
  toolchain.push(&bin[..]);
  toolchain
}

#[cfg(not(target_os = "nacl"))]
pub fn add_gold_args(cmd: &mut process::Command) {
    #[cfg(windows)]
    const LIB_PATH: &'static str = "bin";
    #[cfg(not(windows))]
    const LIB_PATH: &'static str = "lib";

    let gold_plugin = need_nacl_toolchain()
        .join(LIB_PATH)
        .join(format!("LLVMgold{}", ::std::env::consts::DLL_SUFFIX));

    cmd.arg(format!("-plugin={}", gold_plugin.display()));
    cmd.arg("-plugin-opt=emit-llvm");
}

pub fn expect_next<'a, T>(args: &mut T) -> <T as Iterator>::Item
    where T: Iterator, <T as Iterator>::Item: AsRef<str> + PartialEq<&'a str>
{
    let arg = args.next();
    if arg.is_none() { panic!("expected another argument"); }
    arg.unwrap()
}

pub fn get_crate_root() -> PathBuf {
  const CRATE_ROOT: &'static str = env!("CARGO_MANIFEST_DIR");
  let pwd = Path::new(CRATE_ROOT)
    .join("../..");
  pwd.to_path_buf()
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum ArchSubtype {
    Linux,
    Mac,
    NonSFI,
}
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum Arch {
    Le32,
    X8632(Option<ArchSubtype>),
    X8664,
    AArch32(Option<ArchSubtype>),
    Mips32,
    Wasm32,
    Wasm64,
}

#[cfg(not(any(target_arch = "wasm32", target_arch = "wasm64")))]
impl Default for Arch {
    fn default() -> Arch {
        Arch::Wasm32
    }
}
#[cfg(target_arch = "wasm32")]
impl Default for Arch {
    fn default() -> Arch {
        Arch::Wasm32
    }
}
#[cfg(target_arch = "wasm64")]
impl Default for Arch {
    fn default() -> Arch {
        Arch::Wasm64
    }
}

lazy_static! {
  static ref ARCHS: Vec<(Arch, regex::Regex)> =
    vec![
      (Arch::X8632(None), regex::Regex::new(r"^([xX]86[-_]?32|i?[36]86|ia32)$").unwrap()),
      (Arch::X8632(Some(ArchSubtype::Linux)), regex::Regex::new(r"^x86-32-linux$").unwrap()),
      (Arch::X8632(Some(ArchSubtype::Mac)), regex::Regex::new(r"^x86-32-mac$").unwrap()),
      (Arch::X8632(Some(ArchSubtype::NonSFI)), regex::Regex::new(r"^x86-32-nonsfi$").unwrap()),
      (Arch::X8664, regex::Regex::new(r"^([xX]86[-_]?64|amd64)$").unwrap()),
      (Arch::AArch32(None), regex::Regex::new(r"^arm(v7a?)?$").unwrap()),
      (Arch::AArch32(Some(ArchSubtype::NonSFI)), regex::Regex::new(r"^arm-nonsfi$").unwrap()),
      (Arch::Mips32, regex::Regex::new(r"^mips(32|el)?$").unwrap()),
      (Arch::Le32, regex::Regex::new(r"^le32$").unwrap()),
      (Arch::Wasm32, regex::Regex::new(r"^wasm32$").unwrap()),
      (Arch::Wasm64, regex::Regex::new(r"^wasm64$").unwrap()),
    ];
}

impl Arch {
  pub fn parse_from_triple(triple: &str) -> Result<Arch, String> {
    let mut split = triple.split('-').peekable();

    fn check_triple_format<'a>(next: Option<&'a str>,
                               triple: &str)
      -> Result<&'a str, String>
    {
      if next.is_none() {
        return Err(format!("`{}` is an unknown target triple format",
                           triple));
      } else {
        return Ok(next.unwrap());
      }
    }

    let arch_str = check_triple_format(split.next(), triple.as_ref())?;
    let mut arch = None;
    for &(a, ref r) in ARCHS.iter() {
      if r.is_match(arch_str) {
        arch = Some(a);
        break;
      }
    }

    let arch = match arch {
      None => {
        return Err(format!("`{}` is an unknown target arch",
                           arch_str));
      },
      Some(arch) => arch,
    };

    macro_rules! unsupported_os(
            ($os:ident) => {
                return Err(format!("OS `{}` is not supported",
                                   $os));
            }
        );

    let env = check_triple_format(split.next(), triple.as_ref())?;
    let os = if split.peek().is_none() {
      env
    } else {
      check_triple_format(split.next(), triple.as_ref())?
    };
    let _format = if split.peek().is_some() {
      Some(check_triple_format(split.next(), triple.as_ref())?)
    } else {
      None
    };

    let nacl_or_wasm = os == "nacl" || (arch.is_wasm() && os == "unknown");
    if nacl_or_wasm && split.peek().is_none() {
      return Ok(arch);
    } else if !nacl_or_wasm && split.peek().is_none() {
      unsupported_os!(os);
    } else if nacl_or_wasm && split.peek().is_some() {
      check_triple_format(None, triple.as_ref())?;
      unreachable!();
    } else { panic!("unknown os: {}", os); }
  }

  pub fn llvm_arch(&self) -> Option<&'static str> {
    match self {
      &Arch::Wasm32 => Some("wasm32"),
      &Arch::Wasm64 => Some("wasm64"),
      &Arch::Le32   => Some("le32"),
      _ => None,
    }
  }

  pub fn bcld_output_format(&self) -> &'static str {
    match self {
      &Arch::Le32 | &Arch::X8632(None) |
      &Arch::X8632(Some(ArchSubtype::NonSFI)) => "elf32-i386-nacl",

      &Arch::AArch32(None) | &Arch::AArch32(Some(ArchSubtype::NonSFI)) =>
        "elf32-littlearm-nacl",

      &Arch::Mips32 => "elf32-tradlittlemips-nacl",
      &Arch::X8664 => "elf64-x86-64-nacl",

      _ => unimplemented!(),
    }
  }

  pub fn is_portable(&self) -> bool {
    match self {
      &Arch::Le32 |
      &Arch::Wasm32 |
      &Arch::Wasm64 => true,
      _ => false,
    }
  }
  pub fn is_wasm(&self) -> bool {
    match self {
      &Arch::Wasm32 |
      &Arch::Wasm64 => true,
      _ => false,
    }
  }

  pub fn bc_subpath(&self) -> &'static str {
    match self {
      &Arch::Le32 => "le32-nacl",
      &Arch::X8632(_) => "i686_bc-nacl",
      &Arch::X8664 => "x86_64_bc-nacl",
      &Arch::AArch32(_) => "arm_bc-nacl",
      _ => unreachable!(),
    }
  }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum OptimizationGoal {
  /// ie -O[0-3]
  Speed(u8),
  /// ie -Os
  Balanced,
  /// ie -Oz
  Size,
}
impl Default for OptimizationGoal {
  fn default() -> OptimizationGoal {
    OptimizationGoal::Speed(0)
  }
}
impl fmt::Display for OptimizationGoal {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      &OptimizationGoal::Speed(n) => write!(f, "-O{}", n),
      &OptimizationGoal::Balanced => write!(f, "-Os"),
      &OptimizationGoal::Size => write!(f, "-Oz"),
    }
  }
}

impl OptimizationGoal {
  pub fn parse(str: &str) -> Option<OptimizationGoal> {
    let o = match str {
      "" | "2" => OptimizationGoal::Speed(2),
      "0" => OptimizationGoal::Speed(0),
      "1" => OptimizationGoal::Speed(1),
      "3" => OptimizationGoal::Speed(3),
      "4" => OptimizationGoal::Speed(4),
      "s" => OptimizationGoal::Balanced,
      "z" => OptimizationGoal::Size,

      _ => { return None; },
    };
    Some(o)
  }

  pub fn check(&self) {
    match self {
      &OptimizationGoal::Speed(n) if n > 4 => {
        panic!("invalid optimization level");
      },
      _ => {},
    }
  }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum StripMode {
  All,
  Debug,
  None,
}

impl Default for StripMode {
  fn default() -> StripMode {
    StripMode::None
  }
}

impl fmt::Display for StripMode {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      &StripMode::None => Ok(()),
      &StripMode::Debug => write!(f, "-s"),
      &StripMode::All => write!(f, "-S"),
    }
  }
}


#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum EhMode {
  None,
  SjLj,
  Zerocost,
}

impl Default for EhMode {
  fn default() -> EhMode {
    EhMode::None
  }
}

impl EhMode {
  pub fn parse_arg(arg: &str) -> Option<Result<EhMode, String>> {
    const PNACL_EH: &'static str = "--pnacl-exceptions=";
    if arg.starts_with(PNACL_EH) {
      const NONE: &'static str = "none";
      const SJLJ: &'static str = "sjlj";
      const ZEROCOST: &'static str = "zerocost";
      let arg = &arg[PNACL_EH.len()..];
      if arg == NONE {
        return Some(Ok(EhMode::None));
      } else if arg == SJLJ {
        return Some(Ok(EhMode::SjLj));
      } else if arg == ZEROCOST {
        return Some(Ok(EhMode::Zerocost));
      } else {
        return Some(Err(format!("`{}` is an unknown eh handler",
                                arg)));
      }
    } else if arg == "--pnacl-allow-exceptions" {
      // TODO(mseaborn): Remove "--pnacl-allow-exceptions", which is
      // superseded by "--pnacl-exceptions".
      return Some(Ok(EhMode::Zerocost));
    } else {
      return None;
    }
  }
}

#[test]
fn eh_mode_test() {
  assert_eq!(EhMode::parse_arg("--something"), None);
  match EhMode::parse_arg("--pnacl-exceptions=notahandler") {
    Some(Err(_)) => {},
    _ => unreachable!(),
  }
  match EhMode::parse_arg("--pnacl-exceptions=blahnone") {
    Some(Err(_)) => {},
    _ => unreachable!(),
  }
  assert_eq!(EhMode::parse_arg("--pnacl-exceptions=none"),
             Some(Ok(EhMode::None)));
  assert_eq!(EhMode::parse_arg("--pnacl-exceptions=sjlj"),
             Some(Ok(EhMode::SjLj)));
  assert_eq!(EhMode::parse_arg("--pnacl-exceptions=zerocost"),
             Some(Ok(EhMode::Zerocost)));

  assert_eq!(EhMode::parse_arg("--pnacl-allow-exceptions"),
             Some(Ok(EhMode::Zerocost)));
}

pub fn boolean_env<K>(k: K) -> bool
  where K: AsRef<std::ffi::OsStr>,
{
  match std::env::var(k) {
    Ok(ref v) if v != "0" => true,
    _ => false,
  }
}
fn run_unlogged_cmd(task: &str, mut cmd: process::Command) {
  println!("({}): Running: {:?}", task, cmd);
  let mut child = cmd.spawn().unwrap();
  assert!(child.wait().unwrap().success(), "{:?}", cmd);
}

/// A function to call if the associated regex was a match. Return `Err` if
/// there was an error parsing the captured regex.
/// The second param indicates whether the argument matched the single or split
/// forms. True for single.
pub type ToolArgActionFn<This> = fn(&mut This, bool, regex::Captures) -> Result<(), Box<dyn Error>>;

pub type ToolArgAction<This> = Option<ToolArgActionFn<This>>;

pub struct ToolArg<This: ?Sized> {
  pub name: Cow<'static, str>,
  pub single: Option<Cow<'static, str>>,
  pub split: Option<Cow<'static, str>>, // Note there is no way to match on the next arg.

  pub help: Option<Cow<'static, str>>,

  pub action: ToolArgAction<This>,
}

pub struct InitedToolArg<This: ?Sized> {
  pub single: Option<regex::Regex>,
  pub split: Option<regex::Regex>,

  pub action: ToolArgAction<This>,
}

impl<'a, This> From<&'a ToolArg<This>> for InitedToolArg<This>
  where This: ?Sized,
{
  fn from(v: &'a ToolArg<This>) -> InitedToolArg<This> {
    let name = v.name.as_ref();
    let single = v.single.as_ref();
    let split = v.split.as_ref();
    let action = v.action;

    InitedToolArg {
      single: single.map(|v| {
        regex::Regex::new(v.as_ref())
          .unwrap_or_else(|e| {
            panic!("Invalid regex in argument {}: {:?}", name, e);
          })
      }),
      split: split.map(|v| {
        regex::Regex::new(v.as_ref())
          .unwrap_or_else(|e| {
            panic!("Invalid regex in argument {}: {:?}", name, e);
          })
      }),
      action,
    }
  }
}

pub trait ToolArgAccessor<This, TArg> {
  //fn name_modifier() -> Option<Cow<'static, str>> { None }
  fn access<'a>(this: &'a mut This) -> &'a mut TArg;
}

impl<This> fmt::Debug for ToolArg<This>
  where This: ?Sized,
{
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let action_msg = if self.action.is_some() {
      "Some(..)"
    } else {
      "None"
    };
    write!(f, "ToolArg {{ single: `{:?}`, split: `{:?}`, action: `{}` }}",
           self.single, self.split, action_msg)
  }
}
impl<This> Clone for ToolArg<This>
  where This: ?Sized,
{
  fn clone(&self) -> Self {
    ToolArg {
      name: self.name.clone(),
      single: self.single.clone(),
      split: self.split.clone(),
      help: self.help.clone(),
      action: self.action,
    }
  }
}

impl<This> InitedToolArg<This>
  where This: ?Sized,
{
  pub fn check<'a, T>(&self,
                      this: &mut This,
                      args: &mut Peekable<T>,
                      count: &mut usize) -> Option<Result<(), Box<dyn Error>>>
  // Some(Ok(<number of args used>))
    where T: Iterator,
          <T as Iterator>::Item: AsRef<str> + PartialEq<&'a str>
  {
    *count = 0;
    let res = {
      let first_arg = match args.peek() {
        Some(arg) => arg.as_ref().to_string(),
        None => { return None; },
      };
      if self.action.is_none() {
        if self.single.is_some() &&
          self.single.as_ref().unwrap().is_match(first_arg.as_ref()) {
          Some(Ok(()))
        } else if self.split.as_ref().map(|r| r.is_match(first_arg.as_ref()) ).unwrap_or(false) {
          assert!(args.next().is_some());
          if args.peek().is_none() {
            let msg = format!("`{}` expects another argument",
                              self.split.as_ref().unwrap());
            Some(Err(From::from(msg)))
          } else {
            *count += 1;
            Some(Ok(()))
          }
        } else {
          None
        }
      } else {
        let action = self.action.unwrap();
        let match_ = self.single
          .as_ref()
          .and_then(|s| s.captures(first_arg.as_ref()) )
          .map(|capture| {
            action(this, true, capture)
          });
        if match_.is_some() {
          match_
        } else if self.split.as_ref().map(|r| r.is_match(first_arg.as_ref()) ).unwrap_or(false) {
          // This is so we can capture the next arg:
          lazy_static! {
              static ref SECOND_ARG: regex::Regex = regex::Regex::new("(.+)").unwrap();
            };
          assert!(args.next().is_some());

          if args.peek().is_none() {
            let msg = format!("`{}` expects another argument",
                              self.split.as_ref().unwrap());
            Some(Err(From::from(msg)))
          } else {
            let cap = SECOND_ARG.captures(args.peek().unwrap().as_ref())
              .unwrap();
            let action_result = action(this, false, cap);
            *count += 1;
            Some(action_result)
          }
        } else {
          None
        }
      }
    };

    if res.is_some() {
      assert!(args.next().is_some());
      *count += 1;
    }

    res
  }
}

// This is an array of arrays so multiple global arg arrays can be glued together.
pub type ToolArgs<This> = Cow<'static, [ToolArg<This>]>;

pub trait Tool: fmt::Debug {
  fn enqueue_commands(&mut self, queue: &mut CommandQueue<Self>) -> Result<(), Box<dyn Error>>
    where Self: Sized;

  fn get_name(&self) -> String;

  fn add_tool_input(&mut self, input: PathBuf) -> Result<(), Box<dyn Error>>;

  fn get_output(&self) -> Option<&PathBuf>;
  /// Unconditionally set the output file.
  fn override_output(&mut self, out: PathBuf);
}

/// Tool argument processing.
pub trait ToolInvocation: Tool + Default {
  fn check_state(&mut self, iteration: usize, skip_inputs_check: bool) -> Result<(), Box<dyn Error>>;

  /// Called until `None` is returned. Put args that override errors before
  /// the the args that can have those errors
  fn args(&self, iteration: usize) -> Option<ToolArgs<Self>>;
}

pub fn process_invocation_args<T>(invocation: &mut T,
                                  args: Vec<String>,
                                  skip_inputs_check: bool)
  -> Result<(), Box<dyn Error>>
  where T: ToolInvocation + 'static,
{
  use std::collections::BTreeMap;
  use std::io::{Cursor, };
  use std::ops::RangeFull;

  let mut program_args: BTreeMap<usize, String> = args
    .into_iter()
    .enumerate()
    .collect();

  let mut iteration = 0;
  let mut used: Vec<usize> = Vec::new();
  'main: loop {
    let next_args = invocation.args(iteration);

    debug_assert!(iteration != 0 || next_args.is_some());

    if next_args.is_none() { break; }
    let next_args = next_args.unwrap();
    let next_args: Vec<InitedToolArg<_>> = next_args
      .into_iter()
      .map(|v| v.into() )
      .collect();

    //println!("iteration `{}`", iteration);

    // (the argument that caused the error, the error msg)
    let mut errors: Vec<(String, Box<dyn Error>)> = Default::default();

    {
      let mut program_arg_id = 0;
      let mut program_args_iter = program_args.iter()
        .map(|(_, arg)| arg )
        .peekable();
      'outer: loop {
        if program_args_iter.peek().is_none() {
          break 'outer;
        }
        if !program_args.contains_key(&program_arg_id) {
          // XXX this is bad: `program_args` is a sorted
          // collection...
          program_arg_id += 1;
          continue 'outer;
        }
        let current_arg = program_args_iter
          .peek()
          .unwrap()
          .to_string();
        //println!("current_arg: {}", current_arg);
        'inner: for accepted_arg in next_args.iter() {

          let mut args_used = 0;

          let check = accepted_arg.check(invocation,
                                         &mut program_args_iter,
                                         &mut args_used);
          match check {
            None => { },
            Some(res) => {
              //println!("checking: {:?}", accepted_arg);
              debug_assert!(args_used != 0);
              loop {
                if args_used == 0 { break; }

                //println!("matched: {:?}", program_args.get(&program_arg_id));
                used.push(program_arg_id);

                program_arg_id += 1;
                args_used -= 1;
              }

              if let Err(msg) = res {
                errors.push((current_arg, msg));
                break;
              }

              continue 'outer;
            },
          }
        }

        program_args_iter.next();
        program_arg_id += 1;
      }
    }

    let mut errors_out = Cursor::new(Vec::new());
    let had_errors = errors.len() != 0;
    for (arg, msg) in errors.into_iter() {
      writeln!(errors_out,
               "error on argument `{}`: `{}`",
               arg, msg)
        .unwrap();
    }

    if had_errors {
      let errors_str = unsafe {
        String::from_utf8_unchecked(errors_out.into_inner())
      };
      Err(errors_str)?;
    }

    invocation.check_state(iteration, skip_inputs_check)?;

    for used in used.drain(RangeFull) {
      program_args.remove(&used);
    }

    iteration += 1;
  }

  Ok(())
}

pub fn main_inner<T>(invocation: Option<T>) -> Result<T, CommandQueueError>
    where T: ToolInvocation + 'static,
{
  use std::env;

  let mut verbose = false;
  let mut no_op   = false;

  let args: Vec<String> = {
    let mut i = env::args();
    i.next();
    i.filter(|arg| {
      match &arg[..] {
        "--pnacl-driver-verbose" |
        "--wasm-driver-verbose" => {
          verbose = true;
          false
        },
        "--dry-run" => {
          no_op = true;
          false
        },
        _ => true,
      }
    })
      .collect()
  };

  let process_args = invocation.is_none();
  let mut invocation: T = invocation.unwrap_or_default();
  if process_args {
    process_invocation_args(&mut invocation, args, false)?;
  }

  let output = invocation.get_output()
    .map(|out| out.clone() );
  let mut commands = CommandQueue::new(output);
  commands.set_verbose(verbose);
  commands.set_dry_run(no_op);
  invocation.enqueue_commands(&mut commands)?;

  commands.run_all(&mut invocation)
    .map(move |_| {
      invocation
    })
}

pub fn main<T>(outs: Option<(&mut dyn Write, &mut dyn Write)>)
  -> Result<(), i32>
  where T: ToolInvocation + 'static,
{
  use std::io::{stdout, stderr};
  use std::panic::catch_unwind;

  #[cfg(test)]
  fn test_safe_exit(code: i32) -> Result<(), i32> {
    Err(code)
  }
  #[cfg(not(test))]
  fn test_safe_exit(code: i32) -> ! {
    ::std::process::exit(code);
  }

  let mut stdout = stdout();
  let mut stderr = stderr();

  let (_, err) = outs.unwrap_or((&mut stdout, &mut stderr));

  match catch_unwind(move || {
    main_inner(None::<T>)?;
    Ok(())
  }) {
    Ok(Err(CommandQueueError::Error(msg))) => {
      write!(err, "{}\n", msg)
        .unwrap();

      test_safe_exit(1)
    },
    Ok(Err(CommandQueueError::ProcessError(code))) => {
      if let Some(code) = code {
        test_safe_exit(code)
      } else {
        test_safe_exit(1)
      }
    }
    Ok(Ok(ok)) => Ok(ok),
    Err(..) => {
      writeln!(err, "Woa! It looks like something bad happened! :(")
        .unwrap();
      writeln!(err, "Please let us know by filling a bug at https://github.com/DiamondLovesYou/pnacl-clang-driver")
        .unwrap();

      test_safe_exit(127)
    },
  }
}

#[test]
fn main_crash_test() {
  use std::io::{self, set_panic, Cursor};
  use std::sync::{Arc, Mutex};

  #[derive(Debug)]
  struct Panic;

  impl Default for Panic {
    fn default() -> Panic { Panic }
  }

  impl Tool for Panic {
    fn enqueue_commands(&mut self, queue: &mut CommandQueue<Self>) -> Result<(), String> { unimplemented!() }

    fn get_name(&self) -> String { unimplemented!() }

    fn add_tool_input(&mut self, _: PathBuf) -> Result<(), Box<Error>> { Ok(()) }

    fn get_output(&self) -> Option<&PathBuf> { unimplemented!() }
    fn override_output(&mut self, out: PathBuf)  { unimplemented!() }
  }

  /// Tool argument processing.
  impl ToolInvocation for Panic {
    fn check_state(&mut self, iteration: usize, _skip_inputs_check: bool) -> Result<(), String> { unimplemented!() }

    /// Called until `None` is returned. Put args that override errors before
        /// the the args that can have those errors.
    fn args(&self, iteration: usize) -> Option<ToolArgs<Self>> { unimplemented!() }
  }

  struct Sink(Arc<Mutex<Cursor<Vec<u8>>>>);
  impl io::Write for Sink {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
      io::Write::write(&mut *self.0.lock().unwrap(), data)
    }
    fn flush(&mut self) -> io::Result<()> {
      io::Write::flush(&mut *self.0.lock().unwrap())
    }
  }

  let out = Arc::new(Mutex::new(Cursor::new(Vec::new())));
  let err = Arc::new(Mutex::new(Cursor::new(Vec::new())));


  {
    let mut out = Sink(out.clone());
    let mut err = Sink(err.clone());
    assert_eq!(main::<Panic>(Some((&mut out, &mut err))), Err(127));
  }
  let stderr = err.lock().unwrap().get_ref().clone();
  let str = String::from_utf8(stderr).unwrap();
  println!("{}", str);
  assert!(str.contains("crbug"));
}

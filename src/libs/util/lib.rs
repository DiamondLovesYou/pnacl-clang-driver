#![feature(plugin)]
#![feature(drain)]
#![feature(trace_macros)]
#![feature(rustc_private)]
#![feature(path_ext)]
#![feature(box_syntax)]
#![feature(catch_panic)]
#![cfg_attr(test, feature(set_stdio))]

#![plugin(regex_macros)]

use std::fmt;
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::process;

extern crate regex;
extern crate rustc_llvm as llvm;
extern crate elf;

#[macro_use]
extern crate maplit;

pub mod filetype;
pub mod ldtools;

pub const SDK_VERSION: &'static str = include_str!(concat!(env!("OUT_DIR"),
                                                           "/REV"));
pub const CLANG_VERSION: &'static str = "3.7.0";

#[cfg(not(any(feature = "sdk", target_os = "nacl")))]
pub fn need_nacl_toolchain() -> PathBuf {
    use std::env::var_os;
    #[cfg(target_os = "linux")]
    fn host_os() -> &'static str { "linux" }
    #[cfg(target_os = "macos")]
    fn host_os() -> &'static str { "mac" }
    #[cfg(target_os = "windows")]
    fn host_os() -> &'static str { "win" }
    #[cfg(all(not(target_os = "linux"),
              not(target_os = "macos"),
              not(target_os = "windows")))]
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

#[cfg(all(feature = "sdk", not(target_os = "nacl")))]
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
}

impl Default for Arch {
    fn default() -> Arch {
        Arch::Le32
    }
}

static ARCHS: &'static [(Arch, regex::Regex)] =
    &[(Arch::X8632(None),
       regex!(r"^([xX]86[-_]?32|i?[36]86|ia32)$")),
      (Arch::X8632(Some(ArchSubtype::Linux)),
       regex!(r"^x86-32-linux$")),
      (Arch::X8632(Some(ArchSubtype::Mac)),
       regex!(r"^x86-32-mac$")),
      (Arch::X8632(Some(ArchSubtype::NonSFI)),
       regex!(r"^x86-32-nonsfi$")),
      (Arch::X8664,
       regex!(r"^([xX]86[-_]?64|amd64)$")),
      (Arch::AArch32(None),
       regex!(r"^arm(v7a?)?$")),
      (Arch::AArch32(Some(ArchSubtype::NonSFI)),
       regex!(r"^arm-nonsfi$")),
      (Arch::Mips32,
       regex!(r"^mips(32|el)?$")),
      (Arch::Le32,
       regex!(r"^le32$")),
      ];

impl Arch {
    pub fn parse_from_triple(triple: &str) ->
        Result<Arch, String>
    {
        let mut split = triple.split('-').peekable();

        fn check_triple_format<'a>(next: Option<&'a str>, triple: &str) ->
            Result<&'a str, String>
        {
            if next.is_none() {
                return Err(format!("`{}` is an unknown target triple format",
                                   triple));
            } else {
                return Ok(next.unwrap());
            }
        }

        let arch_str = try!(check_triple_format(split.next(), triple.as_ref()));
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

        let os = try!(check_triple_format(split.next(), triple.as_ref()));
        if os == "nacl" && split.peek().is_none() {
            return Ok(arch);
        } else if os != "nacl" && split.peek().is_none() {
            unsupported_os!(os);
        } else if os == "nacl" && split.peek().is_some() {
            try!(check_triple_format(None, triple.as_ref()));
        }

        let os = try!(check_triple_format(split.next(), triple.as_ref()));
        if os == "nacl" && split.peek().is_none() {
            return Ok(arch);
        } else if os != "nacl" && split.peek().is_none() {
            unsupported_os!(os);
        } else if os == "nacl" && split.peek().is_some() {
            try!(check_triple_format(None, triple.as_ref()));
            unreachable!();
        } else { unreachable!(); }
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
            &Arch::Le32 => true,
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

#[derive(Debug)]
pub enum CommandKind {
    /// if Some(..), its value will be the argument used. The output will be
    /// written to a random temp folder && added to the next command's
    /// arguments.
    /// ie Some("-o")
    External(process::Command, Option<&'static str>),
    Tool(Box<Tool>),
}

#[derive(Debug)]
pub struct Command {
    pub name: Option<String>,
    pub cmd: CommandKind,
    /// should we print the command we just tried to run if it exits with a non-zero status?
    pub cant_fail: bool,
}

pub struct CommandQueue {
    pub final_output: Option<PathBuf>,

    queue: Vec<Command>,
    verbose: bool,
    dry_run: bool,
}

impl CommandQueue {
    pub fn new(final_output: Option<PathBuf>) -> CommandQueue {
        CommandQueue {
            final_output: final_output,

            queue: Default::default(),
            verbose: false,
            dry_run: false,
        }
    }
    pub fn set_verbose(&mut self, v: bool) {
        self.verbose = v;
    }
    pub fn set_dry_run(&mut self, v: bool) {
        self.dry_run = v;
    }

    pub fn enqueue_external(&mut self, name: Option<&'static str>,
                            mut cmd: process::Command,
                            output_arg: Option<&'static str>,
                            cant_fail: bool) {
        use std::process::{Stdio};

        cmd.stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::inherit());

        let kind = CommandKind::External(cmd, output_arg);
        let command = Command {
            name: name.map(|v| v.to_string() ),
            cmd: kind,
            cant_fail: cant_fail,
        };

        self.queue.push(command);
    }

    pub fn enqueue_tool<T: ToolInvocation + 'static>(&mut self, name: Option<String>,
                                                     mut invocation: T, args: Vec<String>,
                                                     cant_fail: bool) ->
        Result<(), String>
    {
        try!(process_invocation_args(&mut invocation, args));

        let kind = CommandKind::Tool(box invocation as Box<Tool>);
        let command = Command {
            name: name,
            cmd: kind,
            cant_fail: cant_fail,
        };

        self.queue.push(command);

        Ok(())
    }

    pub fn run_all(&mut self) -> Result<(), String> {
        unimplemented!()
    }
}

/// A function to call if the associated regex was a match. Return `Err` if
/// there was an error parsing the captured regex.
pub type ToolArgActionFn<This> = fn(&mut This, regex::Captures) ->
    Result<(), String>;
pub type ToolArgAction<This> = Option<ToolArgActionFn<This>>;

pub struct ToolArg<This> {
    pub single: Option<regex::Regex>,
    pub split: Option<&'static [regex::Regex]>, // Note there is no way to match on the next arg.

    pub action: ToolArgAction<This>,
}
impl<This> ToolArg<This> {
    pub fn check<'a, T>(&self,
                        this: &mut This,
                        args: &mut Peekable<T>,
                        count: &mut usize) ->
        // Some(Ok(<number of args used>))
        Option<Result<(), String>>
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
                    self.single.as_ref().unwrap().is_match(first_arg.as_ref())
                {
                    Some(Ok(()))
                } else if self.split.is_some() &&
                    self.split.unwrap().iter().any(|r| r.is_match(first_arg.as_ref()) )
                {
                    assert!(args.next().is_some());
                    if args.peek().is_none() {
                        Some(Err(format!("`{}` expects another argument",
                                         self.split.unwrap().iter().find(|r| {
                                             r.is_match(first_arg.as_ref())
                                         }).unwrap())))
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
                        action(this, capture)
                    });
                if match_.is_some() {
                    match_
                } else if self.split.is_some() &&
                    self.split.unwrap().iter().any(|r| r.is_match(first_arg.as_ref()) )
                {
                    // This is so we can capture the next arg:
                    static SECOND_ARG: regex::Regex = regex!("(.+)");
                    assert!(args.next().is_some());

                    if args.peek().is_none() {
                        Some(Err(format!("`{}` expects another argument",
                                         self.split.as_ref().unwrap().iter().find(|r| {
                                             r.is_match(first_arg.as_ref())
                                         }).unwrap())))
                    } else {
                        let cap = SECOND_ARG.captures(args.peek().unwrap().as_ref())
                            .unwrap();
                        let action_result = action(this, cap);
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
pub type ToolArgs<This> = &'static [&'static [&'static ToolArg<This>]];

#[macro_export] macro_rules! tool_argument(
    ($name:ident: $ty:ty = { $single_regex:expr, $split:expr };
      fn $fn_name:ident($this:ident, $cap:ident) $fn_body:block) => {
        static $name: ::util::ToolArg<$ty> = ::util::ToolArg {
            single: Some(regex!($single_regex)),
            split: $split,
            action: Some($fn_name as util::ToolArgActionFn<$ty>),
        };
        fn $fn_name($this: &mut $ty, $cap: ::regex::Captures) ->
            ::std::result::Result<(), ::std::string::String>
        {
            $fn_body
        }
    };
    ($name:ident: $ty:ty = { $single_regex:expr, $split:expr }) => {
        static $name: ::util::ToolArg<$ty> = ::util::ToolArg {
            single: Some(regex!($single_regex)),
            split: $split,
            action: None,
        };
    }
);

pub trait Tool: fmt::Debug {
    fn enqueue_commands(&mut self, queue: &mut CommandQueue) -> Result<(), String>;

    fn get_name(&self) -> String;

    fn get_output(&self) -> Option<&PathBuf>;
    /// Unconditionally set the output file.
    fn override_output(&mut self, out: PathBuf);
}

/// Tool argument processing.
pub trait ToolInvocation: Tool + Default {
    fn check_state(&mut self, iteration: usize) -> Result<(), String>;

    /// Called until `None` is returned. Put args that override errors before
    /// the the args that can have those errors.
    fn args(&self, iteration: usize) -> Option<ToolArgs<Self>>;
}

pub fn process_invocation_args<T: ToolInvocation + 'static>(invocation: &mut T,
                                                            args: Vec<String>) ->
    Result<(), String>
{
    use std::collections::BTreeMap;
    use std::io::{Write, Cursor};
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

        // (the argument that caused the error, the error msg)
        let mut errors: Vec<(String, String)> = Default::default();

        {
            let mut program_arg_id = 0;
            let mut program_args_iter = program_args.iter()
                .map(|(_, arg)| arg )
                .peekable();

            'outer: for args in next_args.iter() {
                for accepted_arg in args.iter() {
                    if program_args_iter.peek().is_none() { break 'outer; }
                    let mut args_used = 0;
                    let current_arg = program_args_iter.peek().unwrap().to_string();
                    let check = accepted_arg.check(invocation,
                                                   &mut program_args_iter,
                                                   &mut args_used);
                    match check {
                        None => {
                            program_arg_id += 1;
                        },
                        Some(res) => {
                            debug_assert!(args_used != 0);
                            loop {
                                if args_used == 0 { break; }

                                used.push(program_arg_id);

                                program_arg_id += 1;
                                args_used -= 1;
                            }

                            if let Err(msg) = res {
                                errors.push((current_arg, msg));
                            }
                        },
                    }
                }
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
            return Err(errors_str);
        }

        try!(invocation.check_state(iteration));

        for used in used.drain(RangeFull) {
            program_args.remove(&used);
        }

        iteration += 1;
    }

    // TODO(rdiamond): unused args?

    Ok(())
}

pub fn main_inner<T: ToolInvocation + 'static>() -> Result<(), String> {
    use std::env;

    let mut verbose = false;
    let mut no_op   = false;

    let args: Vec<String> = {
        let mut i = env::args();
        i.next();
        i.filter(|arg| {
            match &arg[..] {
                "--pnacl-driver-verbose" => {
                    verbose = true;
                    true
                },
                "--dry-run" => {
                    no_op = true;
                    true
                },
                _ => false,
            }
        })
            .collect()
    };

    let mut invocation: T = Default::default();

    try!(process_invocation_args(&mut invocation, args));

    let output = invocation.get_output()
        .map(|out| out.clone() );
    let mut commands = CommandQueue::new(output);
    commands.set_verbose(verbose);
    commands.set_dry_run(no_op);
    invocation.enqueue_commands(&mut commands)
        .unwrap();

    commands.run_all()
}

pub fn main<T: ToolInvocation + 'static>() -> Result<(), i32> {
    use std::io::{stdout, Write};
    use std::thread::catch_panic;

    #[cfg(test)]
    fn test_safe_exit(code: i32) -> Result<(), i32> {
        Err(code)
    }
    #[cfg(not(test))]
    fn test_safe_exit(code: i32) -> ! {
        ::std::process::exit(code);
    }

    match catch_panic(main_inner::<T>) {
        Ok(Err(msg)) => {
            write!(stdout(),
                   "{}", msg)
                .unwrap();
            if !msg.ends_with("\n") {
                writeln!(stdout(), "").unwrap();
            }

            test_safe_exit(1)
        },
        Ok(Ok(ok)) => Ok(ok),
        Err(..) => {
            println!("Woa! It looks like something bad happened! :(");
            println!("Please let us know by filling a bug at https://crbug.com");

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
        fn enqueue_commands(&mut self, queue: &mut CommandQueue) -> Result<(), String> { unimplemented!() }

        fn get_name(&self) -> String { unimplemented!() }

        fn get_output(&self) -> Option<&PathBuf> { unimplemented!() }
        fn override_output(&mut self, out: PathBuf)  { unimplemented!() }
    }

    /// Tool argument processing.
    impl ToolInvocation for Panic {
        fn check_state(&mut self, iteration: usize) -> Result<(), String> { unimplemented!() }

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

    let sink = Arc::new(Mutex::new(Cursor::new(Vec::new())));
    io::set_print(box Sink(sink.clone()));
    assert_eq!(main::<Panic>(), Err(127));
    let stderr = sink.lock().unwrap().get_ref().clone();
    let str = String::from_utf8(stderr).unwrap();
    println!("{}", str);
    assert!(str.contains("crbug"));
}

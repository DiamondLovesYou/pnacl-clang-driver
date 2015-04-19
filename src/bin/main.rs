#![crate_name = "driver_clang"]

#![allow(dead_code)]

use std::borrow::ToOwned;
use std::default::Default;
use std::env;
use std::env::{var_os};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

enum EhMode {
    Off,
    SjLj,
}

impl Default for EhMode {
    fn default() -> EhMode {
        EhMode::Off
    }
}

#[allow(dead_code)]
fn need_nacl_toolchain() -> PathBuf {
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

#[cfg(test)]
fn get_bin_path<T: AsRef<Path>>(bin: T) -> PathBuf {
    assert!(bin.as_ref().is_relative());
    bin.as_ref().to_path_buf()
}
#[cfg(all(target_os = "nacl", not(test)))]
fn get_bin_path<T: AsRef<Path>>(bin: T) -> PathBuf {
    use std::env::consts::EXE_SUFFIX;
    assert!(bin.as_ref().is_relative());
    let prefix = if bin.as_ref().starts_with("clang") {
        "real-"
    } else {
        ""
    };
    let bin = format!("{}{}{}",
                      prefix,
                      bin.as_ref().display(),
                      EXE_SUFFIX);
    Path::new("/bin")
        .join(&bin[..])
        .to_path_buf()
}
#[cfg(all(not(target_os = "nacl"), not(test)))]
fn get_bin_path<T: AsRef<Path>>(bin: T) -> PathBuf {
    use std::env::consts::EXE_SUFFIX;

    assert!(bin.as_ref().is_relative());

    let mut toolchain = need_nacl_toolchain();
    toolchain.push("bin");

    let bin = format!("{}{}", bin.as_ref().display(),
                      EXE_SUFFIX);
    toolchain.push(&bin[..]);
    toolchain
}

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

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
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

        assert!(cer.ends_with(EXE_SUFFIX));

        let cer = &cer[..cer.len() - EXE_SUFFIX.len()];

        if cer.ends_with("clang") {
            DriverMode::CC
        } else if cer.ends_with("clang++") {
            DriverMode::CXX
        } else {
            panic!("unknown driver mode!");
        }
    }


    fn get_clang_name(&self) -> PathBuf {
        let bin = match self {
            &DriverMode::CC => "clang",
            &DriverMode::CXX => "clang++",
        };
        get_bin_path(bin)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum GccMode {
    Dashc,
    DashE,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
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

struct Invocation {
    driver_mode: DriverMode,
    gcc_mode: Option<GccMode>,
    eh_mode: EhMode,

    opt_level: u8,

    no_default_libs: bool,
    no_std_lib: bool,
    no_default_std_inc: bool,
    no_default_std_incxx: bool,

    inputs: Vec<(PathBuf, Option<FileLang>)>,
    header_inputs: Vec<PathBuf>,

    linker_args: Vec<String>,
    driver_args: Vec<String>,

    output: Option<PathBuf>,

    verbose: bool,
    run_queue: Vec<Command>,
}

impl Invocation {
    fn new() -> Invocation {
        Invocation::new_driver(DriverMode::new())
    }
    fn new_driver(mode: DriverMode) -> Invocation {
        Invocation {
            driver_mode: mode,
            gcc_mode: Default::default(),
            eh_mode: Default::default(),

            opt_level: 0,

            no_default_libs: false,
            no_std_lib: false,
            no_default_std_inc: false,
            no_default_std_incxx: false,

            inputs: Default::default(),
            header_inputs: Default::default(),

            linker_args: Default::default(),
            driver_args: Default::default(),

            output: Default::default(),

            verbose: false,
            run_queue: Default::default(),
        }
    }

    fn print_version(&self) {
        use std::process::Stdio;
        let mut clang_ver = self.clang_base_cmd();
        clang_ver.stdout(Stdio::piped());
        self.clang_add_std_args(&mut clang_ver);
        clang_ver.arg("--version");

        if self.verbose {
            println!("running `{:?}`:", clang_ver);
        }

        let out = clang_ver.output().unwrap();

        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut add_nacl_version = Some(include_str!(concat!(env!("OUT_DIR"), "/REV")));
        for line in stdout
            .lines_any()
            .map(|l| {
                match add_nacl_version.take() {
                    Some(rev) => format!("{}{}", l, rev),
                    None => l.to_string(),
                }
            })
        {
            println!("{}", line);
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
        if !self.no_default_std_inc {
            if !self.no_default_std_incxx {
                let cxx_inc = get_inc_path()
                    .join("c++")
                    .join("v1")
                    .to_path_buf();
                isystem.push(cxx_inc);
            }
            let c_inc = get_inc_path();
            isystem.push(c_inc);
            for clang_ver in ["3.5.0", "3.6.0"].iter() {
                let c_inc = get_inc_path()
                    .join("..")
                    .join("..")
                    .join("lib")
                    .join("clang")
                    .join(clang_ver)
                    .join("include");
                isystem.push(c_inc);
            }
        }
        isystem
            .into_iter()
            .map(|p| format!("-isystem{}", p.display()) )
            .collect()
    }

    fn get_default_lib_args(&self) -> Vec<String> {
        if self.no_default_libs {
            vec![]
        } else {
            let mut libs = Vec::new();
            libs.push("-L/lib".to_string());
            libs.push("--start-group".to_string());
            libs.push("-lc++".to_string());
            libs
        };
        unimplemented!();
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
        let mut cmd = Command::new(self.driver_mode.get_clang_name());
        cmd.stdin(Stdio::inherit());
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        //Invocation::set_ld_library_path(&mut cmd);

        cmd
    }

    fn clang_add_std_args(&self, cmd: &mut Command) {
        assert!(self.opt_level <= 3);
        cmd.arg(format!("-O{}", self.opt_level));
        cmd.args(&["-fno-vectorize",
                   "-fno-slp-vectorize",
                   "-fno-common",
                   "-pthread",
                   "-nostdinc",
                   "-Dasm=ASM_FORBIDDEN",
                   "-D__asm__=ASM_FORBIDDEN",
                   "-target", "le32-unknown-nacl"]);
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
    fn clang_add_output_args(&self, cmd: &mut Command) {
        let out = self.get_output();
        cmd.arg("-o");
        cmd.arg(out);
    }

    fn queue_clang(&mut self) {
        // build the cmd:
        if !self.is_pch_mode() {
            let mut cmd = self.clang_base_cmd();
            self.clang_add_std_args(&mut cmd);
            self.clang_add_input_args(&mut cmd);
            self.clang_add_output_args(&mut cmd);
            self.run_queue.push(cmd);
        } else {
            let header_inputs = self.header_inputs.clone();
            let output = self.output.clone();
            if header_inputs.len() != 1 &&
                output.is_some()
            {
                panic!("cannot have -o <out> with multiple header file inputs");
            }

            // TODO: what if `-` is provided?
            for input in header_inputs.into_iter() {
                let mut cmd = self.clang_base_cmd();
                self.clang_add_std_args(&mut cmd);

                match output {
                    Some(ref file) => {
                        cmd.arg("-o");
                        cmd.arg(file);
                    },
                    None => {},
                }
                cmd.arg(input);

                self.run_queue.push(cmd);
            }
        }
    }

    fn queue_ld(&mut self) {
        unimplemented!()
    }

    fn queue_all(&mut self) {

        self.queue_clang();

        if self.should_link_output() {
            self.queue_ld();
        }
    }

    fn run_all(&mut self) {
        use std::mem::swap;
        let mut run_queue = Vec::new();
        swap(&mut self.run_queue, &mut run_queue);
        for mut cmd in run_queue.into_iter() {
            if self.verbose {
                println!("running `{:?}`:", cmd);
            }
            let result = cmd.status().unwrap();
            if !result.success() { panic!() }
        }
    }


    fn process_args<'a, T>(&mut self, mut raw_args: T) -> bool
        where T: Iterator, <T as Iterator>::Item: AsRef<str> + PartialEq<&'a str>,
    {

        fn expect_next<'a, T>(args: &mut T) -> <T as Iterator>::Item
            where T: Iterator, <T as Iterator>::Item: AsRef<str> + PartialEq<&'a str>
        {
            let arg = args.next();
            if arg.is_none() { panic!("expected another argument"); }
            arg.unwrap()
        }

        let mut file_lang;

        loop {
            let arg_anchor = raw_args.next();
            if arg_anchor.is_none() { break; }
            let arg_anchor = arg_anchor.unwrap();
            let arg = arg_anchor.as_ref();

            file_lang = None;

            if arg == "-h" || arg == "--help" {
                self.print_help();
                return false;
            } else if arg == "--help-full" {
                self.print_clang_help();
                return false;
            } else if arg == "--version" {
                self.print_version();
                return false;
            }

            if arg == "-fPIC" || arg == "-Qy" || arg == "--traditional-format" ||
                arg.ends_with("-gstabs") || arg.ends_with("-gdwarf2") ||
                arg == "--fatal-warnings" || arg.starts_with("-meabi=") ||
                arg.starts_with("-mfpu=") || arg == "-m32" || arg == "-emit-llvm" ||
                arg == "-msse" || arg == "-march=armv7-a" || arg == "-pipe"
            {
                // ignore.
                continue;
            }

            if arg == "-target" && expect_next(&mut raw_args) != "le32-unknown-nacl" ||
                (arg.starts_with("--target=") &&
                 (&arg[8..]) != "le32-unknown-nacl") ||
                arg == "--pnacl-allow-native" ||
                arg == "--pnacl-allow-translate"
            {
                panic!("this driver must be used to target PNaCl");
            } else if arg == "-allow-asm" {
                panic!("pure PNaCl can't have asm");
            }

            if arg == "--pnacl-allow-exceptions" {
                self.eh_mode = EhMode::SjLj;
            } else if arg.starts_with("--pnacl-exceptions=") {
                if &arg[19..] == "none" {
                    self.eh_mode = EhMode::Off;
                } else if &arg[19..] == "sjlj" {
                    self.eh_mode = EhMode::SjLj;
                } else {
                    panic!("`{}` is not a known EH mode",
                           &arg[19..]);
                }
            } else if arg == "-I" {
                self.add_driver_arg(format!("-I{}",
                                            expect_next(&mut raw_args).as_ref()));
            } else if arg.starts_with("-I") {
                self.add_driver_arg(arg);
            } else if arg == "-isystem" {
                self.add_driver_arg(format!("-isystem{}",
                                            expect_next(&mut raw_args).as_ref()));
            } else if arg.starts_with("-isystem") {
                self.add_driver_arg(arg);
            } else if arg == "-isysroot" {
                self.add_driver_arg(arg);
                self.add_driver_arg(expect_next(&mut raw_args));
            } else if arg.starts_with("-isysroot") {
                self.add_driver_arg("-isysroot");
                self.add_driver_arg(&arg[8..].to_owned());
            } else if arg == "-iquote" {
                self.add_driver_arg(arg);
                self.add_driver_arg(expect_next(&mut raw_args));
            } else if arg.starts_with("-iquote") {
                self.add_driver_arg("-iquote");
                self.add_driver_arg(&arg[7..].to_owned());
            } else if arg == "-idirafter" {
                self.add_driver_arg(format!("-idirafter{}",
                                            expect_next(&mut raw_args).as_ref()));
            } else if arg.starts_with("-idirafter") {
                self.add_driver_arg(&arg[..]);
            } else if arg.starts_with("-mfloat-abi=") {
                self.add_driver_arg(arg);
            } else if arg.starts_with("-f") {
                self.add_driver_arg(arg);
            } else if arg == "-arch" && expect_next(&mut raw_args) != "le32" {
                panic!("-arch must use `le32`");
            } else if arg == "-c" {
                self.set_gcc_mode(GccMode::Dashc);
            } else if arg == "-E" {
                self.set_gcc_mode(GccMode::DashE);
            } else if arg.starts_with("-Wl,") {
                self.add_linker_arg(&arg[4..]);
            } else if arg == "-l" {
                self.add_linker_arg(format!("-l{}",
                                            expect_next(&mut raw_args).as_ref()));
            } else if arg == "-Xlinker" {
                self.add_linker_arg(format!("-Xlinker={}",
                                            expect_next(&mut raw_args).as_ref()));
            } else if arg.starts_with("-l") ||
                arg == "-Bstatic" || arg == "-Bdynamic"
            {
                self.add_linker_arg(arg);
            } else if arg == "-o" {
                self.set_output(expect_next(&mut raw_args).as_ref());
            } else if arg.starts_with("-o") {
                self.set_output(&arg[2..]);
            } else if arg == "-v" {
                self.set_verbose();
            } else if !&arg[..].starts_with("-") || arg == "-" {
                self.add_input(arg, file_lang.clone());
            } else {
                panic!("unknown argument: `{}`",
                       arg);
            }
        }

        return true;
    }

    fn add_driver_arg<T: AsRef<str>>(&mut self, arg: T) {
        self.driver_args.push(arg.as_ref().to_owned());
    }
    fn add_linker_arg<T: AsRef<str>>(&mut self, arg: T) {
        self.linker_args.push(arg.as_ref().to_owned());
    }
    fn add_input<T: AsRef<Path>>(&mut self, file: T, file_lang: Option<FileLang>) {
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

#[cfg(target_os = "nacl")]
#[link(name = "ppapi_cpp", kind = "static")]
#[link(name = "ppapi_simple_cpp", kind = "static")]
#[link(name = "ppapi_stub", kind = "static")]
#[link(name = "cli_main", kind = "static")]
#[link(name = "tar", kind = "static")]
#[link(name = "nacl_spawn", kind = "static")]
extern { }

#[cfg_attr(target_os = "nacl", main_link_name = "nacl_main")]
pub fn main() {
    let mut invocation = Invocation::new();

    let args: Vec<String> = env::args().collect();
    let args: Vec<String> = (&args[1..]).iter().cloned().collect();

    if !invocation.process_args(args.into_iter()) { return; }
    invocation.queue_all();
    invocation.run_all();
}

use super::{Invocation, link, get_system_dir};
use util::{CommandQueue, get_crate_root};

use clang_driver;

use std::collections::HashSet;
use std::error::Error;
use std::fs::read_dir;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};

const SRC_DIR: &'static str = "system/lib/libc/musl/src";
const MODULE_BLACKLIST: &'static [&'static str] = &[
  "ipc",
  "thread",
  "sched",
  "linux",
  "aio",
  "legacy",
  "mq",
  "process",
  "search",
  "setjmp",
  "ldso",
];

#[derive(Copy, Clone)]
enum Blacklisted {
  File(&'static str),
  ModuleFile(&'static str, &'static str),
}
impl Blacklisted {
  fn test(&self, path: &Path) -> bool {
    match self {
      &Blacklisted::File(bfile) => {
        let name = path.file_name().unwrap();
        let name = name.to_str().unwrap();
        bfile == name
      },
      &Blacklisted::ModuleFile(mname, fname) => {
        path.parent().unwrap().file_name().unwrap().to_str().unwrap() == mname &&
          path.file_name().unwrap().to_str().unwrap() == fname
      },
    }
  }
}
impl From<&'static str> for Blacklisted {
  fn from(s: &'static str) -> Blacklisted {
    Blacklisted::File(s)
  }
}
impl From<(&'static str, &'static str)> for Blacklisted {
  fn from((m, f): (&'static str, &'static str)) -> Blacklisted {
    Blacklisted::ModuleFile(m, f)
  }
}

const FILE_BLACKLIST: &'static [Blacklisted] = &[
  Blacklisted::File("getaddrinfo.c"),
  Blacklisted::File("getnameinfo.c"),
  Blacklisted::File("gethostbyaddr.c"),
  Blacklisted::File("gethostbyaddr_r.c"),
  Blacklisted::File("gethostbyname.c"),
  Blacklisted::File("gethostbyname2_r.c"),
  Blacklisted::File("gethostbyname_r.c"),
  Blacklisted::File("gethostbyname2.c"),
  Blacklisted::File("usleep.c"),
  Blacklisted::File("alarm.c"),
  Blacklisted::File("syscall.c"),
  Blacklisted::File("__init_tls.c"),
  Blacklisted::File("__stack_chk_fail.c"),
  Blacklisted::File("timer_create.c"),
  Blacklisted::File("timer_delete.c"),
  Blacklisted::File("timer_getoverrun.c"),
  Blacklisted::File("timer_gettime.c"),
  Blacklisted::File("timer_settime.c"),
  Blacklisted::File("popen.c"),
  Blacklisted::ModuleFile("passwd", "getgrouplist.c"),
];
const FILE_WHITELIST: &'static [&'static str] = &[
  //"thread/wasm_pthread_stubs.c",

  "ldso/dlopen.c", "ldso/dlerror.c", "ldso/dlclose.c",
  "ldso/dladdr.c", "ldso/dlsym.c",
  "ldso/tlsdesc.c", "ldso/__dlsym.c",

  "legacy/getpagesize.c",

  "linux/sysinfo.c",
  "linux/sbrk.c",
  "linux/setgroups.c",

  "process/execl.c",
  "process/execle.c",
  "process/execv.c",
  "process/execve.c",
  "process/wait.c",
  "process/waitpid.c",

  "thread/clone.c",
  "thread/pthread_cleanup_push.c",
];

struct State {
  module_bl: HashSet<&'static str>,
  files: Vec<PathBuf>,
}
impl Default for State {
  fn default() -> Self {
    State {
      module_bl: FromIterator::from_iter(MODULE_BLACKLIST.iter().map(|&v| v )),
      files:     Vec::new(),
    }
  }
}

impl State {
  fn is_file_blacklisted(&self, file: &Path) -> bool {
    FILE_BLACKLIST
      .iter()
      .any(|&i| {
        i.test(file)
      })
  }
  fn visit_file(&mut self, file: &Path)
    -> Result<(), Box<Error>>
  {
    let name = file.file_name().unwrap();
    let name = name.to_str().unwrap();
    if name.ends_with(".c") && !self.is_file_blacklisted(file) {
      self.files.push(file.to_path_buf());
    }

    Ok(())
  }
  fn visit_dir(&mut self, dir: &Path)
    -> Result<(), Box<Error>>
  {
    let name = dir.file_name().unwrap();
    let name = name.to_str().unwrap();
    if !self.module_bl.contains(name) {
      for entry in read_dir(dir)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if ft.is_file() {
          self.visit_file(entry.path().as_path())?;
        } else if ft.is_dir() {
          self.visit_dir(entry.path().as_path())?;
        } else {
          panic!("symlink unimplemented");
        }
      }
    }

    Ok(())
  }
}

fn get_musl_root() -> PathBuf {
  get_system_dir()
    .join("musl")
}

pub fn build_c(invoc: &Invocation,
               file: &PathBuf,
               queue: &mut &mut CommandQueue<Invocation>)
{
  let mut clang = clang_driver::Invocation::default();
  clang.driver_mode = clang_driver::DriverMode::CC;
  let mut args = Vec::new();
  args.push("-c".to_string());
  args.push(format!("{}", file.display()));

  let idir = get_musl_root()
    .join("src")
    .join("internal");
  args.push(format!("-I{}", idir.display()));
  let idir = get_musl_root()
    .join("arch/wasm32");
  args.push(format!("-I{}", idir.display()));

  let out_file = format!("{}.obj", file.file_name().unwrap().to_str().unwrap());
  let out_file = Path::new(&out_file).to_path_buf();

  args.push("-Oz".to_string());
  super::add_default_args(&mut args);

  let cmd = queue
    .enqueue_tool(Some("clang"),
                  clang, args, true,
                  None::<Vec<::tempdir::TempDir>>)
    .expect("internal error: bad clang arguments");
  cmd.prev_outputs = false;
  cmd.output_override = true;
  cmd.intermediate_name = Some(out_file);
}

impl Invocation {
  pub fn build_musl(&self, mut queue: &mut CommandQueue<Invocation>)
    -> Result<(), Box<Error>>
  {
    use std::fs::File;
    use std::io::Write;
    use std::process::Command;

    use tempdir::TempDir;

    // configure arch/wasm32/bits/*.in

    let clang = self.tc.llvm_tool("clang");
    let lld   = self.tc.llvm_tool("wasm-ld");

    let prefix = self.tc.sysroot_cache();
    let lib_dir = prefix.join("lib");

    let config = format!(r#"
CROSS_COMPILE=llvm-
CC={}
CFLAGS=-target wasm32-unknown-unknown-wasm
LDFLAGS=-fuse-ld={} -Wl,--relocatable,--import-memory,--import-table -L{}

prefix={}
includedir=$(prefix)/include
libdir=$(prefix)/lib
syslibdir=$(prefix)/lib

LIBCC=-lcompiler-rt
ARCH=wasm32
"#,
                         clang.display(), lld.display(),
                         lib_dir.display(), prefix.display());

    let config_mak = get_musl_root()
      .join("config.mak");
    let mut config_mak = File::create(config_mak)?;
    config_mak.write_all(config.as_ref())?;

    let mut cmd = Command::new("make");
    cmd.current_dir(get_musl_root())
      .arg("install");
    queue.enqueue_external(None, cmd, None,
                           false, None::<Vec<TempDir>>);

    Ok(())
  }
}
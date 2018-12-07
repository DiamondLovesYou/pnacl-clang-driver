use super::{Invocation, link, get_system_dir};
use util::{CommandQueue, get_crate_root};

use clang_driver;

use std::error::Error;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};

fn get_musl_root() -> PathBuf {
  get_system_dir()
    .join("musl")
}

impl Invocation {
  pub fn build_musl(&self, mut queue: &mut CommandQueue<Invocation>)
    -> Result<(), Box<Error>>
  {
    use std::env::home_dir;
    use std::fs::File;
    use std::io::Write;
    use std::process::Command;

    use tempdir::TempDir;

    // configure arch/wasm32/bits/*.in

    let clang = self.tc.llvm_tool("clang");
    // FIXME what if cargo is installed in a non-default location? Msys comes to mind.
    let lld   = home_dir().unwrap().join(".cargo/bin/wasm-ld");

    let prefix = self.tc.sysroot_cache();
    let lib_dir = prefix.join("lib");

    let config = format!(r#"
CROSS_COMPILE=llvm-
CC={}
CFLAGS=-target wasm32-unknown-unknown-wasm
LDFLAGS=-fuse-ld={} -v {} -L{} -Oz

prefix={}
includedir=$(prefix)/include
libdir=$(prefix)/lib
syslibdir=$(prefix)/lib

LIBCC=-lcompiler-rt
ARCH=wasm32
"#,
                         clang.display(), lld.display(), self.c_cxx_linker_args(),
                         lib_dir.display(), prefix.display());

    let config_mak = get_musl_root()
      .join("config.mak");
    let mut config_mak = File::create(config_mak)?;
    config_mak.write_all(config.as_ref())?;

    if self.clobber_libc_build {
      let mut cmd = Command::new("make");
      cmd.current_dir(get_musl_root())
        .arg("clean");
      queue.enqueue_simple_external(Some("clobber-libc"), cmd, None);
    }

    let mut cmd = Command::new("make");
    cmd.current_dir(get_musl_root())
      .arg("install")
      .arg("-j8");
    queue.enqueue_external(None, cmd, None,
                           false, None::<Vec<TempDir>>);

    Ok(())
  }
}

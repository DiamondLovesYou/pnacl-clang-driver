use super::{Invocation, link, get_system_dir};
use util::{CommandQueue, get_crate_root, CreateIfNotExists};

use clang_driver;

use std::fs::remove_dir_all;
use std::error::Error;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};

fn get_musl_root() -> PathBuf {
  get_system_dir()
    .join("musl")
}

impl Invocation {
  pub fn musl_include_dir(&self) -> PathBuf {
    get_musl_root()
      .join("include")
  }
  pub fn musl_build_obj_dir(&self) -> Result<PathBuf, Box<Error>> {
    Ok(get_musl_root().join("obj").create_if_not_exists()?)
  }
  pub fn dlmalloc_obj_output(&self) -> Result<PathBuf, Box<Error>> {
    Ok(self.musl_build_obj_dir()?.join("dlmalloc.o"))
  }

  /// XXX dlmalloc object path is hardcoded.
  pub fn build_musl(&self, queue: &mut CommandQueue<Invocation>,
                    // we need to (re-)build dlmalloc if we're clobbering.
                    dlmalloc_built: &mut bool)
    -> Result<(), Box<Error>>
  {
    use std::env::home_dir;
    use std::fs::File;
    use std::io::Write;
    use std::process::Command;

    use tempdir::TempDir;

    // configure arch/wasm32/bits/*.in

    let clang = home_dir().unwrap().join(".cargo/bin/wasm-clang");
    // FIXME what if cargo is installed in a non-default location? Msys comes to mind.
    let lld   = home_dir().unwrap().join(".cargo/bin/wasm-ld");

    let prefix = self.tc.sysroot_cache();
    let lib_dir = prefix.join("lib");

    let dlmalloc_o = self.dlmalloc_obj_output()?;

    let config_mak = get_musl_root()
      .join("config.mak");
    let mut config_mak = File::create(config_mak)?;

    let mut ld_flags = String::new();
    for arg in self.c_cxx_linker_args().into_iter() {
      ld_flags.push_str(arg.as_ref());
      ld_flags.push(' ');
    }

    let config = writeln!(config_mak, r#"
CROSS_COMPILE=llvm-
CC={}
LD={}
CFLAGS=
LDFLAGS={} -L{} -Oz

prefix={}
includedir=$(prefix)/include
libdir=$(prefix)/lib
syslibdir=$(prefix)/lib

LIBCC=-lcompiler-rt
ARCH=wasm32
EXTRA_OBJS := {}
"#,
                          clang.display(),
                          lld.display(),
                          ld_flags,
                          lib_dir.display(),
                          prefix.display(),
                          dlmalloc_o.display())?;

    if self.clobber_libc_build {
      let f = |this: &mut &mut Self| {
        let musl = get_musl_root();
        let _ = remove_dir_all(musl.join("obj"));
        let _ = remove_dir_all(musl.join("lib"));

        let _ = this.musl_build_obj_dir();

        Ok(())
      };

      queue.enqueue_function(Some("clobber previous musl build"), f)
        .prev_outputs = false;

      *dlmalloc_built = false;
    }

    if !*dlmalloc_built {
      self.build_dlmalloc(queue)?;
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

use super::{Invocation, link, get_system_dir};
use util::{CommandQueue, get_crate_root, CreateIfNotExists};

use clang_driver;

use std::fs::remove_dir_all;
use std::error::Error;
use std::iter::FromIterator;
use std::path::{Path, PathBuf};

impl Invocation {
  pub fn get_musl_root(&self) -> PathBuf {
    self.srcs.join(self.musl_repo.name.as_ref())
  }
  pub fn musl_include_dir(&self) -> PathBuf {
    self.get_musl_root()
      .join("include")
  }
  pub fn musl_build_obj_dir(&self) -> Result<PathBuf, Box<Error>> {
    Ok(self.get_musl_root().join("obj").create_if_not_exists()?)
  }
  pub fn dlmalloc_obj_output(&self) -> Result<PathBuf, Box<Error>> {
    Ok(self.musl_build_obj_dir()?.join("dlmalloc.o"))
  }

  pub fn checkout_musl(&mut self) -> Result<(), Box<Error>> {
    if self.musl_checkout { return Ok(()); }
    self.musl_checkout = true;
    self.musl_repo.checkout_thin(self.get_musl_root())
  }

  pub fn init_musl(&mut self) -> Result<(), Box<Error>> {
    use std::env::home_dir;
    use std::fs::File;
    use std::io::Write;
    use std::process::Command;

    if self.musl_inited { return Ok(()); }

    {
      let clang = home_dir().unwrap().join(".cargo/bin/wasm-clang");
      // FIXME what if cargo is installed in a non-default location? Msys comes to mind.
      let lld = home_dir().unwrap().join(".cargo/bin/wasm-ld");

      let prefix = self.tc().sysroot_cache();
      let lib_dir = prefix.join("lib");

      let dlmalloc_o = self.dlmalloc_obj_output()?;

      let config_mak = self.get_musl_root()
        .join("config.mak");
      let mut config_mak = File::create(config_mak)?;

      let mut ld_flags = String::new();
      for arg in self.c_cxx_linker_args().into_iter() {
        ld_flags.push_str(arg.as_ref());
        ld_flags.push(' ');
      }

      let config = writeln!(config_mak, r#"
CROSS_COMPILE={}
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
                            self.tc().llvm_tool("llvm-").display(),
                            clang.display(),
                            lld.display(),
                            ld_flags,
                            lib_dir.display(),
                            prefix.display(),
                            dlmalloc_o.display())?;
    }

    self.musl_inited = true;

    Ok(())
  }

  pub fn configure_musl(&mut self, queue: &mut CommandQueue<Self>)
    -> Result<(), Box<Error>>
  {
    use std::process::Command;

    if self.musl_configured { return Ok(()); }

    self.init_musl()?;

    // configure arch/wasm32/bits/*.in
    // this needs to happen before compiler-rt can be built.
    let mut cmd = Command::new("make");
    cmd.current_dir(self.get_musl_root())
      .arg("obj/include/bits/alltypes.h")
      .arg("obj/include/bits/syscall.h")
      .arg("-j8");
    self.tc().set_envs(&mut cmd);
    queue.enqueue_simple_external(Some("configure musl"), cmd, None);

    self.musl_configured = true;

    Ok(())
  }

  /// XXX dlmalloc object path is hardcoded.
  pub fn build_musl(&mut self, queue: &mut CommandQueue<Invocation>,
                    // we need to (re-)build dlmalloc if we're clobbering.
                    dlmalloc_built: &mut bool)
    -> Result<(), Box<Error>>
  {
    use std::env::home_dir;
    use std::fs::File;
    use std::io::Write;
    use std::process::Command;

    self.init_musl()?;

    if self.clobber_libc_build {
      let f = |this: &mut &mut Self| {
        let musl = this.get_musl_root();
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
      *dlmalloc_built = true;
    }

    let mut cmd = Command::new("make");
    cmd.current_dir(self.get_musl_root())
      .arg("install")
      .arg("-j8");
    self.tc().set_envs(&mut cmd);
    queue.enqueue_simple_external(Some("install musl"),
                                  cmd, None);

    Ok(())
  }
}

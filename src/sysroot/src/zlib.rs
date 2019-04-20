use super::{Invocation, link};
use util::{CommandQueue, get_crate_root, CreateIfNotExists, Tool, };

use clang_driver;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command, };

fn build_flags(flags: Vec<String>) -> String {
  let mut out = String::new();
  for (i, flag) in flags.into_iter().enumerate() {
    if i == 0 {
      out = flag;
    } else {
      out.push(' ');
      out.push_str(&flag);
    }
  }

  out
}
impl Invocation {
  pub fn zlib_build_dir(&self) -> PathBuf {
    get_crate_root()
      .join("system/zlib-build")
  }
  pub fn build_zlib(&self, queue: &mut CommandQueue<Invocation>)
    -> Result<(), Box<Error>>
  {
    let src = get_crate_root()
      .join("system/zlib");

    if self.clobber_zlib_build {
      let f = move |sess: &mut &mut Invocation| {
        let build = sess.zlib_build_dir();
        if build.exists() {
          ::std::fs::remove_dir_all(&build)?;
          build.create_if_not_exists()?;
        }

        Ok(())
      };
      queue.enqueue_function(Some("clobber-zlib-build"), f);
    }

    let build_dir = self.zlib_build_dir()
      .create_if_not_exists()?;
    let install_dir = self.tc.sysroot_cache()
      .create_if_not_exists()?;

    let mut cflags = vec![self.c_cxx_linker_cflags()];

    if self.emit_wast {
      cflags.push("--emit-wast".into());
    }

    let cflags = build_flags(cflags);

    let mut conf = Command::new(src.join("configure"));
    conf.current_dir(&build_dir)
      .env("CC", self.cc())
      .env("CXX", self.cxx())
      .env("CFLAGS", &cflags)
      .env("CXXFLAGS", &cflags)
      .arg(format!("--prefix={}", install_dir.display()));


    {
      let cmd = queue
        .enqueue_simple_external(Some("configure zlib"),
                                 conf, None);

      cmd.prev_outputs = false;
      cmd.output_override = false;
    }

    let mut install = Command::new("make");
    install.current_dir(&build_dir)
      .arg("install");
    {
      let cmd = queue
        .enqueue_simple_external(Some("install zlib"),
                                 install, None);

      cmd.prev_outputs = false;
      cmd.output_override = false;
    }

    Ok(())
  }
}

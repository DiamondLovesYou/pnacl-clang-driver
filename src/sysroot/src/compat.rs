use super::{Invocation, link};
use util::{CommandQueue, ToolInvocation, ToolArgs, CreateIfNotExists, Tool};

use clang_driver;
use cmake_driver;

use std::error::Error;
use std::path::{Path, PathBuf};

const FILES: &'static [&'static str] = &[
  "ctype.h",
];

impl Invocation {
  pub fn build_compat(&self, queue: &mut CommandQueue<Self>) -> Result<(), Box<Error>> {
    use std::fs::copy;
    let f = move |sess: &mut &mut Invocation| {
      for &file in FILES.iter() {
        let dest = sess.tc().sysroot()
          .join("include/compat")
          .create_if_not_exists()?
          .join(file);
        let file = super::get_system_dir()
          .join("compat/include")
          .join(file);
        copy(file, dest)?;
      }

      Ok(())
    };
    queue.enqueue_function(Some("build-compat-headers"), f);

    Ok(())
  }
}

use super::{Invocation, link};
use util::{CommandQueue, ToolInvocation, ToolArgs, CreateIfNotExists, Tool};

use clang_driver;
use cmake_driver;

use std::error::Error;
use std::path::{Path, PathBuf};

impl Invocation {
  pub fn libunwind_src(&self) -> PathBuf {
    super::get_system_dir()
      .join("libunwind")
  }
  pub fn build_libunwind(&self, queue: &mut CommandQueue<Self>) -> Result<(), Box<Error>> {
    use std::process::Command;
    use tempdir::TempDir;

    use cmake_driver::{Var};

    if self.clobber_libunwind_build {
      let f = move |sess: &mut &mut Invocation| {
        let libunwind_build = super::get_system_dir()
          .join("libunwind-build");
        ::std::fs::remove_dir_all(&libunwind_build)?;
        libunwind_build.create_if_not_exists()?;

        Ok(())
      };
      queue.enqueue_function(Some("clobber-libunwind-build"), f);
    }

    let libcxx    = self.libcxx_src();
    let libcxxabi = self.libcxxabi_src();
    let libunwind = self.libunwind_src();

    let libunwind_build = super::get_system_dir()
      .join("libunwind-build")
      .create_if_not_exists()?;

    let sysroot = self.tc.sysroot_cache();

    let mut cmake = cmake_driver::Invocation::default();
    cmake.override_output(libunwind_build.clone());
    cmake
      .cmake_on("LIBUNWIND_USE_COMPILER_RT")
      .cmake_on("LLVM_ENABLE_LIBCXX")
      .cmake_on("LIBUNWIND_ENABLE_SHARED")
      .cmake_off("LIBUNWIND_ENABLE_ASSERTIONS")
      .cmake_off("LIBUNWIND_ENABLE_THREADS")
      .cmake_str("LIBUNWIND_TARGET_TRIPLE", "wasm32-unknown-unknown-wasm")
      .cmake_path("LIBUNWIND_SYSROOT", &sysroot)
      // cmake removes the trailing slash if it is a path type,
      // which is important for this var.
      .cmake_str("LIBUNWIND_INSTALL_PREFIX",
                 format!("{}/", sysroot.display()))
      .cmake_path("LLVM_PATH", self.llvm_src())
      .cmake_path("LIBUNWIND_CXX_INCLUDE_PATHS", libcxx.join("include"))
      .cmake_path("LLVM_CONFIG_PATH", self.tc.llvm_tool("llvm-config"))
      .c_cxx_flag("-nodefaultlibs")
      .c_cxx_flag("-lc")
      .c_cxx_flag(self.c_cxx_linker_cflags())
      .c_cxx_flag("-D_LIBUNWIND_DISABLE_VISIBILITY_ANNOTATIONS")
      .generator("Ninja");

    {
      let cmd = queue.enqueue_tool(None, cmake,
                                   vec![format!("{}", libunwind.display()), ],
                                   false, None::<Vec<TempDir>>)?;
      cmd.prev_outputs = false;
      cmd.output_override = false;
    }

    let mut cmd = Command::new("ninja");
    cmd.current_dir(libunwind_build);
      //.arg("install");

    queue.enqueue_external(None, cmd, None,
                           false, None::<Vec<TempDir>>);
    Ok(())
  }
}

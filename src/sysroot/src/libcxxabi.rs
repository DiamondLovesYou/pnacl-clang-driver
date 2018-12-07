use super::{Invocation, link};
use util::{CommandQueue, ToolInvocation, ToolArgs, CreateIfNotExists, Tool};

use clang_driver;
use cmake_driver;

use std::error::Error;
use std::path::{Path, PathBuf};

// this is, like, almost the exact same as libc++.

impl Invocation {
  pub fn libcxxabi_src(&self) -> PathBuf {
    super::get_system_dir()
      .join("libcxxabi")
  }
  pub fn build_libcxxabi(&self, mut queue: &mut CommandQueue<Invocation>) -> Result<(), Box<Error>> {
    use std::process::Command;
    use tempdir::TempDir;

    use cmake_driver::{Var};

    if self.clobber_libcxxabi_build {
      let f = move |sess: &mut &mut Invocation| {
        let libcxxabi_build = super::get_system_dir()
          .join("libcxxabi-build");
        ::std::fs::remove_dir_all(&libcxxabi_build)?;
        libcxxabi_build.create_if_not_exists()?;

        Ok(())
      };
      queue.enqueue_function(Some("clobber-libcxxabi-build"), f);
    }

    let libcxx    = self.libcxx_src();
    let libcxxabi = self.libcxxabi_src();

    let libcxxabi_build = super::get_system_dir()
      .join("libcxxabi-build")
      .create_if_not_exists()?;

    let sysroot = self.tc.sysroot_cache();

    let mut cmake = cmake_driver::Invocation::default();
    cmake.override_output(libcxxabi_build.clone());
    cmake
      .cmake_off("LIBCXXABI_USE_LLVM_UNWINDER")
      .cmake_on("LIBCXXABI_USE_COMPILER_RT")
      .cmake_on("LLVM_ENABLE_LIBCXX")
      .cmake_on("LIBCXXABI_ENABLE_SHARED")
      .cmake_on("LIBCXXABI_ENABLE_THREADS")
      .cmake_off("LIBCXXABI_ENABLE_EXCEPTIONS")
      .cmake_str("LIBCXXABI_TARGET_TRIPLE", "wasm32-unknown-unknown-wasm")
      .cmake_path("LIBCXXABI_SYSROOT", &sysroot)
      // cmake removes the trailing slash if it is a path type,
      // which is important for this var.
      .cmake_str("LIBCXXABI_INSTALL_PREFIX",
                 format!("{}/", sysroot.display()))
      .cmake_str("CMAKE_INSTALL_PREFIX",
                 format!("{}/", sysroot.display()))
      .cmake_str("CMAKE_BUILD_TYPE", "MinSizeRel")
      .cmake_path("LLVM_PATH", self.llvm_src())
      .cmake_path("LIBCXXABI_LIBCXX_PATH", libcxx)
      .c_cxx_flag("-nodefaultlibs")
      .c_cxx_flag("-lc")
      .c_cxx_flag(self.c_cxx_linker_args())
      .c_cxx_flag("-D_LIBCPP_HAS_THREAD_API_PTHREAD")
      .c_cxx_flag(format!("-I{}", self.libunwind_src().join("include").display()))
      .generator("Ninja");

    {
      let cmd = queue.enqueue_tool(None, cmake,
                                   vec![format!("{}", libcxxabi.display()), ],
                                   false, None::<Vec<TempDir>>)?;
      cmd.prev_outputs = false;
      cmd.output_override = false;
    }

    let mut cmd = Command::new("ninja");
    cmd.current_dir(libcxxabi_build)
      .arg("install");

    queue.enqueue_external(None, cmd, None,
                           false, None::<Vec<TempDir>>);
    Ok(())
  }
}

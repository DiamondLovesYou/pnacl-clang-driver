
use super::{Invocation, link};
use util::{CommandQueue, ToolInvocation, ToolArgs, CreateIfNotExists, Tool};

use clang_driver;
use cmake_driver;

use std::error::Error;
use std::path::{Path, PathBuf};

impl Invocation {
  pub fn libcxx_src(&self) -> PathBuf {
    super::get_system_dir()
      .join("libcxx")
  }
  pub fn build_libcxx(&self, queue: &mut CommandQueue<Invocation>) -> Result<(), Box<Error>> {
    use std::process::Command;
    use tempdir::TempDir;

    if self.clobber_libcxx_build {
      let f = move |sess: &mut &mut Invocation| {
        let libcxx_build = super::get_system_dir()
          .join("libcxx-build");
        ::std::fs::remove_dir_all(&libcxx_build)?;
        libcxx_build.create_if_not_exists()?;

        Ok(())
      };
      queue.enqueue_function(Some("clobber-libcxx-build"), f);
    }

    let libcxx = self.libcxx_src();
    let libcxxabi = self.libcxxabi_src();

    let libcxx_build = super::get_system_dir()
      .join("libcxx-build")
      .create_if_not_exists()?;

    let sysroot = self.tc.sysroot_cache();

    let mut cmake = cmake_driver::Invocation::default();
    cmake.override_output(libcxx_build.clone());
    cmake
      .cmake_on("LIBCXX_USE_COMPILER_RT")
      .cmake_on("LIBCXX_HAS_MUSL_LIBC")
      .cmake_on("LIBCXX_ENABLE_STATIC")
      .cmake_on("LIBCXX_ENABLE_SHARED")
      .cmake_on("LIBCXX_ENABLE_THREADS")
      .cmake_on("LIBCXX_INSTALL_SUPPORT_HEADERS")
      .cmake_off("LIBCXX_ENABLE_WERROR")
      .cmake_off("LIBCXX_ENABLE_EXCEPTIONS")
      // cmake removes the trailing slash if it is a path type,
      // which is important for this var.
      .cmake_str("LIBCXX_INSTALL_PREFIX",
                 format!("{}/", sysroot.display()))
      .cmake_str("CMAKE_INSTALL_PREFIX",
                 format!("{}/", sysroot.display()))
      .cmake_str("LIBCXX_TARGET_TRIPLE", "wasm32-unknown-unknown-wasm")
      .cmake_str("LIBCXX_CXX_ABI", "libcxxabi")
      .cmake_str("CMAKE_BUILD_TYPE", "MinSizeRel")
      .cmake_path("LIBCXX_SYSROOT", &sysroot)
      .cmake_path("LIBCXX_CXX_ABI_LIBRARY_PATH",
                  sysroot.join("lib"))
      .cmake_path("LIBCXX_LIBRARY_DIR",
                  sysroot.join("lib"))
      .cmake_path("LLVM_PATH", self.llvm_src())
      .c_cxx_flag("-nodefaultlibs")
      .c_cxx_flag("-lc")
      .c_cxx_flag("-O3")
      .c_cxx_flag("--emit-wast")
      .c_cxx_flag(self.c_cxx_linker_cflags())
      .shared_ld_flag("-Wl,--relocatable")
      .exe_ld_flag("-Wl,--gc-sections")
      .c_cxx_flag(format!("-I{}", self.libcxxabi_src().join("include").display()))
      .c_cxx_flag(format!("-I{}", libcxx.join("include/support/musl").display()))
      .c_cxx_flag("-D_LIBCPP_HAS_THREAD_API_PTHREAD")
      .generator("Ninja");

    {
      let cmd = queue.enqueue_tool(None, cmake,
                                   vec![format!("{}", libcxx.display()), ],
                                   false, None::<Vec<TempDir>>)?;
      cmd.prev_outputs = false;
      cmd.output_override = false;
    }

    let mut cmd = Command::new("ninja");
    cmd.current_dir(libcxx_build)
      .arg("install");

    queue.enqueue_external(None, cmd, None,
                           false, None::<Vec<TempDir>>);
    Ok(())
  }
}

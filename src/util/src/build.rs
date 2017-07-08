extern crate regex;

use std::default::Default;
use std::env::{current_dir, var_os};
use std::fs::{File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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

const FOR_NACL_ENV: &'static str = "CARGO_FEATURE_NACL";

fn main() {
  if var_os(FOR_NACL_ENV).is_some() {
    let libs_dir = current_dir()
      .unwrap()
      .join("lib");
    println!("cargo:rustc-link-search=native={}",
             libs_dir.display());

    let mut rev = need_nacl_toolchain();
    rev.push("REV");
    let dest = Path::new(&var_os("OUT_DIR").unwrap())
      .join("REV");

    let mut rev = File::open(rev)
      .unwrap();
    let mut rev_str = Default::default();
    rev.read_to_string(&mut rev_str)
      .unwrap();

    let re = regex::Regex::new(r"\[GIT\].*/native_client(?:\.git)?:\s*([0-9a-f]{40})").unwrap();
    let caps = re.captures(&rev_str[..])
      .unwrap_or_else(|| {
        panic!("woa! couldn't find the native_client revision!");
      });
    let only_ref = caps.get(1);
    let only_ref = only_ref.expect("expected two regex captures in revision");

    let mut dest = File::create(dest)
      .unwrap();

    (write!(dest, " nacl-version={}", only_ref.as_str())).unwrap();
  }
}

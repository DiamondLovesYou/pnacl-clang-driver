
extern crate clang_driver;
extern crate wasm_driver_utils as util;
extern crate env_logger;

pub fn main() {
  env_logger::init();
  let _ = util::main::<clang_driver::Invocation>(None);
}

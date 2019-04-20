
extern crate clang_driver;
extern crate util;
extern crate env_logger;

pub fn main() {
  env_logger::init();
  let _ = util::main::<clang_driver::Invocation>(None);
}

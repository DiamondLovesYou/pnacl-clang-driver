
extern crate cmake_driver;
extern crate util;
extern crate env_logger;

pub fn main() {
  env_logger::init();
  let _ = util::main::<cmake_driver::Invocation>(None);
}

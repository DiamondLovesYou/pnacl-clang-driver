
extern crate cmake_driver;
extern crate util;

pub fn main() {
  let _ = util::main::<cmake_driver::Invocation>(None);
}

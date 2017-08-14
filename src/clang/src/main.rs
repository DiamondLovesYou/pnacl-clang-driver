
extern crate clang_driver;
extern crate util;

pub fn main() {
  let _ = util::main::<clang_driver::Invocation>(None);
}

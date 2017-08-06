extern crate sysroot_driver;
extern crate util;

pub fn main() {
    let _ = util::main::<sysroot_driver::Invocation>(None);
}

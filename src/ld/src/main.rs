
extern crate ld_driver;
extern crate util;

pub fn main() {
    let _ = util::main::<ld_driver::Invocation>(None);
}

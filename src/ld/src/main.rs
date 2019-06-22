
extern crate ld_driver;
extern crate wasm_driver_utils as util;
extern crate env_logger;

pub fn main() {
    env_logger::init();
    let _ = util::main::<ld_driver::Invocation>(None);
}

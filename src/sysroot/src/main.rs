extern crate wasm_sysroot_builder as sysroot_driver;
extern crate wasm_driver_utils as util;

pub fn main() {
    let _ = util::main::<sysroot_driver::Invocation>(None);
}

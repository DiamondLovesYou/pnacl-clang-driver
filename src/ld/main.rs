
extern crate pnacl_ld;
extern crate util;

pub fn main() {
    let _ = util::main::<pnacl_ld::Invocation>(None);
}

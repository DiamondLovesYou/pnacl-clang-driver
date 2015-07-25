
#![feature(plugin)]
#![plugin(regex_macros)]

use std::path::{Path, PathBuf};

use util::CommandQueue;

extern crate regex;
#[macro_use] extern crate util;

#[derive(Clone, Debug)]
pub struct Invocation {
    simplify_libcalls: bool,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    args: Vec<String>,
}
impl Default for Invocation {
    fn default() -> Invocation {
        Invocation {
            simplify_libcalls: false,
            input: Default::default(),
            output: Default::default(),
            args: Default::default(),
        }
    }
}

impl Invocation {
}

impl util::Tool for Invocation {
    fn enqueue_commands(&mut self, queue: &mut CommandQueue) -> Result<(), String> {
        let mut cmd = ::std::process::Command::new(util::get_bin_path("opt"));

        cmd.args(self.args.as_ref());
        cmd.arg(if self.simplify_libcalls { "-enable-simplify-libcalls" }
                else                      { "-disable-simplify-libcalls" });

        if self.input.is_some() {
            cmd.arg(self.input.as_ref().unwrap());
        }

        queue.enqueue_external(None, cmd, Some("-o"), false);

        Ok(())
    }

    fn get_name(&self) -> String { From::from("pnacl-opt") }

    fn get_output(&self) -> Option<&PathBuf> { self.output.as_ref() }
    fn override_output(&mut self, out: PathBuf) { self.output = Some(out); }
}

impl util::ToolInvocation for Invocation {
    fn check_state(&mut self, iteration: usize) -> Result<(), String> {
        debug_assert!(iteration == 0);

        Ok(())
    }
    fn args(&self, iteration: usize) -> Option<util::ToolArgs<Invocation>> {
        match iteration {
            0 => {
                static A: util::ToolArgs<Invocation> =
                    &[&SIMPLIFY_LIBCALLS,
                      &OUTPUT,
                      &ARGS,
                      &INPUTS,
                      ];
                Some(A)
            },
            _ => None,
        }
    }
}

tool_argument!(SIMPLIFY_LIBCALLS: Invocation = { r"^--(enable|disable)-simplify-libcalls$", None };
               fn set_simplify_libcalls(this, _single, cap) {
                   this.simplify_libcalls = match cap.at(1).unwrap() {
                       "enable" => true,
                       "disable" => false,
                       _ => unreachable!(),
                   };
                   Ok(())
               });

tool_argument!(OUTPUT: Invocation = { r"^-o(.+)$", Some(regex!(r"^-o$")) };
               fn set_output(this, single, cap) {
                   if this.output.is_some() {
                       return Err("more than one output specified".to_string());
                   }

                   let out = if single { cap.at(1).unwrap() }
                             else      { cap.at(0).unwrap() };
                   let out = Path::new(out);
                   let out = out.to_path_buf();
                   this.output = Some(out);
                   Ok(())
               });

tool_argument!(ARGS: Invocation = { r"^(-.*)$", None };
               fn add_arg(this, _single, cap) {
                   this.args.push(cap.at(0).unwrap().to_string());
                   Ok(())
               });

tool_argument!(INPUTS: Invocation = { r"^(.*)$", None };
               fn set_input(this, _single, cap) {
                   if this.output.is_some() {
                       return Err("more than one input specified".to_string());
                   }
                   let input = cap.at(0).unwrap();
                   this.input = Some(From::from(input));
                   Ok(())
               });

#![feature(plugin)]
#![plugin(regex_macros)]

use std::path::PathBuf;

extern crate regex;

#[macro_use]
extern crate util;

use util::{Tool, ToolInvocation, CommandQueue, ToolArgs};
use util::process_invocation_args;

#[derive(Debug)]
struct Test {
    check_state: Option<String>,
    arg: Option<String>,
}

impl Default for Test {
    fn default() -> Test {
        Test {
            check_state: Default::default(),
            arg: Default::default(),
        }
    }
}

impl Tool for Test {
    fn enqueue_commands(&mut self, _queue: &mut CommandQueue) -> Result<(), String> { unimplemented!() }

    fn get_name(&self) -> String { "test".to_string() }

    fn get_output(&self) -> Option<&PathBuf> { unimplemented!() }
    fn override_output(&mut self, _out: PathBuf)  { unimplemented!() }
}

argument!(impl SINGLE where { Some(r"^-?-single=(.+)$"), None } for Test {
    fn set_single(this, cap) {
        this.arg = Some(cap.at(1).unwrap().to_string());
    }
});
argument!(impl SPLIT where { None, Some(r"^-?-split") } for Test {
    fn set_split(this, cap) {
        this.arg = Some(cap.at(0).unwrap().to_string());
    }
});
argument!(impl BOTH where { Some(r"^-(both|single_only)(.+)$"), Some(r"^-(both|split_only)$") } for Test {
    fn set_both(this, cap) {
        let index;
        if cap.at(2).is_none() {
            index = 1;
        } else {
            index = 2;
        }
        this.arg = Some(cap.at(index).unwrap().to_string());
    }
});
argument!(impl ERROR where { Some(r"^--error$"), None } for Test {
    fn set_error(_this, _cap) {
        return Err("error".to_string());
    }
});

/// Tool argument processing.
impl ToolInvocation for Test {
    fn check_state(&mut self, _iteration: usize) -> Result<(), String> {
        if self.check_state.is_none() || self.arg == self.check_state { return Ok(()); }
        else { return Err("invalid state".to_string()); }
    }

    /// Called until `None` is returned. Put args that override errors before
    /// the the args that can have those errors.
    fn args(&self, iteration: usize) -> Option<ToolArgs<Self>> {
        match iteration {
            0 => {
                static ARGS: ToolArgs<Test> = &[&SINGLE, &SPLIT,
                                                &BOTH, &ERROR];
                Some(ARGS)
            },
            _ => None,
        }
    }
}

#[test]
fn single() {
    let args = vec!["--single=something".to_string()];
    let mut invocation: Test = Default::default();

    assert_eq!(process_invocation_args(&mut invocation, args),
               Ok(()));
    assert_eq!(invocation.arg, Some("something".to_string()));
}
#[test]
fn split() {
    let args = vec!["--split".to_string(),
                    "something".to_string(),
                    ];
    let mut invocation: Test = Default::default();

    assert_eq!(process_invocation_args(&mut invocation, args),
               Ok(()));
    assert_eq!(invocation.arg, Some("something".to_string()));
}
#[test]
fn both() {
    let args = vec!["-both".to_string(),
                    "something".to_string(),
                    ];
    let mut invocation: Test = Default::default();

    assert_eq!(process_invocation_args(&mut invocation, args),
               Ok(()));
    assert_eq!(invocation.arg, Some("something".to_string()));

    let args = vec!["-bothsomething".to_string(),
                    ];
    let mut invocation: Test = Default::default();

    assert_eq!(process_invocation_args(&mut invocation, args),
               Ok(()));
    assert_eq!(invocation.arg, Some("something".to_string()));
}

#[test]
fn error() {
    let args = vec!["--error".to_string(),
                    ];
    let mut invocation: Test = Default::default();

    assert_eq!(process_invocation_args(&mut invocation, args),
               Err("error on argument `--error`: `error`\n".to_string()));
    assert_eq!(invocation.arg, None);
}

#[test]
fn check_state() {
    let args = vec!["-bothsomething".to_string(),
                    ];
    let mut invocation: Test = Default::default();
    invocation.check_state = Some("not something".to_string());

    assert_eq!(process_invocation_args(&mut invocation, args),
               Err("invalid state".to_string()));
}

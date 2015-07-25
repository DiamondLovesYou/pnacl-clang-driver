#![feature(plugin)]
#![plugin(regex_macros)]

extern crate regex;
#[macro_use] extern crate util;


#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Translator {
    Subzero,
    Llc,
}

impl Default for Translator {
    fn default() -> Translator {
        Translator::Llc
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum OutputMode {
    Asm,
    Obj,
    Link,
}
impl Default for OutputMode {
    fn default() -> OutputMode {
        OutputMode::Link,
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SplitMode {
    Auto,
    Threads(usize),
}

impl Default for SplitMode {
    fn default() -> SplitMode {
        SplitMode::Auto
    }
}

#[derive(Debug, Clone)]
pub struct Invocation {
    pub translate_pso: bool,
    pub allow_bitcode_input: bool,
    pub use_irt: bool,
    pub use_irt_shim: bool,

    pub use_stdlib: bool,
    pub use_defaultlibs: bool,

    pub fast_trans: bool,

    pub eh_mode: util::EhMode,

    pub optimize: util::OptimizationGoal,

    pub backend: Translator,

    pub inputs: Vec<PathBuf>,
    pub output: PathBuf,
    pub output_mode: OutputMode,

    pub bitcode_stream_rate: u64,
}

impl Invocation {
    pub fn pic(&self) -> bool {
    }

    pub fn use_zerocost_eh(&self) -> bool {
        self.eh_mode == util::EhMode::Zerocost
    }
}

impl util::Tool for Invocation {
}
impl util::ToolInvocation for Invocation {
}

impl Default for Invocation {
    fn default() -> Invocation {
        Invocation {
            translate_pso: false,
            allow_bitcode_input: false,
            use_irt: true,
            use_irt_shim: true,

            use_stdlib: true,
            use_defaultlibs: true,

            fast_trans: false,

            eh_mode: Default::default(),
        }
    }
}

argument!(impl OUTPUT where { Some(r"^-o(.+)$"), Some(r"^-o$") } for Invocation {
    fn set_output(this, cap) {
        let index;
        if cap.at(0).unwrap()
    }
});

#![feature(plugin)]
#![plugin(regex_macros)]

use std::fmt;
use std::path::{Path, PathBuf};
use std::process;

use util::{Arch, CommandQueue};

pub use util::ldtools::Input;

extern crate regex;
#[macro_use] extern crate util;

extern crate pnacl_opt as opt;

const BASE_UNRESOLVED: &'static [&'static str] = &[
    // The following functions are implemented in the native support library.
    // Before a .pexe is produced, they get rewritten to intrinsic calls.
    // However, this rewriting happens after bitcode linking - so gold has
    // to be told that these are allowed to remain unresolved.
    "--allow-unresolved=memcpy",
    "--allow-unresolved=memset",
    "--allow-unresolved=memmove",
    "--allow-unresolved=setjmp",
    "--allow-unresolved=longjmp",

    // These TLS layout functions are either defined by the ExpandTls
    // pass or (for non-ABI-stable code only) by PNaCl's native support
    // code.
    "--allow-unresolved=__nacl_tp_tls_offset",
    "--allow-unresolved=__nacl_tp_tdb_offset",

    // __nacl_get_arch() is for non-ABI-stable code only.
    "--allow-unresolved=__nacl_get_arch",
    ];

const SJLJ_UNRESOLVED: &'static [&'static str] = &[
    // These symbols are defined by libsupc++ and the PNaClSjLjEH
    // pass generates references to them.
    "--undefined=__pnacl_eh_stack",
    "--undefined=__pnacl_eh_resume",

    // These symbols are defined by the PNaClSjLjEH pass and
    // libsupc++ refers to them.
    "--allow-unresolved=__pnacl_eh_type_table",
    "--allow-unresolved=__pnacl_eh_action_table",
    "--allow-unresolved=__pnacl_eh_filter_table",
    ];

const ZEROCOST_UNRESOLVED: &'static [&'static str] =
    &["--allow-unresolved=_Unwind_Backtrace",
      "--allow-unresolved=_Unwind_DeleteException",
      "--allow-unresolved=_Unwind_GetCFA",
      "--allow-unresolved=_Unwind_GetDataRelBase",
      "--allow-unresolved=_Unwind_GetGR",
      "--allow-unresolved=_Unwind_GetIP",
      "--allow-unresolved=_Unwind_GetIPInfo",
      "--allow-unresolved=_Unwind_GetLanguageSpecificData",
      "--allow-unresolved=_Unwind_GetRegionStart",
      "--allow-unresolved=_Unwind_GetTextRelBase",
      "--allow-unresolved=_Unwind_PNaClSetResult0",
      "--allow-unresolved=_Unwind_PNaClSetResult1",
      "--allow-unresolved=_Unwind_RaiseException",
      "--allow-unresolved=_Unwind_Resume",
      "--allow-unresolved=_Unwind_Resume_or_Rethrow",
      "--allow-unresolved=_Unwind_SetGR",
      "--allow-unresolved=_Unwind_SetIP",
      ];

const SPECIAL_LIBS: &'static [(&'static str, (&'static str, bool))] =
    &[("-lnacl", ("nacl_sys_private", true)),
      ("-lpthread", ("pthread_private", false)),
      ];

#[derive(Clone, Debug)]
pub struct Invocation {
    pub allow_native: bool,
    pub use_irt: bool,
    pub abi_check: bool,
    pub run_passes_separately: bool,
    pub relocatable: bool,
    pub use_stdlib: bool,
    pub use_defaultlibs: bool,
    pub pic: bool,
    pub allow_nexe_build_id: bool,
    pub static_: bool,

    pub optimize: util::OptimizationGoal,
    pub lto: bool,
    pub strip: util::StripMode,

    pub eh_mode: util::EhMode,

    pub arch: Option<Arch>,

    pub disabled_passes: Vec<String>,

    bitcode_inputs: Vec<Input>,
    native_inputs: Vec<Input>,
    has_native_inputs: bool,
    has_bitcode_inputs: bool,

    output: Option<PathBuf>,

    pub search_paths: Vec<PathBuf>,

    pub soname: Option<String>,

    ld_flags: Vec<String>,
    ld_flags_native: Vec<String>,

    trans_flags: Vec<String>,

    // detect mismatched --start-group && --end-group
    grouped: usize,
}

impl Default for Invocation {
    fn default() -> Invocation {
        Invocation {
            allow_native: false,
            use_irt: true,
            abi_check: true,
            run_passes_separately: false,
            relocatable: false,
            use_stdlib: true,
            use_defaultlibs: true,
            pic: false,
            allow_nexe_build_id: false,
            static_: true,

            optimize: Default::default(),
            lto: false,
            strip: Default::default(),

            eh_mode: Default::default(),

            arch: Default::default(),

            disabled_passes: Default::default(),

            bitcode_inputs: Default::default(),
            native_inputs: Default::default(),
            has_native_inputs: false,
            has_bitcode_inputs: false,

            output: Default::default(),

            search_paths: Default::default(),

            soname: Default::default(),

            ld_flags: Default::default(),
            ld_flags_native: Default::default(),

            trans_flags: Default::default(),

            grouped: 0,
        }
    }
}

impl Invocation {
    pub fn get_static(&self) -> bool {
        !self.relocatable
    }
    pub fn is_portable(&self) -> bool {
        self.get_arch() == util::Arch::Le32
    }

    pub fn get_arch(&self) -> util::Arch {
        self.arch.unwrap_or_default()
    }

    pub fn has_bitcode_inputs(&self) -> bool {
        self.has_bitcode_inputs
    }
    pub fn has_native_inputs(&self) -> bool {
        self.has_native_inputs
    }

    pub fn get_output(&self) -> PathBuf {
        self.output
            .clone()
            .unwrap_or_else(|| From::from("a.out") )
    }

    /// Add a non-flag input.
    pub fn add_input(&mut self, input: Input) -> Result<(), String> {
        use util::ldtools::*;
        let expanded = try!(extend_inputs(input));
        'outer: for input in expanded.into_iter() {
            'inner: loop {
                match &input {
                    &Input::Library(_, _, AllowedTypes::Any) => unreachable!(),
                    &Input::Library(_, ref name, ty) => {
                        if ty == AllowedTypes::Native {
                            try!(self.check_native_allowed());
                        }

                        let input_str = name.to_str();
                        if input_str.is_none() {
                            inputs.push(name.clone());
                            continue;
                        }
                        let input_str = input_str.unwrap();

                        let mut private_lib = None;
                        for i in SPECIAL_LIBS.iter() {
                            let &(public_name, (_, _)) = i;
                            if public_name == input_str {
                                private_lib = Some(i);
                                break;
                            }
                        }
                        if private_lib.is_some()
                    },
                    _ => (),
                }
                break;
            }

        }
        Ok(())
    }

    fn check_native_allowed(&self) -> Result<(), String> {
        if !self.allow_native {
            return Err("native is not allowed (use `--pnacl-allow-native`)".to_string());
        }
        Ok(())
    }

    pub fn add_native_ld_flag(&mut self, flag: &str) -> Result<(), String> {
        try!(self.check_native_allowed());

        self.ld_flags_native.push(flag.to_string());
        Ok(())
    }
    pub fn add_trans_flag(&mut self, flag: &str) -> Result<(), String> {
        try!(self.check_native_allowed());

        self.trans_flags.push(flag.to_string());
        Ok(())
    }
}

impl util::ToolInvocation for Invocation {
    fn check_state(&mut self, iteration: usize) -> Result<(), String> {
        use std::mem;
        use util::ldtools::*;
        match iteration {
            0 => {
                if self.allow_native && self.arch.is_none() {
                    return Err("`--pnacl-allow-native` given, but translation is not happening (missing `-target`?)".to_string());
                }

                if self.use_stdlib {
                    // add stdlib locations:
                    let base = util::need_nacl_toolchain();
                    let arch_subpath = self.get_arch().bc_subpath();
                    let base_usr_lib = base
                        .join(arch_subpath)
                        .join("usr/lib");
                    let base_lib = base
                        .join(arch_subpath)
                        .join("lib");
                    let base_clang_lib = base
                        .join("lib/clang")
                        .join(util::CLANG_VERSION)
                        .join("lib/le32-nacl");
                    self.search_paths.push(base_usr_lib);
                    self.search_paths.push(base_lib);
                    self.search_paths.push(base_clang_lib);
                }
            },
            1 => {
                if !self.has_native_inputs() && !self.has_bitcode_inputs() {
                    return Err("no inputs".to_string());
                }

                // Fix private libs:
                /// If not using the IRT or if private libraries are used:
                /// - Place private libraries that can coexist before their public
                ///   equivalent (keep both);
                /// - Replace public libraries that can't coexist with their private
                ///   equivalent.
                ///
                /// This occurs before path resolution (important because public/private
                /// libraries aren't always colocated) and assumes that -l:libfoo.a syntax
                /// isn't used by the driver for relevant libraries.
                fn fix_private_libs(invocation_inputs: &mut Vec<Input>,
                                    search_paths: &[PathBuf], static_only: bool) ->
                    Result<(), String>
                {
                    let mut inputs = Vec::new();
                    for input in {
                        let mut i: Vec<Input> = Vec::new();
                        mem::swap(invocation_inputs, &mut i);
                        i.into_iter()
                    }
                    {
                        {
                            let input_str = input.to_str();
                            if input_str.is_none() {
                                inputs.push(input.clone());
                                continue;
                            }
                            let input_str = input_str.unwrap();

                            let mut private_lib = None;
                            for i in SPECIAL_LIBS.iter() {
                                let &(public_name, (_, _)) = i;
                                if public_name == input_str {
                                    private_lib = Some(i);
                                    break;
                                }
                            }

                            if private_lib.is_some() {
                                let &(_, (private_name, can_coexist)) = private_lib.unwrap();
                                inputs.push(From::from(private_name));

                                if !can_coexist {
                                    continue;
                                }
                            }
                        }

                        let expanded = try!(expand_input(input));

                        inputs.push(input);
                    }

                    *invocation_inputs =
                        try!(expand_inputs(inputs.into_iter(),
                                           search_paths,
                                           static_only));
                    Ok(())
                }

                if self.allow_native {
                    try!(fix_private_libs(&mut self.native_inputs, self.search_paths.as_ref(),
                                          self.static_));
                }

                try!(fix_private_libs(&mut self.bitcode_inputs, self.search_paths.as_ref(),
                                      self.static_));

            },

            _ => unreachable!(),
        }

        Ok(())
    }
    fn args(&self, iteration: usize) -> Option<util::ToolArgs<Invocation>> {
        match iteration {
            0 => {
                static ARGS: util::ToolArgs<Invocation> =
                    &[&ALLOW_NATIVE,
                      &TARGET,
                      &SEARCH_PATH,
                      &NO_STDLIB,
                      ];
                Some(ARGS)
            },
            1 => {
                // The rest
                static ARGS: util::ToolArgs<Invocation> =
                    &[&NO_IRT_ARG,
                      &PNACL_EXCEPTIONS,
                      &PNACL_DISABLE_ABI_CHECK,
                      &PNACL_DISABLE_PASS,
                      &PNACL_RUN_PASSES_SEPARATELY,
                      &OUTPUT,
                      &STATIC,
                      &RELOCATABLE1,
                      &RELOCATABLE2,
                      &RELOCATABLE3,
                      &RPATH,
                      &RPATH_LINK,
                      &LINKER_SCRIPT,
                      &HYPHIN_E,
                      &VERSION_SCRIPT,
                      &NATIVE_FLAGS,
                      &SEGMENT,
                      &SECTION_START,
                      &BUILD_ID,
                      &TRANS_FLAGS,
                      &EXPORT_DYNAMIC,
                      &SONAME,
                      &PASSTHROUGH_BC_LINK_FLAGS1,
                      &PASSTHROUGH_BC_LINK_FLAGS2,
                      &PASSTHROUGH_BC_LINK_FLAGS3,
                      &PASSTHROUGH_BC_LINK_FLAGS4,
                      &PIC_FLAG,
                      &OPTIMIZE_FLAG,
                      &LTO_FLAG,
                      &FAST_TRANS_FLAG,
                      &STRIP_ALL_FLAG,
                      &STRIP_DEBUG_FLAG,
                      &LIBRARY,
                      &AS_NEEDED_FLAG,
                      &GROUP_FLAG,
                      &WHOLE_ARCHIVE_FLAG,
                      &LINKAGE_FLAG,
                      &UNDEFINED,
                      &UNSUPPORTED, // must be before INPUTS.
                      &INPUTS,
                      ];
                Some(ARGS)
            },
            _ => None,
        }
    }
}
impl util::Tool for Invocation {
    fn enqueue_commands(&mut self, queue: &mut CommandQueue) -> Result<(), String> {
        use util::EhMode;

        if self.has_bitcode_inputs() {
            let bc_ld_bin = util::get_bin_path("le32-nacl-ld.gold");
            let mut cmd = process::Command::new(bc_ld_bin);
            cmd.args(&["--oformat",
                      self.get_arch().bcld_output_format()]);

            if !self.relocatable {
                cmd.arg("--undef-sym-check");
                cmd.args(BASE_UNRESOLVED);

                match self.eh_mode {
                    EhMode::None => {},
                    EhMode::SjLj => {
                        cmd.args(SJLJ_UNRESOLVED);
                    },
                    EhMode::Zerocost => {
                        cmd.args(ZEROCOST_UNRESOLVED);
                    },
                }
            }

            for path in self.search_paths.iter() {
                debug_assert!(!path.starts_with("-L"));

                cmd.arg(format!("-L{}", path.display()));
            }

            if self.static_ { cmd.arg("-static"); }
            if self.relocatable { cmd.arg("-relocatable"); }
            if let Some(ref soname) = self.soname {
                cmd.arg(format!("--soname={}", soname));
            }

            cmd.args(self.ld_flags.as_ref());
            cmd.args(self.bitcode_inputs.as_ref());

            queue.enqueue_external(Some("link"), cmd, Some("-o"), false);


            let abi_simplify = self.static_ && !self.has_native_inputs &&
                self.eh_mode != EhMode::Zerocost && !self.allow_nexe_build_id &&
                self.is_portable();
            let need_expand_byval = self.is_portable() && self.static_;
            let need_expand_varargs = need_expand_byval && !self.has_native_inputs;

            let mut args = Vec::new();

            // Do not serialize use lists into the (non-finalized) pexe. See
            // https://code.google.com/p/nativeclient/issues/detail?id=4190
            args.push("-preserve-bc-uselistorder=false");

            let mut passes = Vec::new();
            passes.push(format!("{}", self.strip));

            if abi_simplify {
                passes.push("-pnacl-abi-simplify-preopt".to_string());
                if self.eh_mode == EhMode::SjLj {
                    args.push("-enable-pnacl-sjlj-eh");
                } else {
                    debug_assert!(self.eh_mode == EhMode::None);
                }
            } else {
                if self.eh_mode != EhMode::Zerocost {
                    passes.push("-lowerinvoke".to_string());
                    passes.push("-simplifycfg".to_string());
                }

                if need_expand_varargs {
                    passes.push("-expand-varargs".to_string());
                }
            }

            passes.push(format!("{}", self.optimize));

            let do_lto = match self.optimize {
                util::OptimizationGoal::Speed(n) if n >= 2 => true,
                util::OptimizationGoal::Balanced |
                util::OptimizationGoal::Size => true,
                _ => false,
            };
            let do_lto = do_lto || self.lto;
            if do_lto {
                let do_inlining = match self.optimize {
                    util::OptimizationGoal::Balanced |
                    util::OptimizationGoal::Size => false,
                    _ => true,
                };

                passes.push("-ipsccp".to_string());
                passes.push("-globalopt".to_string());
                passes.push("-constmerge".to_string());
                passes.push("-deadargelim".to_string());
                if do_inlining {
                    passes.push("-inline".to_string());
                }
                passes.push("-prune-eh".to_string());
                if do_inlining {
                    passes.push("-globalopt".to_string());
                }
                passes.push("-argpromotion".to_string());
                passes.push("-instcombine".to_string());
                passes.push("-jump-threading".to_string());
                // Note: no SROA. Not needed since the IR simplification passes remove
                // aggregate types anyway.
                passes.push("-functionattrs".to_string());
                passes.push("-globalsmodref-aa".to_string());
                passes.push("-licm".to_string());
                passes.push("-gvn".to_string());
                passes.push("-memcpyopt".to_string());
                passes.push("-dse".to_string());
                passes.push("-indvars".to_string());
                passes.push("-loop-deletion".to_string());
                passes.push("-alignment-from-assumptions".to_string());
                passes.push("-instcombine".to_string());
                passes.push("-jump-threading".to_string());
                passes.push("-simplifycfg".to_string());
                passes.push("-global-dce".to_string());
                passes.push("-mergefunc".to_string());
            }

            if abi_simplify {
                passes.push("-pnacl-abi-simplify-postopt".to_string());
                if self.abi_check {
                    passes.push("-verify-pnaclabi-module".to_string());
                    passes.push("-verify-pnaclabi-functions".to_string());
                    passes.push("-pnaclabi-allow-debug-metadata".to_string());
                }
            } else if need_expand_byval {
                passes.push("-expand-byval".to_string());
            }

            if self.run_passes_separately {
                for pass in passes.into_iter() {
                    let opt: opt::Invocation = Default::default();

                    let mut opt_args: Vec<String> = args.iter()
                        .map(|arg| arg.to_string() )
                        .collect();
                    // remove the opening hyphen:
                    let name = pass[1..].to_string();
                    opt_args.push(pass);
                    try!(queue.enqueue_tool(Some(name), opt, opt_args, true));
                }
            } else {
                passes.extend(args.into_iter().map(|arg| arg.to_string() ));
                let opt: opt::Invocation = Default::default();
                try!(queue.enqueue_tool(Some("optimizations".to_string()), opt, passes,
                                        true))
            }
        }

        if self.has_native_inputs {

        }

        Ok(())
    }

    fn get_name(&self) -> String { From::from("pnacl-ld") }

    fn get_output(&self) -> Option<&PathBuf> { self.output.as_ref() }
    fn override_output(&mut self, out: PathBuf) { self.output = Some(out); }
}

type ToolArg = util::ToolArg<Invocation>;
type ToolArgActionFn = util::ToolArgActionFn<Invocation>;

static ALLOW_NATIVE: ToolArg = util::ToolArg {
    single: Some(regex!(r"^--pnacl-allow-native$")),
    split: None,
    action: Some(set_allow_native as ToolArgActionFn),
};
fn set_allow_native<'str>(this: &mut Invocation, _single: bool, _: regex::Captures) -> Result<(), String> {
    this.allow_native = true;
    Ok(())
}

static NO_IRT_ARG: ToolArg = util::ToolArg {
    single: Some(regex!(r"^--noirt$")),
    split: None,
    action: Some(set_noirt as ToolArgActionFn)
};
fn set_noirt<'str>(this: &mut Invocation, _single: bool, _: regex::Captures) -> Result<(), String> {
    this.use_irt = false;
    Ok(())
}
static PNACL_DISABLE_ABI_CHECK: ToolArg = util::ToolArg {
    single: Some(regex!(r"^--pnacl-allow-nexe-build-id$")),
    split: None,
    action: Some(set_pnacl_disable_abi_check as ToolArgActionFn),
};
fn set_pnacl_disable_abi_check<'str>(this: &mut Invocation, _single: bool, _: regex::Captures) -> Result<(), String> {
    this.abi_check = false;
    Ok(())
}

tool_argument!(PNACL_EXCEPTIONS: Invocation = {
    r"^(--pnacl-exceptions=(none|sjlj|zerocost)|--pnacl-allow-exceptions)$", None
};
               fn set_eh_mode(this, _single, cap) {
                   this.eh_mode = try!(util::EhMode::parse_arg(cap.at(0).unwrap()).unwrap());
                   if this.eh_mode == util::EhMode::Zerocost {
                       try!(this.check_native_allowed());
                   }
                   Ok(())
               });

tool_argument!(PNACL_DISABLE_PASS: Invocation = { r"^--pnacl-disable-pass=(.+)$", None };
               fn add_disabled_pass(this, _single, cap) {
                   this.disabled_passes.push(cap.at(1).unwrap().to_string());
                   Ok(())
               });
tool_argument!(PNACL_RUN_PASSES_SEPARATELY: Invocation = { r"--pnacl-run-passes-separately", None };
               fn set_run_passes_separately(this, _single, _cap) {
                   this.run_passes_separately = true;
                   Ok(())
               });
tool_argument!(TARGET: Invocation = { r"--target=(.+)", Some(regex!(r"-target")) };
               fn set_target(this, single, cap) {
                   if this.arch.is_some() {
                       return Err("the target has already been set".to_string());
                   }
                   let arch = if single { cap.at(1).unwrap() }
                              else      { cap.at(0).unwrap() };
                   let arch = try!(util::Arch::parse_from_triple(arch));
                   this.arch = Some(arch);
                   Ok(())
               });
tool_argument!(OUTPUT: Invocation = { r"-o(.+)", Some(regex!(r"-(o|-output)")) };
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
tool_argument!(STATIC: Invocation = { r"-static", None };
               fn set_static(this, _single, _cap) {
                   if !this.relocatable {
                       this.static_ = true;
                   } else {
                       this.static_ = false;
                   }
                   Ok(())
               });
static RELOCATABLE1: ToolArg = util::ToolArg {
    single: Some(regex!(r"-r")),
    split: None,
    action: Some(set_relocatable as ToolArgActionFn),
};
static RELOCATABLE2: ToolArg = util::ToolArg {
    single: Some(regex!(r"-relocatable")),
    split: None,
    action: Some(set_relocatable as ToolArgActionFn),
};
static RELOCATABLE3: ToolArg = util::ToolArg {
    single: Some(regex!(r"-i")),
    split: None,
    action: Some(set_relocatable as ToolArgActionFn),
};
fn set_relocatable<'str>(this: &mut Invocation, _single: bool,
                         _: regex::Captures) -> Result<(), String> {
    this.relocatable = true;
    this.static_ = false;
    Ok(())
}

tool_argument!(SEARCH_PATH: Invocation = { r"^-L(.+)$", Some(regex!(r"^-(L|-library-path)$")) };
               fn add_search_path(this, single, cap) {
                   let path = if single { cap.at(1).unwrap() }
                              else      { cap.at(0).unwrap() };
                   let path = Path::new(path);
                   this.search_paths.push(path.to_path_buf());
                   Ok(())
               });
tool_argument!(RPATH: Invocation = { r"^-rpath=(.*)$", Some(regex!(r"^-rpath$")) });
tool_argument!(RPATH_LINK: Invocation = { r"^-rpath-link=(.*)$", Some(regex!(r"^-rpath-link$")) });

fn add_to_native_link_flags(this: &mut Invocation, _single: bool,
                            cap: regex::Captures) -> Result<(), String> {
    this.add_native_ld_flag(cap.at(0).unwrap())
}
fn add_to_bc_link_flags(this: &mut Invocation, _single: bool,
                        cap: regex::Captures) -> Result<(), String> {
    this.ld_flags.push(cap.at(0).unwrap().to_string());
    Ok(())
}
fn add_to_both_link_flags(this: &mut Invocation, _single: bool,
                          cap: regex::Captures) -> Result<(), String> {
    let flag = cap.at(0).unwrap().to_string();
    this.ld_flags.push(flag.clone());
    this.add_native_ld_flag(&flag[..])
}

static LINKER_SCRIPT: ToolArg = util::ToolArg {
    single: None,
    split: Some(regex!(r"^(-T)$")),
    action: Some(add_to_native_link_flags as ToolArgActionFn),
};
/// TODO(pdox): Allow setting an alternative _start symbol in bitcode
static HYPHIN_E: ToolArg = util::ToolArg {
    single: None,
    split: Some(regex!(r"^(-e)$")),
    action: Some(add_to_both_link_flags as ToolArgActionFn),
};

/// TODO(pdox): Support GNU versioning.
tool_argument!(VERSION_SCRIPT: Invocation = { r"^--version-script=.*$", None });

tool_argument!(NATIVE_FLAGS: Invocation = { r"^-Wn,(.*)$", None };
               fn add_native_flags(this, _single, cap) {
                   let args = cap.at(1).unwrap();
                   for arg in args.split(',') {
                       try!(this.add_native_ld_flag(arg));
                   }
                   Ok(())
               });

static SEGMENT: ToolArg = util::ToolArg {
    single: Some(regex!(r"^(-T(text|rodata)-segment=.*)$")),
    split: None,
    action: Some(add_to_native_link_flags as ToolArgActionFn),
};
static SECTION_START: ToolArg = util::ToolArg {
    single: None,
    split: Some(regex!(r"^--section-start$")),
    action: Some(section_start as ToolArgActionFn),
};
fn section_start(this: &mut Invocation,
                 _single: bool,
                 cap: regex::Captures) -> Result<(), String> {
    try!(this.add_native_ld_flag("--section-start"));
    this.add_native_ld_flag(cap.at(1).unwrap())
}
static BUILD_ID: ToolArg = util::ToolArg {
    single: None,
    split: Some(regex!(r"^--build-id$")),
    action: Some(build_id as ToolArgActionFn),
};
fn build_id<'str>(this: &mut Invocation,
                  _single: bool,
                  _cap: regex::Captures) -> Result<(), String> {
    this.add_native_ld_flag("--build-id")
}

tool_argument!(TRANS_FLAGS: Invocation = { r"^-Wt,(.*)$", None };
               fn add_trans_flags(this, _single, cap) {
                   let args = cap.at(1).unwrap();
                   for arg in args.split(',') {
                       try!(this.add_trans_flag(arg));
                   }
                   Ok(())
               });

/// NOTE: -export-dynamic doesn't actually do anything to the bitcode link
/// right now. This is just in case we do want to record that in metadata
/// eventually, and have that influence the native linker flags.
static EXPORT_DYNAMIC: ToolArg = util::ToolArg {
    single: Some(regex!(r"(-export-dynamic)")),
    split: None,
    action: Some(add_to_bc_link_flags as ToolArgActionFn),
};

tool_argument!(SONAME: Invocation = { r"-?-soname=(.+)", Some(regex!(r"-?-soname")) };
               fn set_soname(this, _single, cap) {
                   if this.soname.is_some() {
                       return Err("the shared object name has already been set".to_string());
                   }

                   this.soname = Some(cap.at(1).unwrap().to_string());
                   Ok(())
               });

argument!(impl PASSTHROUGH_BC_LINK_FLAGS1 where { Some(r"(-M|--print-map|-t|--trace)"), None } for Invocation
          => Some(add_to_bc_link_flags));

static PASSTHROUGH_BC_LINK_FLAGS2: ToolArg = util::ToolArg {
    single: None,
    split: Some(regex!(r"-y")),
    action: Some(passthrough_bc_link_flags2 as ToolArgActionFn),
};
fn passthrough_bc_link_flags2<'str>(this: &mut Invocation,
                                    _single: bool,
                                    cap: regex::Captures) -> Result<(), String> {
    this.ld_flags.push("-y".to_string());
    this.ld_flags.push(cap.at(1).unwrap().to_string());
    Ok(())
}
static PASSTHROUGH_BC_LINK_FLAGS3: ToolArg = util::ToolArg {
    single: None,
    split: Some(regex!(r"-defsym")),
    action: Some(passthrough_bc_link_flags3 as ToolArgActionFn),
};
fn passthrough_bc_link_flags3<'str>(this: &mut Invocation,
                                    _single: bool,
                                    cap: regex::Captures) -> Result<(), String> {
    this.ld_flags.push("-defsym".to_string());
    this.ld_flags.push(cap.at(0).unwrap().to_string());
    Ok(())
}
static PASSTHROUGH_BC_LINK_FLAGS4: ToolArg = util::ToolArg {
    single: Some(regex!(r"^-?-wrap=(.+)$")),
    split: Some(regex!(r"^-?-wrap$")),
    action: Some(passthrough_bc_link_flags4 as ToolArgActionFn),
};
fn passthrough_bc_link_flags4<'str>(this: &mut Invocation,
                                    _single: bool,
                                    cap: regex::Captures) -> Result<(), String> {
    this.ld_flags.push("-wrap".to_string());
    this.ld_flags.push(cap.at(0).unwrap().to_string());
    Ok(())
}

tool_argument!(PIC_FLAG: Invocation = { r"^-fPIC$", None };
               fn set_pic(this, _single, _cap) {
                   this.pic = true;
                   Ok(())
               });

tool_argument!(OPTIMIZE_FLAG: Invocation = { r"^-O([0-4sz]?)$", None };
               fn set_optimize(this, _single, cap) {
                   this.optimize = cap.at(0)
                       .and_then(|str| util::OptimizationGoal::parse(str) )
                       .unwrap();
                   Ok(())
               });

tool_argument!(FAST_TRANS_FLAG: Invocation = { r"^(-translate-fast)$", None };
               fn set_trans_fast(this, _single, cap) {
                   this.add_trans_flag(cap.at(0).unwrap())
               });

tool_argument!(STRIP_ALL_FLAG: Invocation = { r"^(-s|--strip-all)$", None };
               fn set_strip_all(this, _single, _cap) {
                   this.strip = util::StripMode::All;
                   Ok(())
               });

tool_argument!(STRIP_DEBUG_FLAG: Invocation = { r"^(-S|--strip-debug)$", None };
               fn set_strip_debug(this, _single, _cap) {
                   this.strip = util::StripMode::Debug;
                   Ok(())
               });

tool_argument!(LIBRARY: Invocation = { r"^-l(.+)$", Some(regex!(r"^-(l|-library)$")) };
               fn add_library(this, _single, cap) {
                   this.add_input(Input::Library(From::from(cap.at(1).unwrap())))
               });

fn add_input_flag<'str>(this: &mut Invocation,
                        _single: bool,
                        cap: regex::Captures) -> Result<(), String> {
    this.add_input(Input::Flag(From::from(cap.at(0).unwrap())));
    Ok(())
}

static AS_NEEDED_FLAG: ToolArg = util::ToolArg {
    single: Some(regex!(r"^(-(-no)?-as-needed)$")),
    split: None,
    action: Some(add_input_flag as ToolArgActionFn),
};
static GROUP_FLAG: ToolArg = util::ToolArg {
    single: Some(regex!(r"^(--(start|end)-group)$")),
    split: None,
    action: Some(add_input_flag as ToolArgActionFn),
};
static WHOLE_ARCHIVE_FLAG: ToolArg = util::ToolArg {
    single: Some(regex!(r"^(-?-(no-)whole-archive)$")),
    split: None,
    action: Some(add_input_flag as ToolArgActionFn),
};
static LINKAGE_FLAG: ToolArg = util::ToolArg {
    single: Some(regex!(r"^(-B(static|dynamic))$")),
    split: None,
    action: Some(add_input_flag as ToolArgActionFn),
};

tool_argument!(UNDEFINED: Invocation = { r"^-(-undefined=|u)(.+)$", Some(regex!(r"^-u$")) };
               fn add_undefined(this, single, cap) {
                   let sym = if single { cap.at(2).unwrap() }
                             else { cap.at(1).unwrap() };

                   this.add_input_flag(From::from(format!("--undefined={}", sym)));
                   Ok(())
               });


tool_argument!(LTO_FLAG: Invocation = { r"^-flto$", None };
               fn set_lto(this, _single, _cap) {
                   this.lto = true;
                   Ok(())
               });

argument!(impl UNSUPPORTED where { Some(r"^-.+$"), None } for Invocation {
    fn unsupported_flag(_this, _single, _cap) {
        return Err("unsupported argument".to_string());
    }
});

argument!(impl NO_STDLIB where { Some(r"^-nostdlib$"), None } for Invocation {
    fn no_stdlib(this, _single, _cap) {
        this.use_stdlib = false;
    }
});

argument!(impl NO_DEFAULTLIBS where { Some(r"^-nodefaultlibs$"), None } for Invocation {
    fn no_defaultlib(this, _single, _cap) {
        this.use_defaultlibs = false;
    }
});

tool_argument!(INPUTS: Invocation = { r"^(.+)$", None };
               fn add_input(this, _single, cap) {
                   this.add_input(Input::File(From::from(cap.at(1).unwrap())))
               });

#[cfg(test)] #[allow(unused_imports)]
mod tests {
    use util;
    use util::*;
    use util::filetype::*;
    use super::*;

    use std::path::{PathBuf, Path};

    #[test]
    fn unsupported_flag() {
        let args = vec!["-unsupported-flag".to_string()];
        let mut i: Invocation = Default::default();

        assert!(util::process_invocation_args(&mut i, args).is_err());
    }

    #[test]
    fn group_flags0() {
        use util::filetype::*;

        override_filetype("libsome.a", Type::Archive(Subtype::Bitcode));
        override_filetype("input.bc", Type::Object(Subtype::Bitcode));

        let args = vec!["input.bc".to_string(),
                        "--start-group".to_string(),
                        "-lsome".to_string(),
                        "--end-group".to_string()];
        let mut i: Invocation = Default::default();
        i.search_paths.push(From::from("."));
        let res = util::process_invocation_args(&mut i, args);

        println!("{:?}", i);

        res.unwrap();

        assert!(i.search_paths.contains(&From::from(".")));
        assert!(&i.bitcode_inputs[1..] == &[Path::new("--start-group").to_path_buf(),
                                            Path::new("-lsome").to_path_buf(),
                                            Path::new("--end-group").to_path_buf()]);
        assert!(&i.native_inputs[..] == &[Path::new("--start-group").to_path_buf(),
                                          Path::new("--end-group").to_path_buf()]);

    }

    #[test]
    fn group_flags1() {
        override_filetype("libsome.a", Type::Archive(Subtype::ELF(elf::types::Machine(0))));
        override_filetype("input.o", Type::Object(Subtype::ELF(elf::types::Machine(0))));

        let args = vec!["input.o".to_string(),
                        "--start-group".to_string(),
                        "-lsome".to_string(),
                        "--end-group".to_string()];
        let mut i: Invocation = Default::default();
        i.allow_native = true;
        i.arch = Some(util::Arch::X8664);
        i.search_paths.push(From::from("."));
        let res = util::process_invocation_args(&mut i, args);

        println!("{:?}", i);

        res.unwrap();

        assert_eq!(&i.bitcode_inputs[1..], &[Path::new("--start-group").to_path_buf(),
                                             Path::new("--end-group").to_path_buf()]);

        assert_eq!(&i.native_inputs[..], &[Path::new("--start-group").to_path_buf(),
                                           Path::new("-lsome").to_path_buf(),
                                           Path::new("--end-group").to_path_buf()]);

    }

    #[test]
    fn input_arguments_bitcode() {
        override_filetype("input0.bc", Type::Object(Subtype::Bitcode));
        override_filetype("input1.bc", Type::Object(Subtype::Bitcode));

        let args = vec!["input0.bc".to_string(),
                        "input1.bc".to_string()];
        let mut i: Invocation = Default::default();
        util::process_invocation_args(&mut i, args).unwrap();

        println!("{:?}", i);

        assert!(&i.bitcode_inputs[..] == &[Path::new("input0.bc").to_path_buf(),
                                           Path::new("input1.bc").to_path_buf()]);
    }

    #[test]
    fn native_needs_targets() {
        let args = vec!["--pnacl-allow-native".to_string()];
        let mut i: Invocation = Default::default();
        let res = util::process_invocation_args(&mut i, args);
        println!("{:?}", i);
        assert!(res.is_err());


        override_filetype("input.o", Type::Object(Subtype::Bitcode));
        let args = vec!["input.o".to_string(),
                        "--pnacl-allow-native".to_string(),
                        "--target=arm-nacl".to_string()];
        let mut i: Invocation = Default::default();
        let res = util::process_invocation_args(&mut i, args);
        println!("{:?}", i);
        res.unwrap();

    }

    #[test]
    fn native_disallowed() {
        override_filetype("input.o", Type::Object(Subtype::ELF(elf::types::Machine(0))));

        let args = vec!["input.o".to_string()];
        let mut i: Invocation = Default::default();

        let res = util::process_invocation_args(&mut i, args);
        println!("{:?}", i);
        assert!(res.is_err());
    }
    #[test]
    fn no_inputs() {
        let args = vec![];
        let mut i: Invocation = Default::default();
        let res = util::process_invocation_args(&mut i, args);
        println!("{:?}", i);
        assert!(res.is_err());
    }
}

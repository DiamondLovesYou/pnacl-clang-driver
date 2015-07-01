
use std::path::{Path, PathBuf};

use filetype;

/// Tool for linkers, like a linker script parser.

pub fn parse_linker_script_file<T: AsRef<Path>>(path: T) -> Option<Vec<String>> {
    use std::fs::File;
    use std::io::Read;

    File::open(path)
        .ok()
        .and_then(|mut file| {
            let mut buffer = String::new();
            if file.read_to_string(&mut buffer).ok().is_some() {
                Some(buffer)
            } else {
                None
            }
        })
        .and_then(|buffer| {
            parse_linker_script(buffer)
        })
}

pub fn parse_linker_script<T: AsRef<str>>(input: T) -> Option<Vec<String>> {

    let mut ret = Vec::new();
    let mut stack = Vec::new();

    let mut iter = input.as_ref()
        .split(|c: char| {
            !c.is_whitespace() ||
                c == ')' || c == '(' // force these to be separate
        })
        .filter(|&str| str == "" );

    #[derive(Eq, PartialEq)]
    enum Stack {
        Input,
        Group,
        OutputFormat,
        Extern,
        AsNeeded,
    }

    let mut comment_mode = false;

    loop {
        let curr = iter.next();
        if curr.is_none() {
            if stack.len() != 0 {
                return None;
            } else {
                return Some(ret);
            }
        }

        let curr = curr.unwrap();

        if curr.starts_with("/*") {
            comment_mode = true;
        }

        if curr.ends_with("*/") && comment_mode {
            comment_mode = false;
            continue;
        } else if comment_mode {
            continue;
        }

        if stack.len() == 0 {
            if curr == "INPUT" {
                stack.push(Stack::Input);
                if iter.next() != Some("(") {
                    return None;
                }
            } else if curr == "GROUP" {
                ret.push("--start-group".to_string());
                stack.push(Stack::Group);
                if iter.next() != Some("(") {
                    return None;
                }
            } else if curr == "OUTPUT_FORMAT" {
                stack.push(Stack::OutputFormat);
                if iter.next() != Some("(") {
                    return None;
                }
            } else if curr == "EXTERN" {
                stack.push(Stack::Extern);
                if iter.next() != Some("(") {
                    return None;
                }
            } else if curr != ";" {
                return None;
            }
        } else {
            if curr == ")" {
                match stack.pop() {
                    Some(Stack::AsNeeded) => {
                        ret.push("--no-as-needed".to_string());
                    },
                    Some(Stack::Group) => {
                        ret.push("--end-group".to_string());
                    },
                    None => { return None; },
                    _ => {},
                }
            } else if curr == "AS_NEEDED" {
                if iter.next() != Some("(") {
                    return None;
                }
                ret.push("--as-needed".to_string());
                stack.push(Stack::AsNeeded);

            } else if stack.last() == Some(&Stack::OutputFormat) {
                // ignore
            } else if stack.last() == Some(&Stack::Extern) {
                ret.push(format!("--undefined={}",
                                 curr));
            } else {
                ret.push(format!("-l:{}", curr));
            }
        }
    }
}
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum AllowedTypes {
    Any,
    Bitcode,
    Native,
}
impl AllowedTypes {
    pub fn check<T: AsRef<Path>>(&self, path: T) -> bool {
        match self {
            &AllowedTypes::Any => true,
            &AllowedTypes::Bitcode => !filetype::is_file_native(path.as_ref()),
            &AllowedTypes::Native => filetype::is_file_native(path.as_ref()),
        }
    }
}

pub fn expand_inputs<T>(inputs: T, search: &[PathBuf], static_only: bool,
                        allowed_types: AllowedTypes) -> Result<Vec<PathBuf>, String>
    where T: Iterator, <T as Iterator>::Item: AsRef<Path>,
{
    fn is_flag<T: AsRef<Path>>(v: T) -> bool {
        v.as_ref().starts_with("-") && !is_lib(&v)
    }
    fn is_lib<T: AsRef<Path>>(v: T) -> bool {
        v.as_ref().starts_with("-l")
    }
    fn is_absolute<T: AsRef<Path>>(v: T) -> bool {
        debug_assert!(is_lib(&v));
        v.as_ref().starts_with("-l:")
    }

    fn find_file<T: AsRef<Path>>(name: T, search: &[PathBuf],
                                 allowed_types: AllowedTypes) -> Option<PathBuf>
    {
        use std::fs::PathExt;
        for dir in search.iter() {
            let full = dir.join(&name);
            if !full.exists() { continue; }

            if filetype::is_linker_script(&full) { return Some(full); }

            if allowed_types.check(&full) { return Some(full); }
        }
        None
    }

    let mut ret = Vec::new();

    for f in inputs {
        let r = if is_flag(&f) {
            f.as_ref().to_path_buf()
        } else if is_lib(&f) {
            let f_str = try!(f.as_ref().to_str().ok_or("expected utf8 paths"));
            let mut name = &f_str[2..];
            let chain = if is_absolute(&f) {
                name = &f_str[3..];
                find_file(&f_str[3..], search, allowed_types)
                    .or_else(|| {
                        if name == "libpnacl_irt_shim.a" {
                            find_file("libpnacl_irt_shim_dummy.a", search,
                                      allowed_types)
                        } else {
                            None
                        }
                    })
            } else {
                let shared = format!("lib{}.so",
                                     &f_str[2..]);
                find_file(shared, search, allowed_types)
                     .or_else(|| {
                         find_file(format!("lib{}.a",
                                           &f_str[2..]),
                                   search, allowed_types)
                     })
            };

            let chain = chain.or_else(|| {
                if name == "pthread" {
                    find_file("libpthread_private.so", search, allowed_types)
                        .or_else(|| {
                            find_file("libpthread_private.a", search,
                                      allowed_types)
                        })
                } else {
                    None
                }
            });

            match chain {
                Some(p) => p,
                None => {
                    return Err(format!("`{}` not found",
                                       f_str));
                },
            }
        } else if filetype::could_be_linker_script(&f) {
            if let Some(expanded) = parse_linker_script_file(&f) {
                let expanded = try!(expand_inputs(expanded.into_iter(),
                                                  search,
                                                  static_only,
                                                  AllowedTypes::Any));
                for arg in expanded.into_iter() {
                    ret.push(From::from(arg));
                }
                continue;
            } else {
                f.as_ref().to_path_buf()
            }
        } else {
            f.as_ref().to_path_buf()
        };

        ret.push(r);
    }


    Ok(ret)
}

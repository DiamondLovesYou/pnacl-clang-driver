
use std::fmt;
use std::path::{Path, PathBuf};

use filetype;

/// Tool for linkers, like a linker script parser.

#[derive(Clone, Debug)]
pub enum Input {
  Library(bool, PathBuf, AllowedTypes),
  File(PathBuf),
  Flag(String),
}

impl fmt::Display for Input {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      &Input::Library(false, ref p, _) => write!(f, "-l{}", p.display()),
      &Input::Library(true, ref p, _) => write!(f, "-l:{}", p.display()),
      &Input::File(ref p) => write!(f, "{}", p.display()),
      &Input::Flag(ref flag) => write!(f, "{}", flag),
    }
  }
}

pub fn parse_linker_script_file<T: AsRef<Path>>(path: T) -> Option<Vec<Input>> {
  use std::fs::File;
  use std::io::Read;

  File::open(path.as_ref())
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
      let dir = path.as_ref().parent().unwrap_or_else(|| ".".as_ref() );
      parse_linker_script(buffer, dir)
    })
}

pub fn parse_linker_script<T, U>(input: T, dir: U)
  -> Option<Vec<Input>>
  where T: AsRef<str>,
        U: AsRef<Path>,
{

  let mut ret = Vec::new();
  let mut stack = Vec::new();

  struct Tokenizer<'a>(&'a str);
  impl<'a> Iterator for Tokenizer<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
      let mut skip_whitespace = 0;
      {
        let mut ci = self.0.char_indices().peekable();
        loop {
          if let Some(&(byte, c)) = ci.peek() {
            if c.is_whitespace() {
              skip_whitespace = byte + c.len_utf8();
              ci.next();
            } else {
              break;
            }
          } else {
            return None;
          }
        }
      }
      self.0 = &self.0[skip_whitespace..];


      let mut token_end = 0;
      {
        let mut ci = self.0.char_indices();
        'outer: loop {
          if let Some((byte, c)) = ci.next() {
            token_end = byte;
            if c.is_whitespace() {
              break;
            } else if c == '(' || c == ')' {
              if byte == 0 {
                token_end = 1;
              }
              break;
            }
          } else {
            break;
          }
        }
      }

      let next = &self.0[..token_end];
      if next.len() == 0 { return None; }

      self.0 = &self.0[token_end..];
      Some(next)
    }
  }

  let mut tokens = Tokenizer(input.as_ref());

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
    let curr = tokens.next();
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
        if tokens.next() != Some("(") {
          return None;
        }
      } else if curr == "GROUP" {
        ret.push(Input::Flag("--start-group".to_string()));
        stack.push(Stack::Group);
        if tokens.next() != Some("(") {
          return None;
        }
      } else if curr == "OUTPUT_FORMAT" {
        stack.push(Stack::OutputFormat);
        if tokens.next() != Some("(") {
          return None;
        }
      } else if curr == "EXTERN" {
        stack.push(Stack::Extern);
        if tokens.next() != Some("(") {
          return None;
        }
      } else if curr != ";" {
        return None;
      }
    } else {
      if curr == ")" {
        match stack.pop() {
          Some(Stack::AsNeeded) => {
            ret.push(Input::Flag("--no-as-needed".to_string()));
          },
          Some(Stack::Group) => {
            ret.push(Input::Flag("--end-group".to_string()));
          },
          None => { return None; },
          _ => {},
        }
      } else if curr == "AS_NEEDED" {
        if tokens.next() != Some("(") {
          return None;
        }
        ret.push(Input::Flag("--as-needed".to_string()));
        stack.push(Stack::AsNeeded);

      } else if stack.last() == Some(&Stack::OutputFormat) {
        // ignore
      } else if stack.last() == Some(&Stack::Extern) {
        ret.push(Input::Flag(format!("--undefined={}", curr)));
      } else if curr.starts_with("-l") {
        ret.push(Input::Flag(curr.into()));
      } else {
        let file = dir.as_ref().join(curr);
        ret.push(Input::Library(true, file, AllowedTypes::Any));
      }
    }
  }
}
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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

pub fn expand_input(input: Input, search: &[PathBuf],
                    static_only: bool) -> Result<Vec<Input>, String> {

  fn find_file<T: AsRef<Path>>(name: T, search: &[PathBuf],
                               allowed_types: AllowedTypes) -> Option<PathBuf> {
    for dir in search.iter() {
      let full = dir.join(&name);
      if !full.exists() { continue; }

      if filetype::is_linker_script(&full) { return Some(full); }

      if allowed_types.check(&full) { return Some(full); }
    }
    None
  }

  let mut ret = Vec::new();

  let r = match input {
    Input::Flag(f) => Input::Flag(f),
    Input::Library(is_absolute, path, allowed_types) => {
      let chain = if is_absolute {
        find_file(&path, search, allowed_types)
      } else {
        if !static_only {
          find_file(format!("lib{}.so", path.display()),
                    search, allowed_types)
            .or_else(|| {
              find_file(format!("lib{}.a", path.display()), search,
                        allowed_types)
            })
        } else {
          find_file(format!("lib{}.a", path.display()), search,
                    allowed_types)
        }
      };

      match chain {
        Some(p) => {
          if filetype::could_be_linker_script(&p) {
            if let Some(expanded) = parse_linker_script_file(&p) {
              for arg in expanded.into_iter() {
                ret.extend(expand_input(arg, search,
                                        static_only)?);
              }
              return Ok(ret);
            }
          }
          let t = if !filetype::is_file_native(&p) {
            AllowedTypes::Bitcode
          } else {
            AllowedTypes::Native
          };
          Input::Library(true, p, t)
        },
        None => {
          return Err(format!("`{}{}` not found",
                             if is_absolute { ":" } else { "" },
                             path.display()));
        },
      }
    },
    Input::File(path) => {
      if filetype::could_be_linker_script(&path) {
        if let Some(expanded) = parse_linker_script_file(&path) {
          for arg in expanded.into_iter() {
            ret.extend(expand_input(arg, search,
                                    static_only)?);
          }
          return Ok(ret);
        } else {
          Input::File(path)
        }
      } else {
        Input::File(path)
      }
    },
  };

  ret.push(r);

  Ok(ret)
}

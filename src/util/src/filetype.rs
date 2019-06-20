
use std::collections::HashMap;
use std::io::{self, Read, Seek, Cursor};
use std::path::{Path, PathBuf};
use std::sync::{self, Arc, Mutex};

use ldtools;


// Rust goes into anaphylaxic shock on mutable globals which require
// drops. This backflip is to that effect. The pointer is never deallocated.
static FILETYPE_CACHE_START: sync::Once = sync::ONCE_INIT;
#[derive(Clone, Copy, Eq, PartialEq)]
struct FiletypeCache(*mut Arc<Mutex<HashMap<PathBuf, Type>>>);
unsafe impl Sync for FiletypeCache {}

static mut FILETYPE_CACHE: FiletypeCache = FiletypeCache(0 as *mut _);


pub fn get_filetype_cache() -> Arc<Mutex<HashMap<PathBuf, Type>>> {
  FILETYPE_CACHE_START.call_once(|| {
    debug_assert!(unsafe { FILETYPE_CACHE == FiletypeCache(0 as *mut _) });

    let cache: Box<Arc<Mutex<HashMap<PathBuf, Type>>>>
    = box Arc::new(Mutex::new(HashMap::new()));

    unsafe { FILETYPE_CACHE = FiletypeCache(::std::mem::transmute(cache)) }
  });

  unsafe {
    let FiletypeCache(inner) = FILETYPE_CACHE;
    (*inner).clone()
  }
}

pub fn override_filetype<T: AsRef<Path>>(p: T, t: Type) {
  let cache = get_filetype_cache();

  cache.lock().unwrap().insert(p.as_ref().to_path_buf(), t);
}
pub fn clear_filetype<T: AsRef<Path>>(p: T) {
  let cache = get_filetype_cache();
  cache.lock().unwrap().remove(&p.as_ref().to_path_buf());
}
pub fn clear_filetypes() {
  let cache = get_filetype_cache();
  cache.lock().unwrap().clear();
}

pub fn get_cached_filetype<T: AsRef<Path>>(p: T) -> Option<Type> {
  let cache = get_filetype_cache();

  let lock = cache.lock().unwrap();

  lock.get(&p.as_ref().to_path_buf())
    .map(|t| t.clone() )
}

// for testing:
static FILE_CACHE_START: sync::Once = sync::ONCE_INIT;

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileContentsCache(*mut Arc<Mutex<HashMap<PathBuf, &'static [u8]>>>);
unsafe impl Sync for FileContentsCache {}
static mut FILE_CACHE: FileContentsCache = FileContentsCache(0 as *mut _);

fn get_file_cache() -> Arc<Mutex<HashMap<PathBuf, &'static [u8]>>> {
  FILE_CACHE_START.call_once(|| {
    debug_assert!(unsafe { FILE_CACHE == FileContentsCache(0 as *mut _) });

    let cache: Box<Arc<Mutex<HashMap<PathBuf, Type>>>>
    = box Arc::new(Mutex::new(HashMap::new()));

    unsafe { FILE_CACHE = FileContentsCache(::std::mem::transmute(cache)) }
  });

  unsafe {
    let FileContentsCache(inner) = FILE_CACHE;
    (*inner).clone()
  }
}

pub fn override_file_contents<T: AsRef<Path>>(p: T, contents: &'static [u8]) {
  let cache = get_file_cache();

  cache
    .lock()
    .unwrap()
    .insert(p.as_ref().to_path_buf(), contents);
}
pub fn clear_file_contents_cache<T: AsRef<Path>>(p: T) {
  let cache = get_file_cache();

  cache
    .lock()
    .unwrap()
    .remove(&p.as_ref().to_path_buf());
}

pub trait ReadSeek: Read + Seek { }
impl<T> ReadSeek for T
  where T: Read + Seek
{
}

pub fn get_file_contents<T, F, U>(path: T, f: F)
  -> io::Result<U>
  where T: AsRef<Path>,
        F: FnOnce(&T, &mut ReadSeek) -> U,
{
  use std::fs::File;
  let opt = {
    let cache = get_file_cache();

    let lock = cache
      .lock()
      .unwrap();

    lock.get(&path.as_ref().to_path_buf())
      .map(|&a| Cursor::new(a.as_ref()) )
  };
  match opt {
    Some(mut stream) => Ok(f(&path, &mut stream)),
    None => {
      let mut file = try!(File::open(&path));
      Ok(f(&path, &mut file))
    }
  }
}

pub fn file_exists<T: AsRef<Path>>(path: T) -> bool {
  let cache = get_file_cache();

  let lock = cache
    .lock()
    .unwrap();

  return lock.get(&path.as_ref().to_path_buf()).is_some() ||
    path.as_ref().exists()
}

const LLVM_BITCODE_MAGIC: &'static [u8] = &[
  'B' as u8,
  'C' as u8,
  0xc0,
  0xde,
];
const LLVM_WRAPPER_MAGIC: &'static [u8] = &[
  0xde,
  0xc0,
  0x17,
  0x0b,
];
const PNACL_BITCODE_MAGIC: &'static [u8] = &[
  'P' as u8,
  'E' as u8,
  'X' as u8,
  'E' as u8,
];
const WASM_MAGIC: &'static [u8] = &[
  0,
  'a' as u8,
  's' as u8,
  'm' as u8,
];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Subtype {
  Bitcode,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Type {
  Archive(Subtype),
  Object(Subtype),
  Pexe,
  Wasm,
}

macro_rules! test_magic (
    ($file_name:ident $buffer_name:ident $max_size:expr =>
     [$($magic:expr),+] -> $ty:expr) => (
        pub fn $file_name<T: AsRef<::std::path::Path>>(path: T) -> bool {
            let cached_type = get_cached_filetype(&path);
            if cached_type.is_some() {
                let cached_type = cached_type.unwrap();
                if cached_type == $ty {
                    return true;
                }
            }

            let is = get_file_contents(&path, |_, file| $buffer_name(file) )
                .unwrap_or(false);

            if is {
                override_filetype(path, $ty);
            }
            return is;
        }

        pub fn $buffer_name<T: ::std::io::Read + ::std::io::Seek + ?Sized>(io: &mut T) ->
            bool
        {
            use std::io::{SeekFrom};
            use std::mem;

            let pos = io.seek(SeekFrom::Current(0)).unwrap();

            let mut buf: [u8; $max_size] = unsafe { mem::uninitialized() };
            let read = io.read(buf.as_mut());
            io.seek(SeekFrom::Start(pos)).unwrap();
            match read {
                Ok(n) => {
                    if n != buf.len() {
                        return false;
                    }
                },
                Err(_) => { return false; },
            }

            return $(buf == $magic.as_ref())||+;
        }
    )
);

test_magic!(is_file_raw_llvm_bitcode is_stream_raw_llvm_bitcode 4 =>
            [LLVM_BITCODE_MAGIC] -> Type::Object(Subtype::Bitcode));
test_magic!(is_file_wrapped_llvm_bitcode is_stream_wrapped_llvm_bitcode 4 =>
            [LLVM_WRAPPER_MAGIC] -> Type::Object(Subtype::Bitcode));
test_magic!(is_file_pnacl_bitcode is_stream_pnacl_bitcode 4 =>
            [PNACL_BITCODE_MAGIC] -> Type::Pexe);
test_magic!(is_file_wasm_module is_stream_wasm_module 4 =>
            [WASM_MAGIC] -> Type::Wasm);

test_magic!(is_file_llvm_bitcode is_stream_llvm_bitcode 4 =>
            [LLVM_BITCODE_MAGIC, LLVM_WRAPPER_MAGIC] -> Type::Object(Subtype::Bitcode));

pub fn file_type<T>(path: T) -> io::Result<Option<Type>>
  where T: AsRef<Path>,
{
  match get_cached_filetype(&path) {
    Some(v) => { return Ok(Some(v)); },
    _ => {},
  }

  let t = get_file_contents(&path, |_, mut file| {
    if is_stream_llvm_bitcode(file) {
      return Some(Type::Object(Subtype::Bitcode));
    }
    if is_stream_pnacl_bitcode(file) {
      return Some(Type::Pexe);
    }
    if is_stream_wasm_module(file) {
      return Some(Type::Wasm);
    }

    if let Ok(Some(ar_type)) = ar::stream_archive_type(&mut file) {
      return Some(Type::Archive(ar_type));
    }

    None
  })?;

  Ok(t)
}

pub fn could_be_linker_script<T: AsRef<Path>>(path: T) -> bool {
  let exts: ::std::collections::HashSet<Option<::std::ffi::OsString>> = hashset!{
        Some(From::from("o")), Some(From::from("so")),
        Some(From::from("a")), Some(From::from("po")),
        Some(From::from("pa")), Some(From::from("x")),
    };

  exts.contains(&path.as_ref().extension().map(|v| From::from(v) )) &&
    ar::archive_type(&path).is_none() &&
    !is_file_raw_llvm_bitcode(&path) &&
    !is_file_wrapped_llvm_bitcode(&path)
}
pub fn is_linker_script<T: AsRef<Path>>(path: T) -> bool {
  could_be_linker_script(path.as_ref()) &&
    ldtools::parse_linker_script_file(&path).is_some()
}

pub mod ar {
  use std::fs::File;
  use std::io::{self, Error, ErrorKind, Read, Seek, SeekFrom, Cursor};
  use std::mem;
  use std::path::Path;
  use std::str::FromStr;

  extern crate ar;

  use super::{is_stream_llvm_bitcode, get_cached_filetype,
              get_file_contents, override_filetype};

  pub use super::Subtype as Type;

  const AR_MAGIC: &'static str = "!<arch>\n";
  const THIN_MAGIC: &'static str = "!<thin>\n";

  pub fn is_file_an_archive<T: AsRef<::std::path::Path>>(path: T) -> bool {
    let cached_type = get_cached_filetype(&path);
    if cached_type.is_some() {
      let cached_type = cached_type.unwrap();
      match cached_type {
        super::Type::Archive(_) => {
          return true;
        },
        _ => {},
      }
    }

    let is = get_file_contents(&path, |_, file| is_buffer_an_archive(file) )
      .unwrap_or(false);
    return is;
  }

  pub fn is_buffer_an_archive<T: ::std::io::Read + ::std::io::Seek + ?Sized>(io: &mut T) ->
  bool
  {
    use std::io::{SeekFrom};
    use std::mem;

    let mut buf: [u8; 8] = unsafe { mem::uninitialized() };
    match io.read(buf.as_mut()) {
      Ok(n) => {
        io.seek(SeekFrom::Current(-(n as i64)))
          .unwrap();
        if n != buf.len() {
          return false;
        }
      },
      Err(_) => { return false; },
    }

    return buf == AR_MAGIC.as_ref() || buf == THIN_MAGIC.as_ref();
  }

  pub fn archive_type<T: AsRef<Path>>(path: T) -> Option<Type> {
    get_cached_filetype(&path)
      .and_then(|t| match t {
        super::Type::Archive(subtype) => Some(subtype),
        _ => None,
      })
      .or_else(|| {
        // XXX(rdiamond): This ignores our cache.
        let file = File::open(path.as_ref())
          .unwrap_or_else(|err| {
            panic!("path `{}`: `{}`", path.as_ref().display(), err);
          });
        let mut ar = ar::Archive::new(file);
        while let Some(Ok(mut member)) = ar.next_entry() {
          let mut buffer = Vec::new();
          member.read_to_end(&mut buffer).unwrap();
          let mut stream = Cursor::new(buffer);
          if is_stream_llvm_bitcode(&mut stream) {
            override_filetype(path, super::Type::Archive(Type::Bitcode));
            return Some(Type::Bitcode);
          }
        }
        None
      })
  }

  pub fn stream_archive_type<T>(mut io: T) -> io::Result<Option<Type>>
    where T: Read + Seek
  {
    let pos = io.seek(SeekFrom::Current(0))?;
    let result = 'block: loop {
      let mut ar = ar::Archive::new(&mut io);
      let mut buffer = Vec::new();
      while let Some(Ok(mut member)) = ar.next_entry() {
        buffer.clear();
        member.read_to_end(&mut buffer)?;
        let mut stream = Cursor::new(&mut buffer);
        if is_stream_llvm_bitcode(&mut stream) {
          break 'block Ok(Some(Type::Bitcode));
        }
      }

      break 'block Ok(None);
    };
    io.seek(SeekFrom::Start(pos))?;

    result
  }


  pub struct MemberHeader {
    pub start: u64,
    name: [u8; 16],
    pub size: u64,
  }

  impl MemberHeader {
    pub fn read(from: &mut File) -> io::Result<MemberHeader> {
      let mut header: [u8; 60] = unsafe { ::std::mem::uninitialized() };
      if try!(from.read(header.as_mut())) < 60 {
        return Err(Error::new(ErrorKind::Other,
                              "Short count reading archive member header"));
      }

      let size_str = match ::std::str::from_utf8(&header[48..58]) {
        Ok(s) => s,
        Err(e) => {
          return Err(Error::new(ErrorKind::Other, e));
        },
      };

      let magic: &[u8] = "`\n".as_ref();
      if &header[58..] != magic {
        return Err(Error::new(ErrorKind::Other, "Invalid archive member
                                      header magic"));
      }

      let mut member = MemberHeader {
        start: try!(from.seek(SeekFrom::Current(0))),
        name: unsafe { mem::uninitialized() },
        size: match FromStr::from_str(size_str) {
          Ok(size) => size,
          Err(e) => {
            return Err(Error::new(ErrorKind::Other, e));
          },
        },
      };

      unsafe {
        ::std::intrinsics::copy_nonoverlapping(header[..16].as_ptr(),
                                               member.name.as_mut_ptr(),
                                               16)
      }

      if member.name().starts_with(r"#1/") {
        return Err(Error::new(ErrorKind::Other, "BSD-style long file
                                      names not supported"));
      }

      Ok(member)
    }

    pub fn name(&self) -> &str {
      unsafe { ::std::str::from_utf8_unchecked(self.name.as_ref()) }
    }
    pub fn is_svr4_symtab(&self) -> bool {
      self.name == "/               ".as_ref()
    }
    pub fn is_llvm_symtab(&self) -> bool {
      self.name == "#_LLVM_SYM_TAB_#".as_ref()
    }
    pub fn is_bsd4_symtab(&self) -> bool {
      self.name == "__.SYMDEF SORTED".as_ref()
    }
    pub fn is_strtab(&self) -> bool {
      self.name == "//              ".as_ref()
    }
    pub fn is_regular_member(&self) -> bool {
      !self.is_svr4_symtab() &&
        !self.is_llvm_symtab() &&
        !self.is_bsd4_symtab() &&
        !self.is_strtab()
    }
    pub fn is_long_name(&self) -> bool {
      self.is_regular_member() &&
        self.name().starts_with("/")
    }
  }
}

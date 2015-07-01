

use std::path::Path;

use ldtools;

const LLVM_BITCODE_MAGIC: &'static str = r"BC\xc0\xde";
const LLVM_WRAPPER_MAGIC: &'static str = r"\xde\xc0\x17\x0b";
const PNACL_BITCODE_MAGIC: &'static str = r"PEXE";

pub enum Type {
    Archive(ar::Type),
}

macro_rules! test_magic (
    ($file_name:ident $buffer_name:ident $max_size:expr =>
     [$($magic:expr),+]) => (
        pub fn $file_name<T: AsRef<::std::path::Path>>(path: T) -> bool {
            use std::fs::File;

            let file_res = File::open(path);
            if file_res.is_err() { return false; }
            let mut file = file_res.unwrap();

            $buffer_name(&mut file)
        }

        pub fn $buffer_name<T: ::std::io::Read + ::std::io::Seek>(io: &mut T) ->
            bool
        {
            use std::io::{Read, SeekFrom};
            use std::mem;

            let mut buf: [u8; $max_size] = unsafe { mem::uninitialized() };
            match io.read(buf.as_mut()) {
                Ok(n) if n == buf.len() => {},
                Ok(n) => {
                    io.seek(SeekFrom::Current(-(n as i64)))
                        .unwrap();
                    return false;
                },
                Err(_) => { return false; },
            }

            io.seek(SeekFrom::Current(-(buf.len() as i64)))
                .unwrap();

            return $(buf == $magic.as_ref())||+;
        }
    )
);

test_magic!(is_file_raw_llvm_bitcode is_stream_raw_llvm_bitcode 4 =>
            [LLVM_BITCODE_MAGIC]);
test_magic!(is_file_wrapped_llvm_bitcode is_stream_wrapped_llvm_bitcode 4 =>
            [LLVM_WRAPPER_MAGIC]);
test_magic!(is_file_pnacl_bitcode is_stream_pnacl_bitcode 4 =>
            [PNACL_BITCODE_MAGIC]);

test_magic!(is_file_llvm_bitcode is_stream_llvm_bitcode 4 =>
            [LLVM_BITCODE_MAGIC, LLVM_WRAPPER_MAGIC]);

pub fn is_file_native<T: AsRef<Path>>(path: T) -> bool {
    use std::fs::File;

    let file_res = File::open(&path);
    if file_res.is_err() { return false; }
    let mut file = file_res.unwrap();

    if is_stream_raw_llvm_bitcode(&mut file) ||
        is_stream_wrapped_llvm_bitcode(&mut file) ||
        is_stream_pnacl_bitcode(&mut file)
    {
        return false;
    }

    if ar::archive_type(&path)
        .map(|ar| {
            match ar {
                ar::Type::ELF(_) => false,
                _ => true,
            }
        }).unwrap_or(false)
    {
        return false;
    }

    // if the file isn't a portable type, we assume it must be native.
    return true;
}

pub fn could_be_linker_script<T: AsRef<Path>>(path: T) -> bool {
    let exts: ::std::collections::HashSet<Option<::std::ffi::OsString>> = hashset!{
        Some(From::from(".o")), Some(From::from(".so")),
        Some(From::from(".a")), Some(From::from(".po")),
        Some(From::from(".pa")), Some(From::from(".x")),
    };

    exts.contains(&path.as_ref().extension().map(|v| From::from(v) )) &&
        !elf::is_file_elf(&path) &&
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

    use elf;
    use llvm::archive_ro;

    use super::{is_stream_llvm_bitcode};

    #[derive(Copy, Clone)]
    pub enum Type {
        Bitcode,
        ELF(elf::types::Machine),
    }

    const AR_MAGIC: &'static str = r"!<arch>\n";
    const THIN_MAGIC: &'static str = r"!<thin>\n";

    test_magic!(is_file_an_archive is_buffer_an_archive 8 => [AR_MAGIC,
                                                              THIN_MAGIC]);

    pub fn archive_type<T: AsRef<Path>>(path: T) -> Option<Type> {
        use elf;
        archive_ro::ArchiveRO::open(path.as_ref())
            .and_then(|ar| {
                for member in ar.iter() {
                    let mut stream = Cursor::new(member.data());
                    if is_stream_llvm_bitcode(&mut stream) {
                        return Some(Type::Bitcode);
                    } else if let Ok(elf) = elf::File::open_stream(&mut stream) {
                        return Some(Type::ELF(elf.ehdr.machine));
                    }
                }

                None
            })
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

pub mod elf {
    use elf;

    pub use elf::*;

    pub fn is_file_elf<T: AsRef<::std::path::Path>>(path: T) -> bool {
        elf::File::open_path(path).ok().is_some()
    }
    pub fn is_stream_elf<T: ::std::io::Read + ::std::io::Seek>(io: &mut T) ->
        bool
    {
        elf::File::open_stream(io).ok().is_some()
    }
}

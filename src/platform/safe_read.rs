//! Bounded reads of files the program does not control.
//!
//! Every file this program reads — the config, the theme, the quote cache —
//! lives at a path anyone with write access to that directory can replace.
//! tickerbar runs inside the long-lived, unsandboxed omarchy-shell process, so
//! a read that never returns or never stops growing does not fail tickerbar:
//! it takes the shell with it.
//!
//! Two things make a plain `read_to_string` unsafe here, and BOTH have to be
//! handled, in this order:
//!
//! 1. **A FIFO blocks in `open`, not in `read`.** Checking the metadata of a
//!    path and then opening it is a race and still hangs; opening first and
//!    then checking hangs before the check can run. `O_NONBLOCK` on the open
//!    is what makes a FIFO return immediately. It changes nothing for a
//!    regular file.
//! 2. **A regular file can still be enormous.** The type check has to be
//!    followed by a byte cap.
//!
//! The type check runs on the OPEN DESCRIPTOR, not on the path, so nothing can
//! swap the file between the check and the read.
//!
//! There is deliberately no `O_NOFOLLOW`. The fstat happens after the kernel
//! resolves symlinks, so a symlink pointing at a FIFO or at a huge file is
//! already refused by the two rules above. All `O_NOFOLLOW` would add is
//! refusing a symlink to a small regular file — which is exactly what every
//! dotfile manager creates for a config, and a legitimate setup this program
//! has no reason to break.

use std::io::Read;
use std::path::Path;

/// A config or theme: generous against any real one, small against a machine.
pub const CONFIG_LIMIT: u64 = 256 * 1024;
/// The quote cache: one small JSON record this program wrote itself.
pub const STATE_LIMIT: u64 = 64 * 1024;

/// Read a whole file as text, refusing anything that is not a regular file and
/// anything above `limit` bytes.
pub fn read_bounded(path: &Path, limit: u64) -> std::io::Result<String> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?
    };
    #[cfg(not(unix))]
    let file = std::fs::File::open(path)?;

    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }

    // The type is settled, so O_NONBLOCK has done its work and now only adds
    // doubt: `read_to_string` does not retry `WouldBlock`, and a filesystem
    // that honours the flag on a regular file (FUSE, a network mount) could
    // turn a good read into a spurious error. Linux ignores it for local
    // regular files; clearing it makes that true everywhere.
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let fd = file.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags != -1 {
            unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
        }
    }

    // Bytes first, text second. `read_to_string` validates UTF-8 as it reads,
    // so a cut through a multibyte codepoint at byte `limit + 1` would report
    // "invalid UTF-8" about a file that is valid UTF-8 and merely too large —
    // the clear message this function exists to give, unreachable in exactly
    // its own case. Reading one byte past the limit is what tells a file that
    // fits from one that was truncated at the cap.
    let mut buf: Vec<u8> = Vec::new();
    file.take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} is larger than {} bytes", path.display(), limit),
        ));
    }
    String::from_utf8(buf).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} is not valid UTF-8: {e}", path.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tickerbar-safe-read-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_regular_file_under_the_limit_reads_whole() {
        let p = tmp("ok").join("f");
        std::fs::write(&p, "hello").unwrap();
        assert_eq!(read_bounded(&p, CONFIG_LIMIT).unwrap(), "hello");
    }

    #[test]
    fn a_file_over_the_limit_is_refused_by_name_and_size() {
        let p = tmp("big").join("f");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(&[b'x'; 200]).unwrap();
        let e = read_bounded(&p, 100).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        assert!(e.to_string().contains("larger than 100 bytes"));
    }

    #[test]
    fn a_directory_is_refused_instead_of_read() {
        let d = tmp("dir");
        assert!(read_bounded(&d, CONFIG_LIMIT).is_err());
    }

    #[test]
    fn an_oversize_file_reports_its_size_even_when_the_cap_cuts_a_codepoint() {
        // 'ñ' is two bytes. With a cap that lands between them, validating
        // UTF-8 before the size check would report invalid UTF-8 about a file
        // that is valid and merely too large.
        let p = tmp("utf8").join("f");
        std::fs::write(&p, "ñ".repeat(50)).unwrap();
        let e = read_bounded(&p, 5).unwrap_err();
        assert!(
            e.to_string().contains("larger than 5 bytes"),
            "got: {e}"
        );
    }

    #[test]
    fn a_file_that_is_not_utf8_says_so() {
        let p = tmp("bin").join("f");
        std::fs::write(&p, [0xff, 0xfe]).unwrap();
        let e = read_bounded(&p, CONFIG_LIMIT).unwrap_err();
        assert!(e.to_string().contains("not valid UTF-8"), "got: {e}");
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_with_no_writer_is_refused_instead_of_blocking_for_ever() {
        // Without O_NONBLOCK this open never returns and the test hangs, which
        // is the whole failure this module exists to stop.
        let p = tmp("fifo").join("f");
        let c = std::ffi::CString::new(p.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let e = read_bounded(&p, CONFIG_LIMIT).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
        assert!(e.to_string().contains("not a regular file"));
    }
}

use std::{
    io::{
        self,
        Read,
        Write,
    },
    mem,
    os::{
        fd::{
            AsFd,
            AsRawFd,
            BorrowedFd,
            FromRawFd,
            OwnedFd,
            RawFd,
        },
        unix::net::UnixStream,
    },
    ptr,
};

const FD_SIZE: usize = mem::size_of::<RawFd>();
const CONTROL_WORDS: usize = 16;

#[cfg(any(target_os = "linux", target_os = "android"))]
const RECV_FLAGS: libc::c_int = libc::MSG_CMSG_CLOEXEC;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const RECV_FLAGS: libc::c_int = 0;

pub(crate) fn send_with_fd(
    stream: &UnixStream, bytes: &[u8], fd: BorrowedFd<'_>,
) -> io::Result<()> {
    let mut control = [0u64; CONTROL_WORDS];
    let mut iov = libc::iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    };
    let mut msg: libc::msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = unsafe { libc::CMSG_SPACE(FD_SIZE as u32) } as _;

    let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    if cmsg.is_null() {
        return Err(io::Error::other("no room for the descriptor message"));
    }
    unsafe {
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(FD_SIZE as u32) as _;
        let raw = fd.as_raw_fd().to_ne_bytes();
        ptr::copy_nonoverlapping(raw.as_ptr(), libc::CMSG_DATA(cmsg), FD_SIZE);
    }

    let sent = loop {
        let n = unsafe { libc::sendmsg(stream.as_raw_fd(), &msg, 0) };
        if n >= 0 {
            break n as usize;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    };

    let mut stream = stream;
    stream.write_all(&bytes[sent..])
}

pub(crate) fn send_with_fd_until_closed(
    stream: &UnixStream, bytes: &[u8], fd: OwnedFd,
) -> io::Result<()> {
    send_with_fd(stream, bytes, fd.as_fd())?;
    let mut stream = stream;
    let mut scratch = [0u8; 64];
    while matches!(stream.read(&mut scratch), Ok(read) if read > 0) {}
    drop(fd);
    Ok(())
}

pub(crate) fn recv_with_fds(
    stream: &UnixStream, buf: &mut [u8], fds: &mut Vec<OwnedFd>,
) -> io::Result<usize> {
    let mut control = [0u64; CONTROL_WORDS];
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: buf.len(),
    };
    let mut msg: libc::msghdr = unsafe { mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = mem::size_of_val(&control) as _;

    let received = loop {
        let n = unsafe { libc::recvmsg(stream.as_raw_fd(), &mut msg, RECV_FLAGS) };
        if n >= 0 {
            break n as usize;
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    };

    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !cmsg.is_null() {
        let (level, kind, len) =
            unsafe { ((*cmsg).cmsg_level, (*cmsg).cmsg_type, (*cmsg).cmsg_len) };
        if level == libc::SOL_SOCKET && kind == libc::SCM_RIGHTS {
            let header = unsafe { libc::CMSG_LEN(0) } as usize;
            let len = usize::try_from(len as u64).unwrap_or(0);
            let count = len.saturating_sub(header) / FD_SIZE;
            let data = unsafe { libc::CMSG_DATA(cmsg) };
            for index in 0..count {
                let mut raw = [0u8; FD_SIZE];
                unsafe {
                    ptr::copy_nonoverlapping(data.add(index * FD_SIZE), raw.as_mut_ptr(), FD_SIZE)
                };
                let fd = unsafe { OwnedFd::from_raw_fd(RawFd::from_ne_bytes(raw)) };
                set_cloexec(&fd);
                fds.push(fd);
            }
        }
        cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
    }

    Ok(received)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn set_cloexec(_fd: &OwnedFd) {}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn set_cloexec(fd: &OwnedFd) {
    let raw = fd.as_raw_fd();
    unsafe {
        let flags = libc::fcntl(raw, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(raw, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{
        TcpListener,
        TcpStream,
    };
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_listener_received_after_a_delay_still_accepts_connections() {
        let (helper, app) = UnixStream::pair().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let sender = std::thread::spawn(move || {
            send_with_fd_until_closed(&helper, b"{}", OwnedFd::from(listener))
        });

        std::thread::sleep(Duration::from_millis(300));
        let mut fds = Vec::new();
        recv_with_fds(&app, &mut [0u8; 16], &mut fds).unwrap();
        drop(app);
        sender.join().unwrap().unwrap();

        let listener = TcpListener::from(fds.remove(0));
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(b"ping").unwrap();
        let (mut accepted, _) = listener.accept().unwrap();
        let mut received = [0u8; 4];
        accepted.read_exact(&mut received).unwrap();
        assert_eq!(&received, b"ping");
    }
}

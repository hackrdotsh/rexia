//! Privileged boundary: bind one IPv4 loopback socket, hand it back, and exit.
use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
use std::{
    io::{self, IoSlice},
    net::{Ipv4Addr, TcpListener},
    os::fd::AsFd,
    process::ExitCode,
};

fn bind() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let port = args
        .next()
        .and_then(|arg| arg.to_str()?.parse::<u16>().ok())
        .filter(|port| *port != 0 && args.next().is_none())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Expected one nonzero TCP port")
        })?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    let fds = [listener.as_fd()];
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
        return Err(io::Error::other("Couldn't attach the listening socket"));
    }
    sendmsg(
        io::stdout(),
        &[IoSlice::new(b"F")],
        &mut control,
        SendFlags::NOSIGNAL | SendFlags::DONTWAIT,
    )?;
    Ok(())
}

fn main() -> ExitCode {
    match bind() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

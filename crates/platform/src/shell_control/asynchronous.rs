use interprocess::local_socket::{
    ListenerOptions,
    tokio::{RecvHalf, SendHalf, Stream, prelude::*},
};
use std::io;

pub struct AsyncLocalListener(interprocess::local_socket::tokio::Listener);

impl AsyncLocalListener {
    pub fn bind(endpoint: &str) -> io::Result<Self> {
        #[cfg(unix)]
        let listener = {
            use std::os::unix::fs::PermissionsExt;
            let listener = ListenerOptions::new()
                .name(super::name(endpoint)?)
                .create_tokio()?;
            std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600))?;
            listener
        };
        #[cfg(windows)]
        let listener = {
            use interprocess::os::windows::named_pipe::PipeListenerOptions;
            let listener = PipeListenerOptions::new()
                .path(format!(r"\\.\pipe\{endpoint}"))
                .accept_remote(false)
                .input_buffer_size_hint(1024 * 1024)
                .output_buffer_size_hint(1024 * 1024)
                .security_descriptor(Some(super::windows::descriptor()?))
                .create_tokio_duplex::<interprocess::os::windows::named_pipe::pipe_mode::Bytes>()?;
            interprocess::local_socket::tokio::Listener::NamedPipe(listener.into())
        };
        Ok(Self(listener))
    }

    pub async fn accept(&self) -> io::Result<AsyncLocalConnection> {
        let stream = self.0.accept().await?;
        check_user(&stream)?;
        Ok(AsyncLocalConnection(stream))
    }
}

pub struct AsyncLocalConnection(Stream);

impl AsyncLocalConnection {
    pub async fn connect(endpoint: &str, expected_pid: u32) -> io::Result<Self> {
        #[cfg(unix)]
        let name = super::name(endpoint)?;
        #[cfg(windows)]
        let name = {
            if endpoint.is_empty()
                || endpoint.len() > 120
                || !endpoint
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            endpoint.to_ns_name::<interprocess::local_socket::GenericNamespaced>()?
        };
        let stream = Stream::connect(name).await?;
        check_user(&stream)?;
        let connection = Self(stream);
        if connection.peer_pid()? != expected_pid {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(connection)
    }

    pub fn peer_pid(&self) -> io::Result<u32> {
        #[cfg(not(target_os = "macos"))]
        return self
            .0
            .peer_creds()?
            .pid()
            .map(|pid| pid as u32)
            .ok_or_else(|| io::ErrorKind::PermissionDenied.into());
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd;
            let Stream::UdSocket(stream) = &self.0;
            let mut pid: libc::pid_t = 0;
            let mut len = size_of_val(&pid) as libc::socklen_t;
            // SAFETY: the descriptor and initialized output buffers remain valid for this call.
            if unsafe {
                libc::getsockopt(
                    stream.inner().as_raw_fd(),
                    0,
                    libc::LOCAL_PEERPID,
                    (&mut pid as *mut libc::pid_t).cast(),
                    &mut len,
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(pid as u32)
        }
    }

    pub fn split(self) -> (RecvHalf, SendHalf) {
        self.0.split()
    }

    pub fn into_inner(self) -> Stream {
        self.0
    }

    pub fn from_inner(stream: Stream) -> Self {
        Self(stream)
    }
}

fn check_user(stream: &Stream) -> io::Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions.
        if stream.peer_creds()?.euid() != Some(unsafe { libc::geteuid() }) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        use std::os::fd::AsRawFd;
        let Stream::UdSocket(stream) = stream;
        let bytes: libc::c_int = 1024 * 1024;
        for option in [libc::SO_SNDBUF, libc::SO_RCVBUF] {
            // SAFETY: the live socket descriptor and integer buffer remain valid for the call.
            unsafe {
                libc::setsockopt(
                    stream.inner().as_raw_fd(),
                    libc::SOL_SOCKET,
                    option,
                    (&raw const bytes).cast(),
                    size_of_val(&bytes) as libc::socklen_t,
                );
            }
        }
    }
    #[cfg(windows)]
    let _ = stream;
    Ok(())
}

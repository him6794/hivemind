//! Windows desktop activation is local to a logon session, role and UI port.
//! The backend owns the pipe before binding services; it grants only SHOW.

use crate::{request_local_ui_activation, ClientRole};
use anyhow::{bail, Context, Result};
use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout, Instant};
use windows_sys::Win32::Foundation::{
    LocalFree, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER,
    ERROR_PIPE_BUSY,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenLogonSid, SECURITY_ATTRIBUTES, TOKEN_GROUPS, TOKEN_QUERY,
};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

const SHOW_MARKER: &[u8] = crate::LOCAL_UI_SHOW_MARKER;
const ACK_MARKER: &[u8] = b"HIVEMIND_LOCAL_UI_REPLY_V1\n";
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);
const FRAME_TIMEOUT: Duration = Duration::from_secs(2);

pub enum InstanceLaunch {
    Primary(ClientInstance),
    Activated,
}

/// Keep ownership even if the listener fails; do not reopen a duplicate backend.
pub struct ClientInstance {
    _owner: Option<Arc<NamedPipeServer>>,
    listener: Option<JoinHandle<()>>,
}

impl Drop for ClientInstance {
    fn drop(&mut self) {
        if let Some(listener) = &self.listener {
            listener.abort();
        }
    }
}

struct LocalAllocation(*mut std::ffi::c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

fn logon_sid() -> Result<String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error()).context("cannot query Windows logon token");
    }
    // OpenProcessToken transferred ownership of this valid handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut bytes = 0;
    let status = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenLogonSid,
            ptr::null_mut(),
            0,
            &mut bytes,
        )
    };
    if status != 0
        || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
    {
        bail!("cannot determine Windows logon SID size");
    }
    if bytes < size_of::<TOKEN_GROUPS>() as u32 || bytes > 64 * 1024 {
        bail!("invalid Windows logon SID size");
    }
    // TOKEN_GROUPS contains pointers, so the buffer must be pointer-aligned.
    let mut buffer = vec![0_usize; (bytes as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenLogonSid,
            buffer.as_mut_ptr().cast(),
            bytes,
            &mut bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error()).context("cannot read Windows logon SID");
    }
    let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
    if groups.GroupCount != 1 || groups.Groups[0].Sid.is_null() {
        bail!("Windows logon token has no unique logon SID");
    }
    let mut text = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(groups.Groups[0].Sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error()).context("cannot format Windows logon SID");
    }
    let allocation = LocalAllocation(text.cast());
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })?;
    drop(allocation);
    Ok(sid)
}

fn pipe_name(role: ClientRole, port: u16, sid: &str) -> String {
    format!(
        r"\\.\pipe\hivemind-desktop-v1-{}-{port}-{sid}",
        role.as_str()
    )
}

fn create_server(
    name: &str,
    sid: &str,
    first_instance: bool,
    max_instances: usize,
) -> Result<NamedPipeServer> {
    let descriptor: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut security = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut security,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error())
            .context("cannot restrict desktop activation pipe to this Windows logon");
    }
    let allocation = LocalAllocation(security);
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security,
        bInheritHandle: 0,
    };
    // The descriptor lives through CreateNamedPipe; handles are non-inheritable.
    let server = unsafe {
        ServerOptions::new()
            .first_pipe_instance(first_instance)
            .max_instances(max_instances)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            )
    };
    drop(allocation);
    Ok(server?)
}

pub(crate) fn allow_foreground(process_id: u32) {
    // Explorer may grant the duplicate foreground rights; hand them only to the
    // pipe owner, and then that owner's helper, never ASFW_ANY.
    unsafe { AllowSetForegroundWindow(process_id) };
}

async fn serve_request<F>(server: &mut NamedPipeServer, activate: &F) -> Result<()>
where
    F: Fn() -> bool,
{
    let mut frame = [0; SHOW_MARKER.len()];
    timeout(FRAME_TIMEOUT, server.read_exact(&mut frame))
        .await
        .context("desktop activation frame timed out")??;
    if frame != SHOW_MARKER {
        bail!("invalid desktop activation frame");
    }
    let queued = activate();
    timeout(FRAME_TIMEOUT, async {
        server.write_all(ACK_MARKER).await?;
        server.write_all(&[u8::from(queued)]).await
    })
    .await
    .context("desktop activation reply timed out")??;
    // Wait for the client to consume ACK and close before disconnecting, since
    // DisconnectNamedPipe discards bytes the client has not read yet.
    let mut extra = [0];
    match timeout(ACTIVATION_TIMEOUT, server.read(&mut extra))
        .await
        .context("desktop activation client did not consume reply")?
    {
        Ok(0) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        _ => bail!("unexpected data after desktop activation"),
    }
}

fn own_server<F>(
    owner: NamedPipeServer,
    mut server: NamedPipeServer,
    request_name: String,
    sid: String,
    activate: F,
) -> ClientInstance
where
    F: Fn() -> bool + Send + Sync + 'static,
{
    let owner = Arc::new(owner);
    let listener_owner = Arc::clone(&owner);
    let listener = tokio::spawn(async move {
        // Keep ownership until an aborted listener has dropped its request pipe.
        let _owner = listener_owner;
        loop {
            if let Err(error) = server.connect().await {
                tracing::error!(%error, "Desktop activation listener failed; retaining instance ownership");
                break;
            }
            if let Err(error) = serve_request(&mut server, &activate).await {
                tracing::warn!(%error, "Rejected desktop activation request");
            }
            // A fresh Tokio pipe avoids carrying EOF/read state into a new
            // connection. Create it before dropping the old namespace handle.
            let next = match create_server(&request_name, &sid, false, 2) {
                Ok(next) => next,
                Err(error) => {
                    tracing::error!(%error, "Cannot renew desktop activation listener; retaining instance ownership");
                    break;
                }
            };
            if let Err(error) = server.disconnect() {
                tracing::warn!(%error, "Desktop activation disconnect failed");
            }
            server = next;
        }
    });
    ClientInstance {
        _owner: Some(owner),
        listener: Some(listener),
    }
}

async fn acquire_named<F>(name: &str, sid: &str, activate: F) -> Result<InstanceLaunch>
where
    F: Fn() -> bool + Send + Sync + 'static,
{
    let deadline = Instant::now() + ACTIVATION_TIMEOUT;
    let owner_name = format!("{name}-owner");
    let request_name = format!("{name}-activate");
    let mut reserve = true;
    loop {
        if reserve {
            match create_server(&owner_name, sid, true, 1) {
                Ok(owner) => {
                    let server = create_server(&request_name, sid, true, 2)
                        .context("cannot start local client activation listener")?;
                    return Ok(InstanceLaunch::Primary(own_server(
                        owner, server, request_name, sid.to_owned(), activate,
                    )));
                }
                Err(error)
                    if error
                        .downcast_ref::<io::Error>()
                        .is_some_and(|e| matches!(e.raw_os_error(), Some(code) if code == ERROR_ACCESS_DENIED as i32 || code == ERROR_PIPE_BUSY as i32)) => {}
                Err(error) => return Err(error).context("cannot reserve local client instance"),
            }
            reserve = false;
        }
        match ClientOptions::new().open(&request_name) {
            Ok(mut client) => {
                let mut process_id = 0;
                if unsafe { GetNamedPipeServerProcessId(client.as_raw_handle(), &mut process_id) }
                    == 0
                {
                    return Err(io::Error::last_os_error())
                        .context("cannot identify existing client process");
                }
                allow_foreground(process_id);
                let reply = timeout(deadline.saturating_duration_since(Instant::now()), async {
                    client.write_all(SHOW_MARKER).await.context("cannot queue SHOW to existing client")?;
                    let mut ack = [0; ACK_MARKER.len()];
                    client.read_exact(&mut ack).await.context("cannot read existing client activation acknowledgement")?;
                    if ack != ACK_MARKER { bail!("invalid existing client activation acknowledgement"); }
                    if client.read_u8().await? != 1 {
                        bail!("existing client has no native window available; refusing duplicate startup");
                    }
                    Ok::<_, anyhow::Error>(())
                }).await.context("existing client did not acknowledge activation in time")?;
                match reply {
                    Ok(()) => return Ok(InstanceLaunch::Activated),
                    Err(error) if error.downcast_ref::<io::Error>().is_some_and(|e| {
                        matches!(e.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::NotConnected)
                            || matches!(e.raw_os_error(), Some(code) if code == windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE as i32
                                || code == windows_sys::Win32::Foundation::ERROR_NO_DATA as i32
                                || code == windows_sys::Win32::Foundation::ERROR_PIPE_NOT_CONNECTED as i32)
                    }) => {
                        // The previous owner may be shutting down. Re-elect only
                        // if its exclusive ownership handle has really gone.
                        reserve = true;
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {}
            Err(error) if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => {
                reserve = true;
            }
            Err(error) => {
                return Err(error)
                    .context("cannot contact existing client; refusing duplicate startup")
            }
        }
        if Instant::now() >= deadline {
            bail!("existing client activation pipe is unavailable; refusing duplicate startup");
        }
        sleep(Duration::from_millis(25)).await;
    }
}

fn ui_port(addr: &str) -> Result<u16> {
    // Binding still validates/resolves the host. Admission needs only the port,
    // so preserve hostname and IPv6 bind forms supported by TcpListener.
    let (_, port) = addr
        .rsplit_once(':')
        .context("local client address has no port")?;
    port.parse().context("invalid local client UI port")
}

pub async fn acquire_or_activate(role: ClientRole, addr: &str) -> Result<InstanceLaunch> {
    let port = ui_port(addr)?;
    // Port zero deliberately requests a new independent, ephemeral listener.
    if port == 0 {
        return Ok(InstanceLaunch::Primary(ClientInstance {
            _owner: None,
            listener: None,
        }));
    }
    let sid = logon_sid()?;
    acquire_named(&pipe_name(role, port, &sid), &sid, move || {
        request_local_ui_activation(role)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_name() -> (String, String) {
        let sid = logon_sid().unwrap();
        (
            format!(
                r"\\.\pipe\hivemind-instance-test-{}-{}",
                std::process::id(),
                rand::random::<u64>()
            ),
            sid,
        )
    }

    async fn connect_test_client(name: &str) -> tokio::net::windows::named_pipe::NamedPipeClient {
        timeout(ACTIVATION_TIMEOUT, async {
            loop {
                match ClientOptions::new().open(format!("{name}-activate")) {
                    Ok(client) => return client,
                    Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                        sleep(Duration::from_millis(1)).await;
                    }
                    Err(error) => panic!("cannot open test activation client: {error}"),
                }
            }
        })
        .await
        .unwrap()
    }

    #[test]
    fn scope_is_role_port_and_logon_not_installation_directory() {
        assert_ne!(
            pipe_name(ClientRole::Master, 8082, "S-1-5-5-1-2"),
            pipe_name(ClientRole::Worker, 8082, "S-1-5-5-1-2")
        );
        assert_ne!(
            pipe_name(ClientRole::Master, 8082, "S-1-5-5-1-2"),
            pipe_name(ClientRole::Master, 8083, "S-1-5-5-1-2")
        );
        assert_ne!(
            pipe_name(ClientRole::Master, 8082, "S-1-5-5-1-2"),
            pipe_name(ClientRole::Master, 8082, "S-1-5-5-1-3")
        );
    }

    #[tokio::test]
    async fn duplicate_requests_activate_without_initializing_another_backend() {
        let (name, sid) = test_name();
        let calls = Arc::new(AtomicUsize::new(0));
        let target = Arc::clone(&calls);
        let primary = acquire_named(&name, &sid, move || {
            target.fetch_add(1, Ordering::SeqCst);
            true
        })
        .await
        .unwrap();
        assert!(matches!(primary, InstanceLaunch::Primary(_)));
        for _ in 0..4 {
            assert!(matches!(
                acquire_named(&name, &sid, || panic!("duplicate must not become primary"))
                    .await
                    .unwrap(),
                InstanceLaunch::Activated
            ));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        drop(primary);
        tokio::task::yield_now().await;
        let replacement = acquire_named(&name, &sid, || true).await.unwrap();
        assert!(matches!(replacement, InstanceLaunch::Primary(_)));
    }

    #[tokio::test]
    async fn malformed_frames_cannot_activate_or_release_ownership() {
        let (name, sid) = test_name();
        let calls = Arc::new(AtomicUsize::new(0));
        let target = Arc::clone(&calls);
        let primary = acquire_named(&name, &sid, move || {
            target.fetch_add(1, Ordering::SeqCst);
            true
        })
        .await
        .unwrap();
        let mut client = ClientOptions::new()
            .open(format!("{name}-activate"))
            .unwrap();
        client
            .write_all(&vec![b'x'; SHOW_MARKER.len()])
            .await
            .unwrap();
        drop(client);
        assert!(matches!(
            acquire_named(&name, &sid, || panic!("ownership must persist"))
                .await
                .unwrap(),
            InstanceLaunch::Activated
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(primary);
    }

    #[test]
    fn admission_preserves_hostname_and_ipv6_bind_forms() {
        for addr in [
            "127.0.0.1:8082",
            "localhost:8082",
            "[::1]:8082",
            "0.0.0.0:8082",
        ] {
            assert_eq!(ui_port(addr).unwrap(), 8082);
        }
        assert_eq!(ui_port("localhost:0").unwrap(), 0);
        assert!(ui_port("localhost").is_err());
        assert!(ui_port("localhost:65536").is_err());
    }

    #[tokio::test]
    async fn concurrent_duplicate_launches_keep_one_owner() {
        let (name, sid) = test_name();
        let calls = Arc::new(AtomicUsize::new(0));
        let target = Arc::clone(&calls);
        let primary = acquire_named(&name, &sid, move || {
            target.fetch_add(1, Ordering::SeqCst);
            true
        })
        .await
        .unwrap();
        let mut requests = tokio::task::JoinSet::new();
        for _ in 0..12 {
            let name = name.clone();
            let sid = sid.clone();
            requests.spawn(async move {
                assert!(matches!(
                    acquire_named(&name, &sid, || panic!("duplicate must not initialize"))
                        .await
                        .unwrap(),
                    InstanceLaunch::Activated
                ));
            });
        }
        while let Some(result) = requests.join_next().await {
            result.unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 12);
        drop(primary);
    }

    #[tokio::test]
    async fn delayed_ack_read_is_not_limited_by_frame_timeout() {
        let (name, sid) = test_name();
        let primary = acquire_named(&name, &sid, || true).await.unwrap();
        let mut client = ClientOptions::new()
            .open(format!("{name}-activate"))
            .unwrap();
        client.write_all(SHOW_MARKER).await.unwrap();
        sleep(FRAME_TIMEOUT + Duration::from_millis(150)).await;
        let mut reply = [0; ACK_MARKER.len()];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply, ACK_MARKER);
        assert_eq!(client.read_u8().await.unwrap(), 1);
        drop(client);
        assert!(matches!(
            acquire_named(&name, &sid, || true).await.unwrap(),
            InstanceLaunch::Activated
        ));
        drop(primary);
    }

    #[tokio::test]
    async fn truncated_and_idle_requests_do_not_release_ownership() {
        let (name, sid) = test_name();
        let calls = Arc::new(AtomicUsize::new(0));
        let target = Arc::clone(&calls);
        let primary = acquire_named(&name, &sid, move || {
            target.fetch_add(1, Ordering::SeqCst);
            true
        })
        .await
        .unwrap();
        let mut truncated = ClientOptions::new()
            .open(format!("{name}-activate"))
            .unwrap();
        truncated.write_all(&SHOW_MARKER[..4]).await.unwrap();
        drop(truncated);
        assert!(matches!(
            acquire_named(&name, &sid, || panic!("ownership lost"))
                .await
                .unwrap(),
            InstanceLaunch::Activated
        ));
        let idle = connect_test_client(&name).await;
        assert!(matches!(
            acquire_named(&name, &sid, || panic!("ownership lost"))
                .await
                .unwrap(),
            InstanceLaunch::Activated
        ));
        drop(idle);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        drop(primary);
    }

    #[tokio::test]
    async fn unavailable_ui_rejects_duplicates_without_releasing_owner() {
        let (name, sid) = test_name();
        let primary = acquire_named(&name, &sid, || false).await.unwrap();
        for _ in 0..2 {
            let error = match acquire_named(&name, &sid, || {
                panic!("unavailable UI must not start another backend")
            })
            .await
            {
                Err(error) => error,
                Ok(_) => panic!("unavailable UI must not claim activation"),
            };
            assert!(error.to_string().contains("no native window available"));
        }
        drop(primary);
    }

    #[tokio::test]
    async fn listener_failure_keeps_exclusive_backend_ownership() {
        let (name, sid) = test_name();
        let InstanceLaunch::Primary(mut primary) =
            acquire_named(&name, &sid, || true).await.unwrap()
        else {
            panic!("initial admission must be primary");
        };
        let listener = primary.listener.take().unwrap();
        listener.abort();
        let _ = listener.await;
        assert!(create_server(&format!("{name}-owner"), &sid, true, 1).is_err());
        drop(primary);
        assert!(matches!(
            acquire_named(&name, &sid, || true).await.unwrap(),
            InstanceLaunch::Primary(_)
        ));
    }

    #[tokio::test]
    async fn ephemeral_ui_ports_do_not_merge_independent_instances() {
        for _ in 0..2 {
            assert!(matches!(
                acquire_or_activate(ClientRole::Master, "127.0.0.1:0")
                    .await
                    .unwrap(),
                InstanceLaunch::Primary(_)
            ));
        }
    }
}

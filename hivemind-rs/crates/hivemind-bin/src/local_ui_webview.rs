#[cfg(windows)]
use anyhow::{anyhow, Context, Result};
#[cfg(windows)]
use std::io::{Read, Write};

#[cfg(windows)]
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
#[cfg(windows)]
use url::Url;
#[cfg(windows)]
use wry::{NewWindowResponse, WebViewBuilder, WebViewBuilderExtWindows};

#[cfg(windows)]
const READY_MARKER: &[u8] = b"HIVEMIND_LOCAL_UI_READY\n";

#[cfg(windows)]
const LOCALHOST_PREFIX: &str = "http://127.0.0.1:";

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LocalUiOrigin {
    port: u16,
}

#[cfg(windows)]
impl LocalUiOrigin {
    fn from_initial_url(input: &str) -> std::result::Result<Self, String> {
        let remainder = input
            .strip_prefix(LOCALHOST_PREFIX)
            .ok_or_else(|| "URL must begin with http://127.0.0.1:<port>/".to_owned())?;
        let authority_end = remainder
            .find(|character| matches!(character, '/' | '?' | '#'))
            .unwrap_or(remainder.len());
        if remainder.as_bytes().get(authority_end) != Some(&b'/') {
            return Err("URL must include a path beginning with '/'".to_owned());
        }

        let port_text = &remainder[..authority_end];
        if port_text.is_empty() || !port_text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("URL must contain a valid TCP port".to_owned());
        }
        let port = port_text
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or_else(|| "URL port must be between 1 and 65535".to_owned())?;

        let parsed = Url::parse(input).map_err(|_| "URL is not valid".to_owned())?;
        if parsed.scheme() != "http"
            || parsed.host_str() != Some("127.0.0.1")
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.port_or_known_default() != Some(port)
        {
            return Err("URL must use the exact http://127.0.0.1:<port> origin".to_owned());
        }

        Ok(Self { port })
    }

    fn allows_navigation_to(self, destination: &str) -> bool {
        let Ok(parsed) = Url::parse(destination) else {
            return false;
        };

        parsed.scheme() == "http"
            && parsed.host_str() == Some("127.0.0.1")
            && parsed.username() == ""
            && parsed.password().is_none()
            && parsed.port_or_known_default() == Some(self.port)
    }
}

#[cfg(windows)]
#[derive(Debug)]
enum UiEvent {
    ParentStdinClosed,
}

#[cfg(windows)]
pub fn run(window_title: &str) -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let argument = arguments
        .next()
        .ok_or_else(|| anyhow!("expected exactly one local UI URL argument"))?;
    if arguments.next().is_some() {
        return Err(anyhow!("expected exactly one local UI URL argument"));
    }
    let url = argument
        .into_string()
        .map_err(|_| anyhow!("local UI URL argument must be valid UTF-8"))?;
    let origin = LocalUiOrigin::from_initial_url(&url).map_err(anyhow::Error::msg)?;

    let mut event_loop_builder = EventLoopBuilder::<UiEvent>::with_user_event();
    let event_loop = event_loop_builder.build();
    let proxy = event_loop.create_proxy();
    watch_parent_stdin(proxy)?;

    let window = WindowBuilder::new()
        .with_title(window_title)
        .with_inner_size(LogicalSize::new(1100.0, 760.0))
        .build(&event_loop)
        .context("failed to create local UI window")?;
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_navigation_handler(move |destination| origin.allows_navigation_to(&destination))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .with_devtools(false)
        .with_incognito(true)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        .build(&window)
        .context("failed to initialize local UI WebView")?;

    std::io::stdout()
        .lock()
        .write_all(READY_MARKER)
        .context("failed to write local UI readiness marker")?;
    std::io::stdout()
        .lock()
        .flush()
        .context("failed to flush local UI readiness marker")?;

    let window_id = window.id();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                window_id: event_window_id,
                event: WindowEvent::CloseRequested,
                ..
            } if event_window_id == window_id => *control_flow = ControlFlow::Exit,
            Event::UserEvent(UiEvent::ParentStdinClosed) => *control_flow = ControlFlow::Exit,
            _ => {}
        }
        let _keep_ui_alive = (&window, &webview);
    });
}

#[cfg(windows)]
fn watch_parent_stdin(proxy: EventLoopProxy<UiEvent>) -> Result<()> {
    std::thread::Builder::new()
        .name("local-ui-parent-monitor".to_owned())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut stdin = stdin.lock();
            let mut byte = [0_u8; 1];
            loop {
                match stdin.read(&mut byte) {
                    Ok(0) => {
                        let _ = proxy.send_event(UiEvent::ParentStdinClosed);
                        break;
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        let _ = proxy.send_event(UiEvent::ParentStdinClosed);
                        break;
                    }
                }
            }
        })
        .context("failed to start local UI lifetime monitor")?;
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::LocalUiOrigin;

    #[test]
    fn accepts_only_explicit_loopback_urls_with_a_valid_port() {
        assert_eq!(
            LocalUiOrigin::from_initial_url("http://127.0.0.1:8080/").unwrap(),
            LocalUiOrigin { port: 8080 }
        );
        assert_eq!(
            LocalUiOrigin::from_initial_url("http://127.0.0.1:65535/ui?tab=queue").unwrap(),
            LocalUiOrigin { port: 65535 }
        );

        for invalid in [
            "http://localhost:8080/",
            "https://127.0.0.1:8080/",
            "http://127.0.0.2:8080/",
            "http://127.0.0.1/",
            "http://127.0.0.1:0/",
            "http://127.0.0.1:65536/",
            "http://127.0.0.1:8080",
            "http://user@127.0.0.1:8080/",
        ] {
            assert!(
                LocalUiOrigin::from_initial_url(invalid).is_err(),
                "unexpectedly accepted {invalid}"
            );
        }
    }

    #[test]
    fn navigation_is_restricted_to_the_exact_origin() {
        let origin = LocalUiOrigin { port: 8080 };
        assert!(origin.allows_navigation_to("http://127.0.0.1:8080/tasks/1"));
        assert!(origin.allows_navigation_to("http://127.0.0.1:8080/?tab=queue"));
        assert!(!origin.allows_navigation_to("https://127.0.0.1:8080/"));
        assert!(!origin.allows_navigation_to("http://127.0.0.1:8081/"));
        assert!(!origin.allows_navigation_to("http://example.com/"));
        assert!(!origin.allows_navigation_to("http://user@127.0.0.1:8080/"));
    }
}

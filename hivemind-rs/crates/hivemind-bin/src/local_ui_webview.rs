use anyhow::{anyhow, Context, Result};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, WebviewWindowBuilder};
use tauri::{AppHandle, WebviewUrl};
use url::Url;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
use windows_core::Interface;

const READY_MARKER: &[u8] = b"HIVEMIND_LOCAL_UI_READY\n";
const LOCALHOST_PREFIX: &str = "http://127.0.0.1:";
const WINDOW_LABEL: &str = "local-ui";
const BOOTSTRAP_URL: &str = "about:blank";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LocalUiOrigin {
    port: u16,
}

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

    fn allows_navigation_to(self, destination: &Url) -> bool {
        destination.scheme() == "http"
            && destination.host_str() == Some("127.0.0.1")
            && destination.username() == ""
            && destination.password().is_none()
            && destination.port_or_known_default() == Some(self.port)
    }
}

struct LocalUiNavigationPolicy {
    origin: LocalUiOrigin,
    allow_initial_blank: AtomicBool,
}

impl LocalUiNavigationPolicy {
    fn new(origin: LocalUiOrigin) -> Self {
        Self {
            origin,
            allow_initial_blank: AtomicBool::new(true),
        }
    }

    fn allows_navigation_to(&self, destination: &Url) -> bool {
        if destination.as_str() == BOOTSTRAP_URL {
            self.allow_initial_blank.swap(false, Ordering::AcqRel)
        } else {
            self.origin.allows_navigation_to(destination)
        }
    }

    fn finish_bootstrap(&self) {
        self.allow_initial_blank.store(false, Ordering::Release);
    }
}

fn parse_helper_arguments<I>(arguments: I) -> Result<(Url, LocalUiOrigin)>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let argument = arguments
        .next()
        .ok_or_else(|| anyhow!("expected exactly one local UI URL argument"))?;
    if arguments.next().is_some() {
        return Err(anyhow!("expected exactly one local UI URL argument"));
    }
    let argument = argument
        .into_string()
        .map_err(|_| anyhow!("local UI URL argument must be valid UTF-8"))?;
    let origin = LocalUiOrigin::from_initial_url(&argument).map_err(anyhow::Error::msg)?;
    let url = Url::parse(&argument).context("local UI URL is not valid")?;
    Ok((url, origin))
}

pub fn run(window_title: &str) -> Result<()> {
    let (url, origin) = parse_helper_arguments(std::env::args_os().skip(1))?;
    let bootstrap_url = Url::parse(BOOTSTRAP_URL).context("invalid local UI bootstrap URL")?;
    let window_title = window_title.to_owned();

    tauri::Builder::default()
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let navigation_policy = Arc::new(LocalUiNavigationPolicy::new(origin));
            let initial_page_started = Arc::new(AtomicBool::new(false));
            let initial_url = url.clone();
            let policy_for_navigation = Arc::clone(&navigation_policy);
            let policy_for_load = Arc::clone(&navigation_policy);
            let started_for_load = Arc::clone(&initial_page_started);
            let app_for_load = app_handle.clone();

            WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(bootstrap_url))
                .title(window_title)
                .inner_size(1100.0, 760.0)
                .visible(false)
                .devtools(false)
                .incognito(true)
                .on_navigation(move |destination| {
                    policy_for_navigation.allows_navigation_to(destination)
                })
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .on_download(|_, event| !matches!(event, DownloadEvent::Requested { .. }))
                .on_page_load(move |window, payload| {
                    if payload.event() != PageLoadEvent::Finished
                        || payload.url().as_str() != BOOTSTRAP_URL
                        || started_for_load.swap(true, Ordering::AcqRel)
                    {
                        return;
                    }

                    // The only non-loopback navigation is the inert bootstrap page.
                    // Apply native WebView2 settings before loading the backend UI.
                    policy_for_load.finish_bootstrap();
                    let app_handle = app_for_load.clone();
                    let ui_window = window.clone();
                    let initial_url = initial_url.clone();
                    if let Err(error) = window.with_webview(move |platform_webview| {
                        let initialized = configure_native_webview(&platform_webview)
                            .and_then(|()| {
                                ui_window
                                    .navigate(initial_url)
                                    .context("failed to navigate to the local UI")
                            })
                            .and_then(|()| {
                                ui_window
                                    .show()
                                    .context("failed to show the local UI window")
                            })
                            .and_then(|()| write_readiness_marker());
                        if let Err(error) = initialized {
                            eprintln!(
                                "failed to initialize restricted local UI WebView: {error:#}"
                            );
                            app_handle.exit(1);
                        }
                    }) {
                        eprintln!("failed to access the local UI WebView: {error:#}");
                        app_for_load.exit(1);
                    }
                })
                .build()
                .context("failed to create local UI WebView window")?;

            watch_parent_stdin(app_handle)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .context("failed to run local UI WebView")
}

fn configure_native_webview(webview: &tauri::webview::PlatformWebview) -> Result<()> {
    let core = unsafe { webview.controller().CoreWebView2() }
        .context("failed to access the WebView2 controller")?;
    let settings = unsafe { core.Settings() }.context("failed to access WebView2 settings")?;
    let settings3: ICoreWebView2Settings3 = settings
        .cast()
        .context("WebView2 runtime does not support required browser settings")?;

    unsafe {
        settings
            .SetAreDefaultContextMenusEnabled(false)
            .context("failed to disable WebView2 context menus")?;
        settings
            .SetAreDevToolsEnabled(false)
            .context("failed to disable WebView2 developer tools")?;
        settings3
            .SetAreBrowserAcceleratorKeysEnabled(false)
            .context("failed to disable WebView2 browser accelerator keys")?;
    }
    Ok(())
}

fn write_readiness_marker() -> Result<()> {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    stdout
        .write_all(READY_MARKER)
        .context("failed to write local UI readiness marker")?;
    stdout
        .flush()
        .context("failed to flush local UI readiness marker")?;
    Ok(())
}

fn watch_parent_stdin(app_handle: AppHandle) -> Result<()> {
    watch_parent_pipe(std::io::stdin(), move || app_handle.exit(0))
}

fn watch_parent_pipe<R, F>(mut reader: R, on_close: F) -> Result<()>
where
    R: Read + Send + 'static,
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .name("local-ui-parent-monitor".to_owned())
        .spawn(move || {
            let mut byte = [0_u8; 1];
            loop {
                match reader.read(&mut byte) {
                    Ok(0) => {
                        on_close();
                        break;
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => {
                        on_close();
                        break;
                    }
                }
            }
        })
        .context("failed to start local UI lifetime monitor")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        parse_helper_arguments, watch_parent_pipe, LocalUiNavigationPolicy, LocalUiOrigin,
        BOOTSTRAP_URL,
    };
    use std::ffi::OsString;
    use std::io::Cursor;
    use std::sync::mpsc;
    use std::time::Duration;
    use url::Url;

    #[test]
    fn accepts_only_one_explicit_loopback_url_with_a_valid_port() {
        let (url, origin) =
            parse_helper_arguments([OsString::from("http://127.0.0.1:8080/worker?tab=queue")])
                .unwrap();
        assert_eq!(url.as_str(), "http://127.0.0.1:8080/worker?tab=queue");
        assert_eq!(origin, LocalUiOrigin { port: 8080 });

        assert!(parse_helper_arguments([]).is_err());
        assert!(parse_helper_arguments([
            OsString::from("http://127.0.0.1:8080/"),
            OsString::from("extra"),
        ])
        .is_err());

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
                parse_helper_arguments([OsString::from(invalid)]).is_err(),
                "unexpectedly accepted {invalid}"
            );
        }
    }

    #[test]
    fn navigation_is_restricted_to_the_exact_origin_after_bootstrap() {
        let policy = LocalUiNavigationPolicy::new(LocalUiOrigin { port: 8080 });
        assert!(policy.allows_navigation_to(&Url::parse(BOOTSTRAP_URL).unwrap()));
        assert!(!policy.allows_navigation_to(&Url::parse(BOOTSTRAP_URL).unwrap()));
        assert!(policy.allows_navigation_to(&Url::parse("http://127.0.0.1:8080/tasks/1").unwrap()));
        assert!(
            policy.allows_navigation_to(&Url::parse("http://127.0.0.1:8080/?tab=queue").unwrap())
        );
        for destination in [
            "https://127.0.0.1:8080/",
            "http://127.0.0.1:8081/",
            "http://example.com/",
            "http://user@127.0.0.1:8080/",
            "about:blank",
        ] {
            assert!(
                !policy.allows_navigation_to(&Url::parse(destination).unwrap()),
                "unexpectedly allowed {destination}"
            );
        }
    }

    #[test]
    fn parent_pipe_eof_requests_helper_exit() {
        let (closed_tx, closed_rx) = mpsc::channel();
        watch_parent_pipe(Cursor::new(Vec::<u8>::new()), move || {
            closed_tx.send(()).unwrap();
        })
        .unwrap();
        closed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("EOF should request helper exit");
    }
}

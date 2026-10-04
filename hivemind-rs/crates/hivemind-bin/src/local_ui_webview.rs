use anyhow::{anyhow, Context, Result};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, WebviewWindowBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WindowEvent};
use url::Url;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
use webview2_com::{take_pwstr, WebMessageReceivedEventHandler};
use windows_core::{Interface, PWSTR};

const READY_MARKER: &[u8] = hivemind_client_runtime::LOCAL_UI_READY_MARKER;
const LOCALHOST_PREFIX: &str = "http://127.0.0.1:";
const WINDOW_LABEL: &str = "local-ui";
const BOOTSTRAP_URL: &str = "about:blank";
const CONTROL_PREFIX: &str = "/__hivemind_desktop__/";
const QUIT_MARKER: &[u8] = hivemind_client_runtime::LOCAL_UI_QUIT_MARKER;
const SHOW_MARKER: &[u8] = hivemind_client_runtime::LOCAL_UI_SHOW_MARKER;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum WindowAction {
    Ready,
    Request,
    Cancel,
    Background,
    Quit,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowMessage {
    channel: String,
    nonce: String,
    action: WindowAction,
    id: u32,
}

struct WindowLifecycle {
    origin: LocalUiOrigin,
    nonce: String,
    close_pending: AtomicBool,
}

impl WindowLifecycle {
    fn new(origin: LocalUiOrigin) -> Self {
        Self {
            origin,
            nonce: uuid::Uuid::new_v4().simple().to_string(),
            close_pending: AtomicBool::new(false),
        }
    }

    fn parse_message(&self, source: &str, message: &str) -> Option<(WindowAction, u32)> {
        let source = Url::parse(source).ok()?;
        if !self.origin.allows_navigation_to(&source) || message.len() > 512 {
            return None;
        }
        let message: WindowMessage = serde_json::from_str(message).ok()?;
        if message.channel != "hivemind-window-v1" || message.nonce != self.nonce || message.id == 0
        {
            return None;
        }
        Some((message.action, message.id))
    }

    fn initialization_script(&self) -> Result<String> {
        let origin = serde_json::to_string(&format!("http://127.0.0.1:{}", self.origin.port))?;
        let nonce = serde_json::to_string(&self.nonce)?;
        Ok(include_str!("local_ui_bridge.js")
            .replace("__HIVEMIND_ORIGIN__", &origin)
            .replace("__HIVEMIND_NONCE__", &nonce))
    }
}

fn show_window(app: &AppHandle) -> Result<()> {
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .context("local UI window is unavailable")?;
    window.unminimize()?;
    window.show()?;
    window.set_focus()?;
    window.eval("window.dispatchEvent(new CustomEvent('hivemind:window-shown'))")?;
    Ok(())
}

fn request_close(app: &AppHandle, lifecycle: &WindowLifecycle) -> Result<()> {
    if lifecycle.close_pending.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        if let Err(error) =
            window.eval("window.dispatchEvent(new CustomEvent('hivemind:close-requested'))")
        {
            lifecycle.close_pending.store(false, Ordering::Release);
            return Err(error.into());
        }
    }
    Ok(())
}

fn handle_window_action(
    app: &AppHandle,
    lifecycle: &WindowLifecycle,
    action: WindowAction,
    id: u32,
) {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    let result: Result<()> = (|| {
        match action {
            WindowAction::Ready => {
                if lifecycle.close_pending.load(Ordering::Acquire) {
                    window.eval(
                        "window.dispatchEvent(new CustomEvent('hivemind:close-requested'))",
                    )?;
                }
            }
            WindowAction::Request => request_close(app, lifecycle)?,
            WindowAction::Cancel => {
                lifecycle.close_pending.store(false, Ordering::Release);
            }
            WindowAction::Background | WindowAction::Quit => {
                if !lifecycle.close_pending.swap(false, Ordering::AcqRel) {
                    return Err(anyhow!("no window close request is pending"));
                }
                let result = if action == WindowAction::Background {
                    window.hide().map_err(anyhow::Error::from)
                } else {
                    write_pipe_marker(QUIT_MARKER)
                };
                if result.is_err() {
                    lifecycle.close_pending.store(true, Ordering::Release);
                }
                result?;
            }
        }
        Ok(())
    })();
    let ok = result.is_ok();
    if let Err(error) = result {
        eprintln!("Local UI window action failed: {error}");
    }
    let _ = window.eval(format!(
        "window.dispatchEvent(new CustomEvent('hivemind:host-response',{{detail:{{id:{id},ok:{ok}}}}}))"
    ));
    if ok && action == WindowAction::Quit {
        app.exit(0);
    }
}

fn install_tray(app: &AppHandle, lifecycle: Arc<WindowLifecycle>, title: &str) -> Result<()> {
    let show = MenuItem::with_id(app, "show", "Open", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit…", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut pixels = vec![0_u8; 32 * 32 * 4];
    for y in 4..28 {
        for x in 4..28 {
            let index = (y * 32 + x) * 4;
            let ink = (7..11).contains(&x) || (21..25).contains(&x) || (14..18).contains(&y);
            let value = if ink { 245 } else { 32 };
            pixels[index..index + 4].copy_from_slice(&[value, value, value, 255]);
        }
    }
    TrayIconBuilder::with_id("local-ui-tray")
        .icon(tauri::image::Image::new_owned(pixels, 32, 32))
        .tooltip(title)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                if let Err(error) = show_window(tray.app_handle()) {
                    eprintln!("Could not restore local UI: {error}");
                }
            }
        })
        .on_menu_event(move |app, event| {
            if let Err(error) = show_window(app) {
                eprintln!("Could not restore local UI: {error}");
                return;
            }
            if event.id.as_ref() == "quit" {
                if let Err(error) = request_close(app, &lifecycle) {
                    eprintln!("Could not request local UI close: {error}");
                }
            }
        })
        .build(app)
        .context("failed to create local UI system tray")?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LocalUiOrigin {
    port: u16,
}

impl LocalUiOrigin {
    fn from_initial_url(input: &str) -> std::result::Result<(Self, Url), String> {
        let remainder = input
            .strip_prefix(LOCALHOST_PREFIX)
            .ok_or_else(|| "URL must begin with http://127.0.0.1:<port>/".to_owned())?;
        let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
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

        Ok((Self { port }, parsed))
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
    let (origin, url) = LocalUiOrigin::from_initial_url(&argument).map_err(anyhow::Error::msg)?;
    Ok((url, origin))
}

pub fn run(window_title: &str) -> Result<()> {
    let (url, origin) = parse_helper_arguments(std::env::args_os().skip(1))?;
    let bootstrap_url = Url::parse(BOOTSTRAP_URL).context("invalid local UI bootstrap URL")?;
    let window_title = window_title.to_owned();
    let lifecycle = Arc::new(WindowLifecycle::new(origin));
    let lifecycle_for_events = Arc::clone(&lifecycle);

    tauri::Builder::default()
        .on_window_event(move |window, event| {
            if window.label() == WINDOW_LABEL {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Err(error) = request_close(window.app_handle(), &lifecycle_for_events) {
                        eprintln!("Could not request local UI close: {error}");
                    }
                }
            }
        })
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let navigation_policy = Arc::new(LocalUiNavigationPolicy::new(origin));
            let policy_for_navigation = Arc::clone(&navigation_policy);
            let lifecycle_for_load = Arc::clone(&lifecycle);
            let lifecycle_for_webview = Arc::clone(&lifecycle);
            let app_for_webview = app_handle.clone();
            let initialization_script = lifecycle.initialization_script()?;
            let window =
                WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(bootstrap_url))
                    .title(&window_title)
                    .inner_size(1100.0, 760.0)
                    .min_inner_size(360.0, 480.0)
                    .initialization_script(&initialization_script)
                    .visible(false)
                    .devtools(false)
                    .incognito(true)
                    .on_navigation(move |destination| {
                        !destination.path().starts_with(CONTROL_PREFIX)
                            && policy_for_navigation.allows_navigation_to(destination)
                    })
                    .on_page_load(move |webview, payload| {
                        if payload.event() == PageLoadEvent::Finished
                            && origin.allows_navigation_to(payload.url())
                            && lifecycle_for_load.close_pending.load(Ordering::Acquire)
                        {
                            let app = webview.app_handle().clone();
                            let lifecycle = Arc::clone(&lifecycle_for_load);
                            let dispatcher = app.clone();
                            let _ = dispatcher.run_on_main_thread(move || {
                                if lifecycle.close_pending.load(Ordering::Acquire) {
                                    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
                                        let _ = window.eval("window.dispatchEvent(new CustomEvent('hivemind:close-requested'))");
                                    }
                                }
                            });
                        }
                    })
                    .on_new_window(|_, _| NewWindowResponse::Deny)
                    .on_download(|_, event| !matches!(event, DownloadEvent::Requested { .. }))
                    .build()
                    .context("failed to create local UI WebView window")?;

            let (settings_tx, settings_rx) = mpsc::sync_channel(1);
            window
                .with_webview(move |platform_webview| {
                    let initialized = configure_native_webview(
                        &platform_webview,
                        app_for_webview,
                        lifecycle_for_webview,
                    )
                    .map(|()| {
                        // The only non-loopback navigation is the inert bootstrap page.
                        // Apply native WebView2 settings before loading the backend UI.
                        navigation_policy.finish_bootstrap();
                    });
                    let _ = settings_tx.send(initialized);
                })
                .context("failed to access the local UI WebView")?;
            settings_rx
                .try_recv()
                .context("local UI WebView settings callback did not complete")??;

            // Wry invokes with_webview inline while holding the window ID lock.
            // Navigate only after it returns so the dispatcher can acquire that lock.
            install_tray(&app_handle, Arc::clone(&lifecycle), &window_title)?;
            window
                .navigate(url)
                .context("failed to navigate to the local UI")?;
            window
                .show()
                .context("failed to show the local UI window")?;
            watch_parent_stdin(app_handle)?;
            write_readiness_marker()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .context("failed to run local UI WebView")
}

fn configure_native_webview(
    webview: &tauri::webview::PlatformWebview,
    app: AppHandle,
    lifecycle: Arc<WindowLifecycle>,
) -> Result<()> {
    let core = unsafe { webview.controller().CoreWebView2() }
        .context("failed to access the WebView2 controller")?;
    let mut token = 0;
    unsafe {
        core.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut source = PWSTR::null();
                args.Source(&mut source)?;
                let source = take_pwstr(source);
                let mut message = PWSTR::null();
                if args.TryGetWebMessageAsString(&mut message).is_err() {
                    return Ok(());
                }
                let message = take_pwstr(message);
                if let Some((action, id)) = lifecycle.parse_message(&source, &message) {
                    let app = app.clone();
                    let lifecycle = Arc::clone(&lifecycle);
                    let dispatcher = app.clone();
                    // Dispatch outside Wry's WebView lock. This channel grants only window actions.
                    let _ = dispatcher.run_on_main_thread(move || {
                        handle_window_action(&app, &lifecycle, action, id);
                    });
                }
                Ok(())
            })),
            &mut token,
        )
        .context("failed to install the local window lifecycle handler")?;
    }
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
    write_pipe_marker(READY_MARKER)
}

fn write_pipe_marker(marker: &[u8]) -> Result<()> {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    stdout
        .write_all(marker)
        .context("failed to write local UI pipe marker")?;
    stdout
        .flush()
        .context("failed to flush local UI readiness marker")?;
    Ok(())
}

fn watch_parent_stdin(app_handle: AppHandle) -> Result<()> {
    let on_close = app_handle.clone();
    watch_parent_pipe(
        std::io::stdin(),
        move || {
            let app = app_handle.clone();
            if let Err(error) = app_handle.run_on_main_thread(move || {
                if let Err(error) = show_window(&app) {
                    eprintln!("Could not restore existing local UI: {error}");
                }
            }) {
                eprintln!("Could not dispatch local UI restoration: {error}");
            }
        },
        move || on_close.exit(0),
    )
}

fn watch_parent_pipe<R, S, F>(mut reader: R, on_show: S, on_close: F) -> Result<()>
where
    R: Read + Send + 'static,
    S: Fn() + Send + 'static,
    F: FnOnce() + Send + 'static,
{
    std::thread::Builder::new()
        .name("local-ui-parent-monitor".to_owned())
        .spawn(move || {
            let mut bytes = [0_u8; 1024];
            let mut frame = [0_u8; SHOW_MARKER.len()];
            let mut length = 0;
            let mut overflow = false;
            loop {
                match reader.read(&mut bytes) {
                    Ok(0) => {
                        on_close();
                        break;
                    }
                    Ok(read) => {
                        for byte in &bytes[..read] {
                            if length < frame.len() {
                                frame[length] = *byte;
                                length += 1;
                            } else {
                                overflow = true;
                            }
                            if *byte == b'\n' {
                                if !overflow && length == frame.len() && frame == SHOW_MARKER {
                                    on_show();
                                }
                                length = 0;
                                overflow = false;
                            }
                        }
                    }
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
        WindowAction, WindowLifecycle, BOOTSTRAP_URL,
    };
    use std::ffi::OsString;
    use std::io::{Cursor, Read};
    use std::sync::mpsc;
    use std::time::Duration;
    use url::Url;

    struct ChunkedReader<R> {
        inner: R,
        chunk_size: usize,
    }

    impl<R: Read> Read for ChunkedReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let limit = buffer.len().min(self.chunk_size);
            self.inner.read(&mut buffer[..limit])
        }
    }

    #[test]
    fn lifecycle_actions_require_the_private_nonce_and_exact_origin() {
        let lifecycle = WindowLifecycle::new(LocalUiOrigin { port: 8080 });
        let source = "http://127.0.0.1:8080/";
        let message = serde_json::json!({
            "channel": "hivemind-window-v1", "nonce": lifecycle.nonce,
            "action": "background", "id": 1,
        })
        .to_string();
        assert_eq!(
            lifecycle.parse_message(source, &message),
            Some((WindowAction::Background, 1))
        );
        for invalid_source in [
            "http://127.0.0.1:8081/",
            "https://127.0.0.1:8080/",
            "http://localhost:8080/",
            "about:blank",
            "https://example.com/",
        ] {
            assert!(lifecycle.parse_message(invalid_source, &message).is_none());
        }
        for invalid_message in [
            message.replace(&lifecycle.nonce, "unknown"),
            message.replace("background", "shell"),
            message.replace("hivemind-window-v1", "other-channel"),
            message.replace("\"id\":1", "\"id\":0"),
            message.replace("\"id\":1", "\"id\":-1"),
            message.replace("\"id\":1", "\"id\":1,\"command\":\"shell\""),
            "invalid-json".to_owned(),
            " ".repeat(513),
        ] {
            assert!(
                lifecycle.parse_message(source, &invalid_message).is_none(),
                "accepted {invalid_message}"
            );
        }
    }

    #[test]
    fn lifecycle_script_exposes_no_tauri_network_filesystem_or_shell_permission() {
        let lifecycle = WindowLifecycle::new(LocalUiOrigin { port: 8080 });
        let script = lifecycle.initialization_script().unwrap();
        assert!(script.contains("http://127.0.0.1:8080"));
        assert!(script.contains(&lifecycle.nonce));
        assert!(script.contains("Object.freeze"));
        assert!(!script.contains("__TAURI"));
        assert!(!script.contains("__HIVEMIND_NONCE__"));
        assert!(!script.contains("fetch("));
        assert!(!script.contains("location.assign"));
        assert!(script.contains("postMessage(JSON.stringify("));
    }

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
    fn finishing_bootstrap_denies_blank_but_allows_the_configured_loopback_origin() {
        let policy = LocalUiNavigationPolicy::new(LocalUiOrigin { port: 8080 });
        policy.finish_bootstrap();

        assert!(!policy.allows_navigation_to(&Url::parse(BOOTSTRAP_URL).unwrap()));
        assert!(policy.allows_navigation_to(&Url::parse("http://127.0.0.1:8080/").unwrap()));
        assert!(!policy.allows_navigation_to(&Url::parse("http://127.0.0.1:8081/").unwrap()));
    }

    #[test]
    fn parent_pipe_accepts_split_and_multiple_complete_show_frames() {
        let (show_tx, show_rx) = mpsc::channel();
        let (closed_tx, closed_rx) = mpsc::channel();
        let mut bytes = super::SHOW_MARKER.to_vec();
        bytes.extend_from_slice(b"NOPE\n");
        bytes.extend_from_slice(&vec![b'x'; 4096]);
        bytes.extend_from_slice(b"\n");
        bytes.extend_from_slice(super::SHOW_MARKER);
        bytes.extend_from_slice(&super::SHOW_MARKER[..super::SHOW_MARKER.len() - 1]);
        watch_parent_pipe(
            ChunkedReader {
                inner: Cursor::new(bytes),
                chunk_size: 3,
            },
            move || {
                show_tx.send(()).unwrap();
            },
            move || {
                closed_tx.send(()).unwrap();
            },
        )
        .unwrap();
        closed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(show_rx.try_iter().count(), 2);
    }

    #[test]
    fn parent_pipe_eof_requests_helper_exit() {
        let (closed_tx, closed_rx) = mpsc::channel();
        watch_parent_pipe(
            Cursor::new(Vec::<u8>::new()),
            || {},
            move || {
                closed_tx.send(()).unwrap();
            },
        )
        .unwrap();
        closed_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("EOF should request helper exit");
    }
}

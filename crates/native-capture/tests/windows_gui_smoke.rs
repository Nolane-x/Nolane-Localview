#[cfg(not(all(windows, feature = "gui-smoke")))]
fn main() {}

#[cfg(all(windows, feature = "gui-smoke"))]
fn main() {
    windows_smoke::run();
}

#[cfg(all(windows, feature = "gui-smoke"))]
mod windows_smoke {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    use localview_capture::CaptureTarget;
    use localview_native_capture::{
        capture_webview2_for_gui_smoke, CaptureRequest, NativeCaptureBackend, ViewportMeta,
    };
    use localview_visual::decode_png_rgba;
    use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
    use windows::{
        core::{w, PCWSTR},
        Win32::{
            Foundation::{E_POINTER, HWND, RECT},
            System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, ShowWindow, CW_USEDEFAULT, SW_SHOW,
                WS_OVERLAPPEDWINDOW,
            },
        },
    };

    const SMOKE_WIDTH: u32 = 320;
    const SMOKE_HEIGHT: u32 = 180;
    const FIXTURE_HTML: &str = r#"<!doctype html>
<html>
<head><style>
html, body { margin: 0; width: 100%; height: 100%; background: rgb(18, 52, 86); overflow: hidden; }
#proof { position: fixed; left: 25%; top: 25%; width: 50%; height: 50%; background: rgb(220, 40, 60); }
</style></head>
<body><div id="proof"></div></body>
</html>"#;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn start_fixture_server() -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind deterministic loopback fixture");
        let address = listener.local_addr().expect("read fixture server address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept WebView2 fixture request");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{}",
                FIXTURE_HTML.len(),
                FIXTURE_HTML
            );
            stream
                .write_all(response.as_bytes())
                .expect("write deterministic WebView2 fixture");
            stream.flush().expect("flush deterministic WebView2 fixture");
        });
        (format!("http://{address}/"), handle)
    }

    fn create_window() -> HWND {
        unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("LocalView WebView2 GUI smoke"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                480,
                320,
                None,
                None,
                None,
                None,
            )
            .expect("create Win32 parent window")
        }
    }

    fn create_environment() -> ICoreWebView2Environment {
        let (tx, rx) = mpsc::channel();
        CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
            Box::new(|handler| unsafe {
                CreateCoreWebView2Environment(&handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, environment| {
                error_code?;
                tx.send(environment.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                    .expect("send WebView2 environment");
                Ok(())
            }),
        )
        .expect("start WebView2 environment creation");
        rx.recv()
            .expect("receive WebView2 environment")
            .expect("WebView2 environment creation must succeed")
    }

    fn create_controller(
        environment: ICoreWebView2Environment,
        parent: HWND,
    ) -> ICoreWebView2Controller {
        let (tx, rx) = mpsc::channel();
        CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                environment
                    .CreateCoreWebView2Controller(parent, &handler)
                    .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(move |error_code, controller| {
                error_code?;
                tx.send(controller.ok_or_else(|| windows::core::Error::from(E_POINTER)))
                    .expect("send WebView2 controller");
                Ok(())
            }),
        )
        .expect("start WebView2 controller creation");
        rx.recv()
            .expect("receive WebView2 controller")
            .expect("WebView2 controller creation must succeed")
    }

    fn navigate_and_wait(webview: &ICoreWebView2, route: &str) {
        let (tx, rx) = mpsc::channel();
        let handler = NavigationCompletedEventHandler::create(Box::new(move |_sender, _args| {
            tx.send(()).expect("send WebView2 navigation completion");
            Ok(())
        }));
        let mut token = 0_i64;
        let route_wide = wide(route);
        unsafe {
            webview
                .add_NavigationCompleted(&handler, &mut token)
                .expect("subscribe WebView2 navigation completion");
            webview
                .Navigate(PCWSTR(route_wide.as_ptr()))
                .expect("navigate WebView2 to loopback fixture");
        }
        webview2_com::wait_with_pump(rx).expect("WebView2 loopback fixture must finish loading");
        unsafe {
            webview
                .remove_NavigationCompleted(token)
                .expect("remove WebView2 navigation completion handler");
        }
    }

    fn execute_script(webview: &ICoreWebView2, script: &str) -> String {
        let (tx, rx) = mpsc::channel();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |error_code, result| {
            error_code?;
            tx.send(webview2_com::string_from_pcwstr(&result))
                .expect("send WebView2 script result");
            Ok(())
        }));
        let script_wide = wide(script);
        unsafe {
            webview
                .ExecuteScript(PCWSTR(script_wide.as_ptr()), &handler)
                .expect("execute WebView2 readiness diagnostic");
        }
        webview2_com::wait_with_pump(rx).expect("WebView2 readiness diagnostic must complete")
    }

    fn assert_fixture_dom_ready(webview: &ICoreWebView2) {
        let result = execute_script(
            webview,
            r#"(() => {
                const proof = document.getElementById('proof');
                if (!proof) return { ready: document.readyState, missing: true };
                const rect = proof.getBoundingClientRect();
                return {
                    ready: document.readyState,
                    missing: false,
                    color: getComputedStyle(proof).backgroundColor,
                    rect: [rect.left, rect.top, rect.width, rect.height],
                    viewport: [innerWidth, innerHeight],
                    href: location.href
                };
            })()"#,
        );
        let value: serde_json::Value =
            serde_json::from_str(&result).expect("WebView2 diagnostic must be valid JSON");
        assert_eq!(value["ready"], "complete", "unexpected DOM readiness: {value}");
        assert_eq!(value["missing"], false, "proof node missing: {value}");
        assert_eq!(
            value["color"], "rgb(220, 40, 60)",
            "proof CSS did not apply: {value}"
        );
        assert_eq!(value["viewport"][0], SMOKE_WIDTH, "unexpected viewport: {value}");
        assert_eq!(value["viewport"][1], SMOKE_HEIGHT, "unexpected viewport: {value}");
        assert_eq!(value["rect"][0], 80.0, "unexpected proof geometry: {value}");
        assert_eq!(value["rect"][1], 45.0, "unexpected proof geometry: {value}");
        assert_eq!(value["rect"][2], 160.0, "unexpected proof geometry: {value}");
        assert_eq!(value["rect"][3], 90.0, "unexpected proof geometry: {value}");
    }

    pub fn run() {
        assert!(
            std::env::var_os("LOCALVIEW_GUI_SMOKE").is_some(),
            "Windows GUI smoke must be explicitly enabled"
        );

        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .expect("initialize STA COM for WebView2 GUI smoke");
        }

        let parent = create_window();
        unsafe {
            let _ = ShowWindow(parent, SW_SHOW);
        }

        let environment = create_environment();
        let controller = create_controller(environment, parent);
        unsafe {
            controller
                .SetBounds(RECT {
                    left: 0,
                    top: 0,
                    right: SMOKE_WIDTH as i32,
                    bottom: SMOKE_HEIGHT as i32,
                })
                .expect("set deterministic WebView2 bounds");
            controller
                .SetIsVisible(true)
                .expect("make WebView2 controller visible");
        }
        let webview = unsafe {
            controller
                .CoreWebView2()
                .expect("obtain real CoreWebView2 from controller")
        };

        let (route, server) = start_fixture_server();
        navigate_and_wait(&webview, &route);
        server.join().expect("loopback fixture server must finish");
        assert_fixture_dom_ready(&webview);

        let request = CaptureRequest {
            target: CaptureTarget::Viewport,
            viewport: ViewportMeta {
                css_width: SMOKE_WIDTH,
                css_height: SMOKE_HEIGHT,
                device_scale_factor: 1.0,
            },
            route: route.clone(),
            revision: Some("windows-gui-smoke".into()),
        };
        let (tx, rx) = mpsc::channel();
        capture_webview2_for_gui_smoke(&webview, request, move |result| {
            tx.send(result).expect("send WebView2 capture result");
        });
        let frame = webview2_com::wait_with_pump(rx)
            .expect("WebView2 capture callback must complete")
            .expect("real WebView2 CapturePreview must succeed");

        assert_eq!(frame.backend, NativeCaptureBackend::WebView2);
        assert_eq!(frame.route, route);
        assert_eq!(frame.revision.as_deref(), Some("windows-gui-smoke"));
        assert!(frame.pixel_width > 0 && frame.pixel_height > 0);
        assert!(
            frame.png.len() > 256,
            "real rendered PNG must not be a trivial header"
        );

        let decoded = decode_png_rgba(&frame.png).expect("captured PNG must fully decode");
        assert_eq!(
            (decoded.width, decoded.height),
            (frame.pixel_width, frame.pixel_height)
        );
        let center_x = decoded.width / 2;
        let center_y = decoded.height / 2;
        let offset = ((center_y * decoded.width + center_x) * 4) as usize;
        let center = &decoded.data[offset..offset + 4];
        assert!(
            center[0] >= 190 && center[1] <= 70 && center[2] <= 90 && center[3] >= 240,
            "center pixel must prove the red fixture region was actually rendered; got {center:?}"
        );

        unsafe {
            controller.Close().expect("close WebView2 controller");
            let _ = DestroyWindow(parent);
            CoUninitialize();
        }
    }
}

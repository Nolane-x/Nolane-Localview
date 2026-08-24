#[cfg(not(all(windows, feature = "gui-smoke")))]
fn main() {}

#[cfg(all(windows, feature = "gui-smoke"))]
fn main() {
    windows_smoke::run();
}

#[cfg(all(windows, feature = "gui-smoke"))]
mod windows_smoke {
    use std::{
        cell::{Cell, RefCell},
        io::{Read, Write},
        net::TcpListener,
        rc::Rc,
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
        core::{w, PCWSTR, PWSTR},
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

    fn start_fixture_server() -> (String, thread::JoinHandle<()>, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind deterministic loopback fixture");
        let address = listener.local_addr().expect("read fixture server address");
        let (request_tx, request_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept WebView2 fixture request");
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).expect("read WebView2 fixture request");
            let request_text = String::from_utf8_lossy(&request[..count]);
            let request_line = request_text.lines().next().unwrap_or("<empty request>").to_owned();
            request_tx
                .send(format!("bytes={count}; line={request_line}"))
                .expect("send WebView2 fixture request trace");
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
        (format!("http://{address}/"), handle, request_rx)
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

    fn navigate_and_wait(
        webview: &ICoreWebView2,
        route: &str,
        request_rx: &mpsc::Receiver<String>,
    ) {
        let expected_navigation_id = Rc::new(Cell::new(None::<u64>));
        let event_trace = Rc::new(RefCell::new(Vec::<String>::new()));
        let expected_route = route.to_owned();

        let starting_navigation_id = expected_navigation_id.clone();
        let starting_trace = event_trace.clone();
        let starting_handler = NavigationStartingEventHandler::create(Box::new(
            move |_sender, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut uri = PWSTR::null();
                let mut navigation_id = 0_u64;
                unsafe {
                    args.Uri(&mut uri)?;
                    args.NavigationId(&mut navigation_id)?;
                }
                let uri = webview2_com::take_pwstr(uri);
                starting_trace
                    .borrow_mut()
                    .push(format!("start id={navigation_id} uri={uri}"));
                if uri == expected_route {
                    starting_navigation_id.set(Some(navigation_id));
                }
                Ok(())
            },
        ));

        let (tx, rx) = mpsc::channel();
        let completed_navigation_id = expected_navigation_id.clone();
        let completed_trace = event_trace.clone();
        let completed_handler = NavigationCompletedEventHandler::create(Box::new(
            move |_sender, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let mut navigation_id = 0_u64;
                let mut is_success = Default::default();
                let mut web_error_status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                unsafe {
                    args.NavigationId(&mut navigation_id)?;
                    args.IsSuccess(&mut is_success)?;
                    args.WebErrorStatus(&mut web_error_status)?;
                }
                completed_trace.borrow_mut().push(format!(
                    "complete id={navigation_id} success={} status={web_error_status:?}",
                    is_success.as_bool()
                ));
                if completed_navigation_id.get() != Some(navigation_id) {
                    return Ok(());
                }
                tx.send((is_success.as_bool(), web_error_status))
                    .expect("send correlated WebView2 navigation completion");
                Ok(())
            },
        ));

        let mut starting_token = 0_i64;
        let mut completed_token = 0_i64;
        let route_wide = wide(route);
        unsafe {
            webview
                .add_NavigationStarting(&starting_handler, &mut starting_token)
                .expect("subscribe WebView2 navigation starting");
            webview
                .add_NavigationCompleted(&completed_handler, &mut completed_token)
                .expect("subscribe WebView2 navigation completion");
            webview
                .Navigate(PCWSTR(route_wide.as_ptr()))
                .expect("navigate WebView2 to loopback fixture");
        }

        let (navigation_succeeded, web_error_status) = webview2_com::wait_with_pump(rx)
            .expect("correlated WebView2 loopback fixture navigation must complete");
        let trace = event_trace.borrow().join(" | ");
        let request_trace = request_rx
            .try_recv()
            .unwrap_or_else(|_| "<no HTTP request observed before completion>".to_owned());
        assert!(
            navigation_succeeded,
            "correlated WebView2 loopback fixture navigation must succeed; WebErrorStatus={web_error_status:?}; HTTP={request_trace}; events={trace}"
        );
        assert!(
            expected_navigation_id.get().is_some(),
            "fixture NavigationStarting must establish a navigation id; events={trace}"
        );

        unsafe {
            webview
                .remove_NavigationStarting(starting_token)
                .expect("remove WebView2 navigation starting handler");
            webview
                .remove_NavigationCompleted(completed_token)
                .expect("remove WebView2 navigation completion handler");
        }
    }

    fn execute_script(webview: &ICoreWebView2, script: &str) -> String {
        let (tx, rx) = mpsc::channel();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |error_code, result| {
            error_code?;
            tx.send(result).expect("send WebView2 script result");
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

    fn assert_fixture_dom_ready(webview: &ICoreWebView2, route: &str) {
        let result = execute_script(
            webview,
            r#"(() => {
                const proof = document.getElementById('proof');
                if (!proof) return { ready: document.readyState, missing: true, href: location.href };
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
        assert_eq!(value["href"], route, "unexpected fixture route: {value}");
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

        let (route, server, request_rx) = start_fixture_server();
        navigate_and_wait(&webview, &route, &request_rx);
        server.join().expect("loopback fixture server must finish");
        assert_fixture_dom_ready(&webview, &route);

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

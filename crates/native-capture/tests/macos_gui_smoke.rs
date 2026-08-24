#[cfg(not(all(target_os = "macos", feature = "gui-smoke")))]
fn main() {}

#[cfg(all(target_os = "macos", feature = "gui-smoke"))]
fn main() {
    macos::run();
}

#[cfg(all(target_os = "macos", feature = "gui-smoke"))]
mod macos {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    use localview_capture::CaptureTarget;
    use localview_native_capture::{
        capture_wk_webview_for_gui_smoke, CaptureRequest, NativeCaptureBackend, ViewportMeta,
    };
    use localview_visual::decode_png_rgba;
    use objc2_app_kit::{NSApplication, NSBackingStoreType, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{
        MainThreadMarker, NSDate, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURL,
    };
    use objc2_web_kit::WKWebView;

    const SMOKE_WIDTH: u32 = 320;
    const SMOKE_HEIGHT: u32 = 180;

    fn pump_until(deadline: Instant, mut done: impl FnMut() -> bool) -> bool {
        let run_loop = NSRunLoop::currentRunLoop();
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        }
        done()
    }

    fn pump_for(duration: Duration) {
        let deadline = Instant::now() + duration;
        let run_loop = NSRunLoop::currentRunLoop();
        while Instant::now() < deadline {
            run_loop.runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        }
    }

    pub fn run() {
        assert!(
            std::env::var_os("LOCALVIEW_GUI_SMOKE").is_some(),
            "macOS GUI smoke must be explicitly enabled"
        );

        let mtm = MainThreadMarker::new().expect("macOS GUI smoke must run on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.finishLaunching();

        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(SMOKE_WIDTH as f64, SMOKE_HEIGHT as f64),
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };

        let view = unsafe { WKWebView::new(mtm) };
        view.setFrame(frame);
        window.setContentView(Some(&view));
        window.orderFrontRegardless();

        let html = NSString::from_str(
            r#"<!doctype html>
<html>
<head><style>
html, body { margin: 0; width: 100%; height: 100%; background: rgb(18, 52, 86); overflow: hidden; }
#proof { position: fixed; left: 25%; top: 25%; width: 50%; height: 50%; background: rgb(220, 40, 60); }
</style></head>
<body><div id="proof"></div></body>
</html>"#,
        );
        let base_url = NSURL::URLWithString(&NSString::from_str("http://127.0.0.1/"))
            .expect("loopback base URL must parse");
        unsafe {
            view.loadHTMLString_baseURL(&html, Some(&base_url));
        }

        assert!(
            pump_until(Instant::now() + Duration::from_secs(8), || unsafe {
                !view.isLoading() && view.estimatedProgress() >= 1.0
            }),
            "deterministic WKWebView fixture did not finish loading"
        );
        window.displayIfNeeded();
        view.displayIfNeeded();
        pump_for(Duration::from_millis(150));

        let request = CaptureRequest {
            target: CaptureTarget::Viewport,
            viewport: ViewportMeta {
                css_width: SMOKE_WIDTH,
                css_height: SMOKE_HEIGHT,
                device_scale_factor: 1.0,
            },
            route: "http://127.0.0.1/".into(),
            revision: Some("macos-gui-smoke".into()),
        };
        let (tx, rx) = mpsc::sync_channel(1);
        capture_wk_webview_for_gui_smoke(&view, request, move |result| {
            let _ = tx.send(result);
        });

        let mut captured = None;
        assert!(
            pump_until(Instant::now() + Duration::from_secs(8), || match rx.try_recv() {
                Ok(result) => {
                    captured = Some(result);
                    true
                }
                Err(mpsc::TryRecvError::Empty) => false,
                Err(mpsc::TryRecvError::Disconnected) => true,
            }),
            "WKWebView snapshot callback timed out"
        );
        let frame = captured
            .expect("snapshot callback disconnected without a result")
            .expect("real WKWebView snapshot must succeed");

        assert_eq!(frame.backend, NativeCaptureBackend::WkWebView);
        assert_eq!(frame.route, "http://127.0.0.1/");
        assert_eq!(frame.revision.as_deref(), Some("macos-gui-smoke"));
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

        window.orderOut(None);
    }
}

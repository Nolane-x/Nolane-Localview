use tauri::webview::PlatformWebview;
use webkit2gtk::{gio, SnapshotOptions, SnapshotRegion, WebView, WebViewExt};

use crate::{
    build_frame, CaptureCompletion, CaptureRequest, NativeCaptureBackend, NativeCaptureError,
};

pub(crate) fn capture(
    webview: PlatformWebview,
    request: CaptureRequest,
    completion: CaptureCompletion,
) {
    capture_view(webview.inner(), request, completion);
}

fn capture_view(view: &WebView, request: CaptureRequest, completion: CaptureCompletion) {
    view.snapshot(
        SnapshotRegion::Visible,
        SnapshotOptions::NONE,
        None::<&gio::Cancellable>,
        move |result| {
            let frame = result
                .map_err(|error| NativeCaptureError::Platform(error.to_string()))
                .and_then(|surface| {
                    let mut png = Vec::new();
                    surface
                        .write_to_png(&mut png)
                        .map_err(|error| NativeCaptureError::Platform(error.to_string()))?;
                    build_frame(png, NativeCaptureBackend::WebKitGtk, request)
                });
            completion(frame);
        },
    );
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
            Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    use gtk::prelude::*;
    use localview_capture::CaptureTarget;
    use localview_visual::decode_png_rgba;
    use webkit2gtk::{LoadEvent, WebView, WebViewExt};

    use super::capture_view;
    use crate::{CaptureRequest, NativeCaptureBackend, ViewportMeta};

    const SMOKE_WIDTH: i32 = 320;
    const SMOKE_HEIGHT: i32 = 180;

    fn pump_until(deadline: Instant, mut done: impl FnMut() -> bool) -> bool {
        let context = gtk::glib::MainContext::default();
        while Instant::now() < deadline {
            while context.pending() {
                context.iteration(false);
            }
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        done()
    }

    #[test]
    #[ignore = "requires a real GUI display; CI runs this explicitly under Xvfb"]
    fn webkitgtk_visible_snapshot_returns_real_rendered_pixels() {
        assert!(
            std::env::var_os("LOCALVIEW_GUI_SMOKE").is_some(),
            "GUI smoke must be explicitly enabled"
        );
        gtk::init().expect("GTK must initialize under the dedicated GUI smoke display");

        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_default_size(SMOKE_WIDTH, SMOKE_HEIGHT);
        let view = WebView::new();
        view.set_size_request(SMOKE_WIDTH, SMOKE_HEIGHT);
        window.add(&view);
        window.show_all();

        let loaded = Arc::new(AtomicBool::new(false));
        let loaded_for_signal = loaded.clone();
        view.connect_load_changed(move |_, event| {
            if event == LoadEvent::Finished {
                loaded_for_signal.store(true, Ordering::SeqCst);
            }
        });
        view.load_html(
            r#"<!doctype html>
<html>
<head><style>
html, body { margin: 0; width: 100%; height: 100%; background: rgb(18, 52, 86); overflow: hidden; }
#proof { position: fixed; left: 25%; top: 25%; width: 50%; height: 50%; background: rgb(220, 40, 60); }
</style></head>
<body><div id="proof"></div></body>
</html>"#,
            Some("http://127.0.0.1/"),
        );

        assert!(
            pump_until(Instant::now() + Duration::from_secs(8), || loaded.load(Ordering::SeqCst)),
            "deterministic WebKitGTK fixture did not finish loading"
        );

        let request = CaptureRequest {
            target: CaptureTarget::Viewport,
            viewport: ViewportMeta {
                css_width: SMOKE_WIDTH as u32,
                css_height: SMOKE_HEIGHT as u32,
                device_scale_factor: 1.0,
            },
            route: "http://127.0.0.1/".into(),
            revision: Some("gui-smoke".into()),
        };
        let (tx, rx) = mpsc::sync_channel(1);
        capture_view(
            &view,
            request,
            Box::new(move |result| {
                let _ = tx.send(result);
            }),
        );

        let deadline = Instant::now() + Duration::from_secs(8);
        let mut captured = None;
        assert!(
            pump_until(deadline, || match rx.try_recv() {
                Ok(result) => {
                    captured = Some(result);
                    true
                }
                Err(mpsc::TryRecvError::Empty) => false,
                Err(mpsc::TryRecvError::Disconnected) => true,
            }),
            "WebKitGTK snapshot callback timed out"
        );
        let frame = captured
            .expect("snapshot callback disconnected without a result")
            .expect("real WebKitGTK visible snapshot must succeed");

        assert_eq!(frame.backend, NativeCaptureBackend::WebKitGtk);
        assert_eq!(frame.route, "http://127.0.0.1/");
        assert_eq!(frame.revision.as_deref(), Some("gui-smoke"));
        assert!(frame.pixel_width > 0 && frame.pixel_height > 0);
        assert!(frame.png.len() > 256, "real rendered PNG must not be a trivial header");

        let decoded = decode_png_rgba(&frame.png).expect("captured PNG must fully decode");
        assert_eq!((decoded.width, decoded.height), (frame.pixel_width, frame.pixel_height));
        let center_x = decoded.width / 2;
        let center_y = decoded.height / 2;
        let offset = ((center_y * decoded.width + center_x) * 4) as usize;
        let center = &decoded.data[offset..offset + 4];
        assert!(
            center[0] >= 190 && center[1] <= 70 && center[2] <= 90 && center[3] >= 240,
            "center pixel must prove the red fixture region was actually rendered; got {center:?}"
        );

        window.close();
    }
}

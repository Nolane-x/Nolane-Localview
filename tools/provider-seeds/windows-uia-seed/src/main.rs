#[cfg(not(windows))]
fn main() {
    eprintln!("localview-windows-uia-seed is a Windows-only UI fixture");
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_seed::run()
}

#[cfg(windows)]
mod windows_seed {
    use std::{
        error::Error,
        io::{self, BufRead, Write},
        sync::mpsc,
        thread,
        time::Duration,
    };

    use localview_windows_uia_seed::{SeedCommand, SeedResponse, SeedState};
    use uuid::Uuid;
    use windows::{
        Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE,
                PeekMessageW, SW_SHOW, SetWindowTextW, ShowWindow, TranslateMessage, WS_CHILD,
                WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
        core::{PCWSTR, w},
    };

    const INITIAL_NAME: &str = "LocalView V4.3 Real Provider Seed";

    pub fn run() -> Result<(), Box<dyn Error>> {
        let window = create_parent_window()?;
        let mut control = create_invoke_control(window, INITIAL_NAME)?;
        let mut state = SeedState::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            raw_handle(window),
            raw_handle(control),
            Uuid::new_v4(),
            INITIAL_NAME.to_owned(),
        );

        emit(&SeedResponse::Ready {
            ground_truth: state.ground_truth(),
        })?;

        let (command_tx, command_rx) = mpsc::channel();
        let reader = thread::Builder::new()
            .name("localview-uia-seed-control".into())
            .spawn(move || read_commands(command_tx))?;

        let mut terminal = false;
        let mut message = MSG::default();
        while !terminal {
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            while let Ok(command) = command_rx.try_recv() {
                match command {
                    SeedCommand::GetGroundTruth => {
                        emit(&SeedResponse::ground_truth(state.ground_truth()))?;
                    }
                    SeedCommand::BurstNameChanges { names } => {
                        if names.is_empty() {
                            emit(&SeedResponse::error(
                                "empty_name_burst",
                                "burst_name_changes requires at least one name",
                            ))?;
                            continue;
                        }

                        let mut ui_error = None;
                        for name in &names {
                            if let Err(error) = set_control_text(control, name) {
                                ui_error = Some(error.to_string());
                                break;
                            }
                            // UI Automation ultimately observes Win32 accessibility notifications.
                            // Give each distinct real mutation a message-pump boundary so Windows does
                            // not collapse the whole burst into one deferred final-state callback. The
                            // observe runtime deliberately does not drain during this command, so its
                            // capacity=1 callback queue must still record overflow rather than relying
                            // on synthetic gap injection.
                            pump_pending_messages();
                            thread::sleep(Duration::from_millis(10));
                        }
                        if let Some(message) = ui_error {
                            emit(&SeedResponse::error("set_window_text_failed", message))?;
                            continue;
                        }

                        match state.burst_name_changes(&names) {
                            Ok(ground_truth) => emit(&SeedResponse::applied(ground_truth))?,
                            Err(error) => emit(&SeedResponse::error(
                                "seed_state_rejected",
                                error.to_string(),
                            ))?,
                        }
                    }
                    SeedCommand::RecreateControl => {
                        let ground_truth = state.ground_truth();
                        unsafe {
                            DestroyWindow(control)?;
                        }
                        control = match if ground_truth.expected_invoke_support {
                            create_invoke_control(window, &ground_truth.logical_name)
                        } else {
                            create_unsupported_invoke_control(window, &ground_truth.logical_name)
                        } {
                            Ok(control) => control,
                            Err(error) => {
                                emit(&SeedResponse::error(
                                    "recreate_control_failed",
                                    error.to_string(),
                                ))?;
                                return Err(error.into());
                            }
                        };

                        match state.record_recreated_control(raw_handle(control), Uuid::new_v4()) {
                            Ok(ground_truth) => emit(&SeedResponse::applied(ground_truth))?,
                            Err(error) => emit(&SeedResponse::error(
                                "seed_state_rejected",
                                error.to_string(),
                            ))?,
                        }
                    }
                    SeedCommand::PresentUnsupportedInvokeControl => {
                        let logical_name = state.ground_truth().logical_name;
                        unsafe {
                            DestroyWindow(control)?;
                        }
                        control = match create_unsupported_invoke_control(window, &logical_name) {
                            Ok(control) => control,
                            Err(error) => {
                                emit(&SeedResponse::error(
                                    "present_unsupported_invoke_control_failed",
                                    error.to_string(),
                                ))?;
                                return Err(error.into());
                            }
                        };

                        match state.record_unsupported_invoke_control(
                            raw_handle(control),
                            Uuid::new_v4(),
                        ) {
                            Ok(ground_truth) => emit(&SeedResponse::applied(ground_truth))?,
                            Err(error) => emit(&SeedResponse::error(
                                "seed_state_rejected",
                                error.to_string(),
                            ))?,
                        }
                    }
                    SeedCommand::Shutdown => {
                        match state.shutdown() {
                            Ok(ground_truth) => emit(&SeedResponse::applied(ground_truth))?,
                            Err(error) => emit(&SeedResponse::error(
                                "seed_state_rejected",
                                error.to_string(),
                            ))?,
                        }
                        terminal = true;
                    }
                }
            }

            thread::sleep(Duration::from_millis(2));
        }

        unsafe {
            DestroyWindow(control)?;
            DestroyWindow(window)?;
        }
        let _ = reader.join();
        Ok(())
    }

    fn read_commands(sender: mpsc::Sender<SeedCommand>) {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<SeedCommand>(&line) {
                Ok(command) => {
                    let terminal = matches!(command, SeedCommand::Shutdown);
                    if sender.send(command).is_err() {
                        break;
                    }
                    if terminal {
                        break;
                    }
                }
                Err(error) => eprintln!("invalid seed control JSON: {error}"),
            }
        }
    }

    fn emit(response: &SeedResponse) -> Result<(), Box<dyn Error>> {
        let stdout = io::stdout();
        let mut stdout = stdout.lock();
        serde_json::to_writer(&mut stdout, response)?;
        writeln!(stdout)?;
        stdout.flush()?;
        Ok(())
    }

    fn create_parent_window() -> windows::core::Result<HWND> {
        let window = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("LocalView V4.3 Real Provider Seed Host"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                520,
                220,
                None,
                None,
                None,
                None,
            )?
        };
        unsafe {
            let _ = ShowWindow(window, SW_SHOW);
        }
        Ok(window)
    }

    fn create_invoke_control(parent: HWND, logical_name: &str) -> windows::core::Result<HWND> {
        create_child_control(parent, logical_name, w!("BUTTON"))
    }

    fn create_unsupported_invoke_control(
        parent: HWND,
        logical_name: &str,
    ) -> windows::core::Result<HWND> {
        create_child_control(parent, logical_name, w!("STATIC"))
    }

    fn create_child_control(
        parent: HWND,
        logical_name: &str,
        class_name: PCWSTR,
    ) -> windows::core::Result<HWND> {
        let wide_name = wide(logical_name);
        let control = unsafe {
            CreateWindowExW(
                Default::default(),
                class_name,
                PCWSTR(wide_name.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                40,
                60,
                420,
                80,
                Some(parent),
                None,
                None,
                None,
            )?
        };
        unsafe {
            let _ = ShowWindow(control, SW_SHOW);
        }
        Ok(control)
    }

    fn set_control_text(control: HWND, logical_name: &str) -> windows::core::Result<()> {
        let wide_name = wide(logical_name);
        unsafe { SetWindowTextW(control, PCWSTR(wide_name.as_ptr())) }
    }

    fn pump_pending_messages() {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn raw_handle(handle: HWND) -> u64 {
        handle.0 as usize as u64
    }
}

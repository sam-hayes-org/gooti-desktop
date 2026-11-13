//! Locks the vault after idle time, when the screen locks, and before the computer sleeps.
//! Locking forgets every private key held in memory.

use std::time::Duration;

use tauri::AppHandle;

use crate::state::AppState;

const IDLE_CHECK: Duration = Duration::from_secs(15);

pub fn start(app: &AppHandle, state: &AppState) {
    start_idle_timer(app.clone(), state.clone());
    #[cfg(target_os = "linux")]
    linux::start(app.clone(), state.clone());
    #[cfg(windows)]
    windows::start(app.clone(), state.clone());
}

/// "Lock after N minutes": without use in the window for that long.
fn start_idle_timer(app: AppHandle, state: AppState) {
    std::thread::Builder::new()
        .name("gooti-idle-lock".into())
        .spawn(move || {
            loop {
                std::thread::sleep(IDLE_CHECK);
                let limit = {
                    let gooti = state.gooti();
                    if !gooti.vault().is_unlocked() {
                        continue;
                    }
                    gooti.settings().auto_lock_minutes
                };
                if let Some(minutes) = limit
                    && state.idle_for() >= Duration::from_secs(u64::from(minutes) * 60)
                {
                    state.lock(&app);
                }
            }
        })
        .expect("could not start the idle lock");
}

/// Screen lock and sleep come from systemd-logind over D-Bus.
///
/// A screen locker must tell logind it locked. `loginctl lock-session` does, and so do
/// lockers that set the session's `LockedHint`. A locker that does neither is not seen.
#[cfg(target_os = "linux")]
mod linux {
    use futures_util::StreamExt;
    use tauri::AppHandle;
    use zbus::zvariant::OwnedObjectPath;

    use crate::state::AppState;

    #[zbus::proxy(
        interface = "org.freedesktop.login1.Manager",
        default_service = "org.freedesktop.login1",
        default_path = "/org/freedesktop/login1"
    )]
    trait Manager {
        #[zbus(name = "GetSessionByPID")]
        fn get_session_by_pid(&self, pid: u32) -> zbus::Result<OwnedObjectPath>;
        fn get_session(&self, session_id: &str) -> zbus::Result<OwnedObjectPath>;
        #[zbus(signal)]
        fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
    }

    #[zbus::proxy(
        interface = "org.freedesktop.login1.User",
        default_service = "org.freedesktop.login1",
        default_path = "/org/freedesktop/login1/user/self"
    )]
    trait User {
        /// The user's graphical session.
        #[zbus(property)]
        fn display(&self) -> zbus::Result<(String, OwnedObjectPath)>;
    }

    #[zbus::proxy(
        interface = "org.freedesktop.login1.Session",
        default_service = "org.freedesktop.login1"
    )]
    trait Session {
        #[zbus(signal)]
        fn lock(&self) -> zbus::Result<()>;
        #[zbus(property)]
        fn locked_hint(&self) -> zbus::Result<bool>;
    }

    pub fn start(app: AppHandle, state: AppState) {
        tauri::async_runtime::spawn(async move {
            if let Err(error) = watch(app, state).await {
                eprintln!("Gooti: cannot watch screen lock and sleep: {error}");
            }
        });
    }

    async fn watch(app: AppHandle, state: AppState) -> zbus::Result<()> {
        let connection = zbus::Connection::system().await?;
        let manager = ManagerProxy::new(&connection).await?;
        let mut sleep = manager.receive_prepare_for_sleep().await?;

        let session = match session_path(&connection, &manager).await {
            Some(path) => Some(
                SessionProxy::builder(&connection)
                    .path(path)?
                    .build()
                    .await?,
            ),
            None => {
                eprintln!("Gooti: no login session found, screen lock is not watched.");
                None
            }
        };
        let (mut lock, mut hint) = match &session {
            Some(session) => (
                Some(session.receive_lock().await?),
                Some(session.receive_locked_hint_changed().await),
            ),
            None => (None, None),
        };

        loop {
            next_event(&app, &state, &mut sleep, lock.as_mut(), hint.as_mut()).await;
        }
    }

    /// Waits for the next event and locks when the matching setting is on.
    async fn next_event(
        app: &AppHandle,
        state: &AppState,
        sleep: &mut PrepareForSleepStream,
        lock: Option<&mut LockStream>,
        hint: Option<&mut zbus::proxy::PropertyStream<'static, bool>>,
    ) {
        let lock_next = async {
            match lock {
                Some(stream) => stream.next().await.map(|_| ()),
                None => std::future::pending().await,
            }
        };
        let hint_next = async {
            match hint {
                Some(stream) => match stream.next().await {
                    Some(change) => Some(change.get().await.unwrap_or(false)),
                    None => None,
                },
                None => std::future::pending().await,
            }
        };
        futures_util::pin_mut!(lock_next, hint_next);
        let sleep_next = sleep.next();
        futures_util::pin_mut!(sleep_next);

        let event = futures_util::future::select(
            sleep_next,
            futures_util::future::select(lock_next, hint_next),
        )
        .await;

        let settings = state.gooti().settings().clone();
        match event {
            futures_util::future::Either::Left((Some(signal), _)) => {
                let starting = signal.args().map(|args| args.start).unwrap_or(false);
                if starting && settings.lock_on_sleep {
                    state.lock(app);
                }
            }
            futures_util::future::Either::Right((inner, _)) => {
                let screen_locked = match inner {
                    futures_util::future::Either::Left((Some(()), _)) => true,
                    futures_util::future::Either::Right((Some(locked), _)) => locked,
                    _ => false,
                };
                if screen_locked && settings.lock_on_screen_lock {
                    state.lock(app);
                }
            }
            futures_util::future::Either::Left((None, _)) => {
                // The sleep stream ended: the bus went away. Avoid a busy loop.
                std::future::pending::<()>().await;
            }
        }
    }

    /// The login session to watch. An app started by the desktop often runs outside any session
    /// (in a systemd user scope), so try, in order: this process, `XDG_SESSION_ID`, and the
    /// user's graphical session.
    async fn session_path(
        connection: &zbus::Connection,
        manager: &ManagerProxy<'_>,
    ) -> Option<OwnedObjectPath> {
        if let Ok(path) = manager.get_session_by_pid(std::process::id()).await {
            return Some(path);
        }
        if let Ok(id) = std::env::var("XDG_SESSION_ID")
            && let Ok(path) = manager.get_session(&id).await
        {
            return Some(path);
        }
        let user = UserProxy::new(connection).await.ok()?;
        let (id, path) = user.display().await.ok()?;
        (!id.is_empty()).then_some(path)
    }
}

/// Screen lock and sleep come as window messages: `WM_WTSSESSION_CHANGE` (after
/// `WTSRegisterSessionNotification`) and `WM_POWERBROADCAST`. Broadcasts reach only top-level
/// windows, so this is a hidden top-level window (a message-only window would miss sleep), with
/// its own thread and message loop.
///
/// Computers with Modern Standby may not send `PBT_APMSUSPEND`. There the screen lock that comes
/// with the screen turning off locks Gooti instead.
#[cfg(windows)]
mod windows {
    use std::sync::OnceLock;

    use tauri::AppHandle;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PBT_APMSUSPEND,
        RegisterClassW, TranslateMessage, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW,
        WTS_SESSION_LOCK,
    };

    use crate::state::AppState;

    /// The window procedure has no other way to reach the app.
    static TARGET: OnceLock<(AppHandle, AppState)> = OnceLock::new();

    pub fn start(app: AppHandle, state: AppState) {
        if TARGET.set((app, state)).is_err() {
            return;
        }
        let started = std::thread::Builder::new()
            .name("gooti-session-watch".into())
            .spawn(|| {
                if let Err(error) = run() {
                    eprintln!("Gooti: cannot watch screen lock and sleep: {error}");
                }
            });
        if let Err(error) = started {
            eprintln!("Gooti: cannot watch screen lock and sleep: {error}");
        }
    }

    fn run() -> Result<(), &'static str> {
        let class: Vec<u16> = "GootiSessionWatch\0".encode_utf16().collect();
        // SAFETY: plain Win32 calls on this thread. `class` outlives the window, which lives as
        // long as the thread, and the structs passed are fully initialized.
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let window_class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..Default::default()
            };
            if RegisterClassW(&window_class) == 0 {
                return Err("the window class was not registered");
            }
            // Never shown: no WS_VISIBLE, and nothing calls ShowWindow.
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if window.is_null() {
                return Err("the window was not created");
            }
            if WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) == 0 {
                eprintln!("Gooti: screen lock is not watched, only sleep.");
            }
            let mut message = MSG::default();
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        Ok(())
    }

    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let event = wparam as u32;
        if message == WM_WTSSESSION_CHANGE && event == WTS_SESSION_LOCK {
            lock_if(|settings| settings.lock_on_screen_lock);
        } else if message == WM_POWERBROADCAST && event == PBT_APMSUSPEND {
            lock_if(|settings| settings.lock_on_sleep);
        }
        // SAFETY: passes the message on unchanged, as every window procedure must.
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    fn lock_if(wanted: impl Fn(&gooti_core::Settings) -> bool) {
        if let Some((app, state)) = TARGET.get() {
            let on = wanted(state.gooti().settings());
            if on {
                state.lock(app);
            }
        }
    }
}

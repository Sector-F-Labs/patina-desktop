//! The lock screen (ADR-0025). Any ext-session-lock locker (patina-lock by
//! default, from the `[lock]` config) locks the session: from then on only
//! its surface is drawn and gets input, and the desktop stays hidden until
//! it unlocks, even if it crashes (then a new locker can take over).
//! Ctrl+Super+Q and Super+Escape lock (ADR-0026), and, on the hardware, so
//! does going to sleep: a
//! logind delay inhibitor holds suspend back until the lock is on screen.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use smithay::{
    backend::renderer::{
        ImportAll, Renderer, Texture,
        element::{
            Kind,
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
    },
    delegate_session_lock,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    utils::SERIAL_COUNTER,
    wayland::session_lock::{
        LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
    },
};

use crate::state::State;

/// Whether the session is locked, and the locker's surface once it has one.
#[derive(Default)]
pub enum Lock {
    #[default]
    Unlocked,
    Locked(Option<LockSurface>),
}

impl State {
    pub fn locked(&self) -> bool {
        matches!(self.lock, Lock::Locked(_))
    }

    /// The locker's surface, while locked.
    pub fn lock_surface(&self) -> Option<&WlSurface> {
        match &self.lock {
            Lock::Locked(Some(surface)) => Some(surface.wl_surface()),
            _ => None,
        }
    }

    /// Runs the `[lock]` command, unless the session is already locked.
    pub fn lock_screen(&mut self) {
        let command = self.config.lock.command.clone();
        if !self.locked() && !command.is_empty() {
            self.spawn_client(&command);
        }
    }

    /// After a frame is drawn: while locked, the lock is now on screen.
    pub fn lock_drawn(&self) {
        if self.locked() {
            self.lock_shown.store(true, Ordering::SeqCst);
        }
    }

    /// Gives the keyboard to the locker's surface.
    fn focus_lock(&mut self) {
        let surface = self.lock_surface().cloned();
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface.map(Into::into), SERIAL_COUNTER.next_serial());
        }
    }
}

impl SessionLockHandler for State {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        // Hide the desktop at once: nothing but black (then the locker's
        // surface) is drawn from the next frame.
        self.lock = Lock::Locked(None);
        crate::session::mark_locked(true);
        self.lock_shown.store(false, Ordering::SeqCst);
        self.activate(None);
        self.focus_lock();
        if let Some(pointer) = self.seat.get_pointer() {
            let location = pointer.current_location();
            self.pointer_motion(location, 0);
        }
        self.needs_redraw = true;
        confirmation.lock();
    }

    fn unlock(&mut self) {
        self.lock = Lock::Unlocked;
        crate::session::mark_locked(false);
        self.lock_shown.store(false, Ordering::SeqCst);
        self.focus_top_window();
        self.needs_redraw = true;
    }

    fn new_surface(&mut self, surface: LockSurface, _: WlOutput) {
        let size = self
            .space
            .output_geometry(&self.output)
            .map(|g| g.size)
            .unwrap_or_default();
        surface.with_pending_state(|s| {
            s.size = Some((size.w.max(1) as u32, size.h.max(1) as u32).into());
        });
        surface.send_configure();
        self.lock = Lock::Locked(Some(surface));
        self.focus_lock();
        self.needs_redraw = true;
    }
}

delegate_session_lock!(State);

/// What to draw while locked: the locker's `surface`, or nothing (black)
/// until it has one.
pub fn elements<R>(
    renderer: &mut R,
    surface: Option<&WlSurface>,
    scale: f64,
) -> Vec<WaylandSurfaceRenderElement<R>>
where
    R: Renderer + ImportAll,
    R::TextureId: Clone + Texture + 'static,
{
    match surface {
        Some(surface) => render_elements_from_surface_tree(
            renderer,
            surface,
            (0, 0),
            scale,
            1.0,
            Kind::Unspecified,
        ),
        None => Vec::new(),
    }
}

/// Frame callbacks for the locker, so it can draw (it isn't in the space).
pub fn send_frames(surface: Option<&WlSurface>, output: &smithay::output::Output, time: Duration) {
    if let Some(surface) = surface {
        smithay::desktop::utils::send_frames_surface_tree(
            surface,
            output,
            time,
            Some(Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    }
}

/// Locks before sleep, on the hardware: holds a logind "delay" inhibitor,
/// and when logind says the machine is about to sleep, asks the compositor
/// to lock (through `lock_now`) and waits for the lock to be on screen (or
/// 3 s) before letting go, so the desktop is never shown on wake. Taken
/// again after waking.
pub fn watch_sleep(
    lock_now: smithay::reexports::calloop::channel::Sender<()>,
    shown: Arc<AtomicBool>,
) {
    let _ = std::thread::Builder::new()
        .name("sleep".into())
        .spawn(move || {
            if let Err(e) = sleep_loop(&lock_now, &shown) {
                eprintln!("patina-compositor: not locking before sleep: {e}");
            }
        });
}

fn sleep_loop(
    lock_now: &smithay::reexports::calloop::channel::Sender<()>,
    shown: &AtomicBool,
) -> Result<(), String> {
    use zbus::blocking::{Connection, Proxy};
    let bus = Connection::system().map_err(|e| e.to_string())?;
    let manager = Proxy::new(
        &bus,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(|e| e.to_string())?;
    let inhibit = || -> Option<zbus::zvariant::OwnedFd> {
        manager
            .call(
                "Inhibit",
                &(
                    "sleep",
                    "Patina",
                    "Lock the screen before sleeping",
                    "delay",
                ),
            )
            .map_err(|e| eprintln!("patina-compositor: logind Inhibit: {e}"))
            .ok()
    };
    let mut held = inhibit();
    let signals = manager
        .receive_signal("PrepareForSleep")
        .map_err(|e| e.to_string())?;
    for message in signals {
        let Ok(going) = message.body().deserialize::<bool>() else {
            continue;
        };
        if going {
            if lock_now.send(()).is_err() {
                return Ok(());
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while !shown.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            // Letting go of the inhibitor lets the machine sleep.
            held = None;
        } else if held.is_none() {
            held = inhibit();
        }
    }
    Ok(())
}

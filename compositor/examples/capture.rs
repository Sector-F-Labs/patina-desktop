//! Captures the screen, or a window, through ext-image-copy-capture-v1 as
//! xdg-desktop-portal-wlr does (ADR-0092), and saves it as a PNG: a check
//! that the compositor serves it, with no portal or OBS.
//!
//! ```sh
//! cargo run -p patina-compositor --example capture -- shot.png
//! cargo run -p patina-compositor --example capture -- shot.png --window firefox
//! cargo run -p patina-compositor --example capture -- shot.png --frames 60
//! ```
//!
//! `--window` takes the first window whose app id contains the text, from
//! ext-foreign-toplevel-list; `--frames N` asks for N frames in a row, as a
//! recording would, and says how long they took (each waits for something
//! on screen to change, so move the pointer or play a video meanwhile).

use std::os::fd::AsFd;
use std::time::Instant;

use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer, wl_output::WlOutput, wl_registry::WlRegistry, wl_shm, wl_shm::WlShm,
        wl_shm_pool::WlShmPool,
    },
};
use smithay_client_toolkit::reexports::protocols::ext::{
    foreign_toplevel_list::v1::client::{
        ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
        ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
    },
    image_capture_source::v1::client::{
        ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
        ext_image_capture_source_v1::ExtImageCaptureSourceV1,
        ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
    },
    image_copy_capture::v1::client::{
        ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
        ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
        ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
    },
};

#[derive(Default)]
struct App {
    /// Windows by their handle, with their app ids once sent.
    windows: Vec<(ExtForeignToplevelHandleV1, String)>,
    size: Option<(u32, u32)>,
    formats: Vec<wl_shm::Format>,
    constraints_done: bool,
    stopped: bool,
    /// The frame's outcome: Ok once ready, Err with the reason if it failed.
    outcome: Option<Result<(), String>>,
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: capture OUT.png [--window APP] [--frames N]")?;
    let (mut window, mut frames) = (None, 1usize);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--window" => window = args.next(),
            "--frames" => frames = args.next().and_then(|n| n.parse().ok()).unwrap_or(1),
            other => return Err(format!("unknown argument {other}")),
        }
    }

    let connection = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let (globals, mut queue) =
        registry_queue_init::<App>(&connection).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let mut app = App::default();
    fn bind_err(
        name: &'static str,
    ) -> impl Fn(smithay_client_toolkit::reexports::client::globals::BindError) -> String {
        move |e| format!("no {name}: {e}")
    }
    let shm: WlShm = globals.bind(&qh, 1..=1, ()).map_err(bind_err("wl_shm"))?;
    let copy: ExtImageCopyCaptureManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(bind_err("ext_image_copy_capture_manager_v1"))?;

    let source: ExtImageCaptureSourceV1 = match &window {
        None => {
            let output: WlOutput = globals
                .bind(&qh, 1..=4, ())
                .map_err(bind_err("wl_output"))?;
            let manager: ExtOutputImageCaptureSourceManagerV1 = globals
                .bind(&qh, 1..=1, ())
                .map_err(bind_err("ext_output_image_capture_source_manager_v1"))?;
            manager.create_source(&output, &qh, ())
        }
        Some(wanted) => {
            let _list: ExtForeignToplevelListV1 = globals
                .bind(&qh, 1..=1, ())
                .map_err(bind_err("ext_foreign_toplevel_list_v1"))?;
            queue.roundtrip(&mut app).map_err(|e| e.to_string())?;
            queue.roundtrip(&mut app).map_err(|e| e.to_string())?;
            for (_, app_id) in &app.windows {
                eprintln!("window: {app_id}");
            }
            let handle = app
                .windows
                .iter()
                .find(|(_, app_id)| app_id.contains(wanted.as_str()))
                .map(|(h, _)| h.clone())
                .ok_or(format!("no window with {wanted:?} in its app id"))?;
            let manager: ExtForeignToplevelImageCaptureSourceManagerV1 =
                globals.bind(&qh, 1..=1, ()).map_err(bind_err(
                    "ext_foreign_toplevel_image_capture_source_manager_v1",
                ))?;
            manager.create_source(&handle, &qh, ())
        }
    };

    let session = copy.create_session(&source, Options::PaintCursors, &qh, ());
    while !app.constraints_done && !app.stopped {
        queue
            .blocking_dispatch(&mut app)
            .map_err(|e| e.to_string())?;
    }
    if app.stopped {
        return Err("the session was stopped".into());
    }
    let (w, h) = app.size.ok_or("no buffer size")?;
    let format = [wl_shm::Format::Xrgb8888, wl_shm::Format::Argb8888]
        .into_iter()
        .find(|f| app.formats.contains(f))
        .ok_or(format!("no format to use in {:?}", app.formats))?;
    eprintln!("{w}x{h}, {format:?}");

    let stride = w * 4;
    let len = (stride * h) as usize;
    let file = tempfile_of(len)?;
    let pool = shm.create_pool(file.as_fd(), len as i32, &qh, ());
    let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, format, &qh, ());

    let started = Instant::now();
    for n in 0..frames {
        app.outcome = None;
        let frame = session.create_frame(&qh, ());
        frame.attach_buffer(&buffer);
        frame.damage_buffer(0, 0, w as i32, h as i32);
        frame.capture();
        while app.outcome.is_none() {
            queue
                .blocking_dispatch(&mut app)
                .map_err(|e| e.to_string())?;
        }
        frame.destroy();
        if let Some(Err(e)) = &app.outcome {
            return Err(format!("frame {n}: {e}"));
        }
    }
    let took = started.elapsed();
    if frames > 1 {
        eprintln!(
            "{frames} frames in {:.2}s, {:.1} a second",
            took.as_secs_f64(),
            frames as f64 / took.as_secs_f64()
        );
    }

    let mut bgra = vec![0u8; len];
    {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = &file;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        file.read_exact(&mut bgra).map_err(|e| e.to_string())?;
    }
    let rgba: Vec<u8> = bgra
        .chunks_exact(4)
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let out = std::fs::File::create(&path).map_err(|e| format!("{path}: {e}"))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(out), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    eprintln!("saved {path}");
    buffer.destroy();
    pool.destroy();
    session.destroy();
    Ok(())
}

/// An unlinked file of `len` bytes to share as the buffer.
fn tempfile_of(len: usize) -> Result<std::fs::File, String> {
    let path = std::env::temp_dir().join(format!("patina-capture-{}", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let _ = std::fs::remove_file(&path);
    file.set_len(len as u64).map_err(|e| e.to_string())?;
    Ok(file)
}

impl Dispatch<WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as smithay_client_toolkit::reexports::client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            app.windows.push((toplevel, String::new()));
        }
    }

    smithay_client_toolkit::reexports::client::event_created_child!(App, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for App {
    fn event(
        app: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_handle_v1::Event::AppId { app_id } = event
            && let Some(entry) = app.windows.iter_mut().find(|(h, _)| h == handle)
        {
            entry.1 = app_id;
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        match event {
            Event::BufferSize { width, height } => app.size = Some((width, height)),
            Event::ShmFormat {
                format: smithay_client_toolkit::reexports::client::WEnum::Value(format),
            } => app.formats.push(format),
            Event::Done => app.constraints_done = true,
            Event::Stopped => app.stopped = true,
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, ()> for App {
    fn event(
        app: &mut Self,
        _: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        match event {
            Event::Ready => app.outcome = Some(Ok(())),
            Event::Failed { reason } => app.outcome = Some(Err(format!("failed: {reason:?}"))),
            _ => {}
        }
    }
}

delegate_noop!(App: ignore WlShm);
delegate_noop!(App: ignore WlOutput);
delegate_noop!(App: WlShmPool);
delegate_noop!(App: ignore WlBuffer);
delegate_noop!(App: ExtImageCopyCaptureManagerV1);
delegate_noop!(App: ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(App: ExtForeignToplevelImageCaptureSourceManagerV1);
delegate_noop!(App: ExtImageCaptureSourceV1);

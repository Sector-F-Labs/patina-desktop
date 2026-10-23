//! Calendar: a Patina calendar app for a CalDAV server, made for `plan` on
//! urd. Day, week and month views, calendars in colour that can be shown
//! and hidden, and events that can be added, changed and deleted.

mod agenda;
mod app;
mod caldav;
mod config;
mod edit;
mod ics;
mod model;
mod sync;
mod views;

/// The window icon, rendered from `packaging/linux/calendar.svg` by `make
/// icons`. On Wayland, GNOME shows the installed launcher's icon instead.
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../assets/calendar-icon-128.png");

fn window_icon() -> egui::IconData {
    let icon = image::load_from_memory_with_format(WINDOW_ICON_PNG, image::ImageFormat::Png)
        .expect("embedded window icon should decode as PNG")
        .into_rgba8();
    let (width, height) = icon.dimensions();
    egui::IconData { rgba: icon.into_raw(), width, height }
}

const USAGE: &str = "usage: calendar [--open DATE [--event UID]]
       calendar --agenda [--from DATE] [--days N]

With --open, opens at DATE (YYYY-MM-DD), and with --event, that event's
occurrence on it. --agenda prints the events on N days (1 by default) from
DATE (today by default) as JSON, from what the app last synced, without
opening a window.";

/// What the command line asked for.
#[derive(Debug, Default, PartialEq)]
struct Args {
    agenda: bool,
    from: Option<chrono::NaiveDate>,
    days: Option<u32>,
    opening: app::Opening,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let date = |v: Option<String>| -> Result<chrono::NaiveDate, String> {
        let v = v.ok_or("a date is missing")?;
        chrono::NaiveDate::parse_from_str(&v, "%Y-%m-%d").map_err(|_| format!("not a date: {v}"))
    };
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--agenda" => parsed.agenda = true,
            "--from" => parsed.from = Some(date(args.next())?),
            "--days" => {
                let v = args.next().ok_or("a number of days is missing")?;
                parsed.days = Some(v.parse().map_err(|_| format!("not a number of days: {v}"))?);
            }
            "--open" => parsed.opening.day = Some(date(args.next())?),
            "--event" => parsed.opening.event = Some(args.next().ok_or("an event's UID is missing")?),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if parsed.opening.event.is_some() && parsed.opening.day.is_none() {
        return Err("--event needs --open DATE".into());
    }
    Ok(parsed)
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return std::process::ExitCode::SUCCESS;
    }
    let args = match parse(args) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("calendar: {e}\n{USAGE}");
            return std::process::ExitCode::from(2);
        }
    };
    if args.agenda {
        let local = app::local_zone();
        let today = chrono::Utc::now().with_timezone(&local).date_naive();
        let items = agenda::agenda(
            &sync::load_cache(),
            &config::Config::load().hidden,
            local,
            args.from.unwrap_or(today),
            args.days.unwrap_or(1),
        );
        println!("{}", serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into()));
        return std::process::ExitCode::SUCCESS;
    }
    match window(args.opening) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("calendar: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn window(opening: app::Opening) -> eframe::Result {
    let config = config::Config::load();
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Calendar")
                .with_app_id("com.sectorflabs.Calendar")
                .with_icon(window_icon())
                .with_inner_size([1200.0, 800.0])
                .with_min_inner_size([720.0, 480.0]),
            !config.system_title_bar,
        ),
        ..Default::default()
    };
    eframe::run_native("calendar", options, Box::new(|cc| Ok(Box::new(app::App::new(cc, opening)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Result<Args, String> {
        parse(line.split_whitespace().map(String::from))
    }

    #[test]
    fn the_command_line() {
        let a = args("--agenda --from 2026-10-23 --days 7").unwrap();
        assert!(a.agenda);
        assert_eq!(a.from, chrono::NaiveDate::from_ymd_opt(2026, 10, 23));
        assert_eq!(a.days, Some(7));
        let a = args("--open 2026-10-24 --event gym").unwrap();
        assert_eq!(a.opening.day, chrono::NaiveDate::from_ymd_opt(2026, 10, 24));
        assert_eq!(a.opening.event.as_deref(), Some("gym"));
        assert_eq!(args("").unwrap(), Args::default());
        assert!(args("--event gym").is_err());
        assert!(args("--open tomorrow").is_err());
        assert!(args("--days").is_err());
        assert!(args("--frobnicate").is_err());
    }

    #[test]
    fn window_icon_decodes_at_its_size() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (128, 128));
    }
}

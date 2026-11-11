//! `patina-webapp`: launches the web apps Settings installs (ADR-0133), and lists, adds and
//! removes them from a shell.

use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

use patina_webapp::{Engine, Store, find_engine, find_webview};

const USAGE: &str = "usage: patina-webapp launch SLUG | list | add URL [NAME] | refresh SLUG | engine SLUG webkit|chromium | remove SLUG | sync";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-webapp: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let store = Store::user().map_err(|e| e.to_string())?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args[..] {
        ["launch", slug] => {
            let app = store
                .find(slug)
                .ok_or(format!("no web app called {slug}"))?;
            // The engine takes this process's place, so the window is the app's.
            let mut command = match app.engine {
                Engine::WebKit => match find_webview() {
                    Some(webview) => {
                        let mut c = Command::new(webview);
                        c.arg(slug);
                        c
                    }
                    None => return Err("patina-webview isn't installed".into()),
                },
                Engine::Chromium => {
                    let engine = find_engine()
                        .ok_or("no Chromium found: install it, or set PATINA_WEBAPP_ENGINE")?;
                    std::fs::create_dir_all(store.profile_dir_for(slug, &engine))
                        .map_err(|e| e.to_string())?;
                    let cmd = store.command(&app, &engine);
                    let mut c = Command::new(&cmd[0]);
                    c.args(&cmd[1..]);
                    c
                }
            };
            let program = command.get_program().to_owned();
            Err(format!("couldn't start {program:?}: {}", command.exec()))
        }
        ["list"] => {
            for a in store.list() {
                println!("{}\t{}\t{}", a.slug, a.name, a.url);
            }
            Ok(())
        }
        ["add", url] | ["add", url, _] => {
            let name = args.get(2).copied().unwrap_or("");
            let app = store.add(name, url)?;
            println!("{}", app.slug);
            if let Err(e) = store.refresh(&app.slug) {
                eprintln!("patina-webapp: no icon or colour: {e}");
            }
            Ok(())
        }
        ["refresh", slug] => {
            let app = store.refresh(slug)?;
            println!(
                "{}\t{}",
                app.color.unwrap_or_default(),
                app.icon.unwrap_or_default()
            );
            Ok(())
        }
        ["engine", slug, name] => {
            let engine = Engine::from_name(name).ok_or("the engines are webkit and chromium")?;
            store.set_engine(slug, engine)
        }
        ["remove", slug] => store.remove(slug),
        ["sync"] => store.write_entries().map_err(|e| e.to_string()),
        _ => Err(USAGE.into()),
    }
}

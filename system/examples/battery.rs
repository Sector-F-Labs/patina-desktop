//! Prints the battery, its health and the last day of its history, as the
//! bar and Settings' Power and Battery page read them:
//! `cargo run -p patina-system --example battery`.

fn main() {
    println!("battery: {:?}", patina_system::power::read());
    println!("health: {:?}", patina_system::power::health().map(|h| h.describe()));
    match patina_system::battery_history::read(24 * 60 * 60, 240) {
        Ok(Some(history)) => {
            for (name, samples) in [("charge", &history.charge), ("rate", &history.rate)] {
                println!("{name}: {} samples", samples.len());
                for s in samples.iter().rev().take(5) {
                    println!("  {} {:.1} {:?}", s.time, s.value, s.state);
                }
            }
        }
        Ok(None) => println!("history: no UPower, or no battery"),
        Err(e) => println!("history: {e}"),
    }
}

//! Prints the media players on the session bus and the one the bar would
//! show: `cargo run -p patina-system --example media`.

use patina_system::mpris;

fn main() {
    let bus = mpris::connect().expect("session bus");
    let players = mpris::players(&bus).expect("players");
    for p in &players {
        println!("{p:?}");
    }
    println!("shown: {:?}", mpris::pick(players));
}

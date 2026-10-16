//! Prints the power modes power-profiles-daemon offers, or with an argument
//! switches to that one: `cargo run -p patina-system --example power -- balanced`.

fn main() {
    if let Some(profile) = std::env::args().nth(1) {
        match patina_system::power_profiles::set(&profile) {
            Ok(()) => println!("now {profile}"),
            Err(e) => eprintln!("{e}"),
        }
    }
    println!("{:?}", patina_system::power_profiles::status());
}

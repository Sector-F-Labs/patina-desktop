//! The prompter's side of the secret exchange, for checking against gcr's own
//! (`keyring/tests/gcr-interop.py`): reads the daemon's first message on stdin,
//! prints the reply with the password given as the first argument.

#[path = "../src/exchange.rs"]
#[allow(dead_code)]
mod exchange;

use std::io::Read;

fn main() {
    let password = std::env::args().nth(1).unwrap_or_default();
    let mut message = String::new();
    std::io::stdin().read_to_string(&mut message).unwrap();
    let mut x = exchange::Exchange::new();
    x.receive(&message).expect("the daemon's message");
    print!("{}", x.send(Some(password.as_bytes())).unwrap());
}

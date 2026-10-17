//! Inert selective forecast capture and benchmark entry point.

#[path = "selective_support/mod.rs"]
mod selective_boundary;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().nth(1).as_deref() == Some("--selective-benchmark") {
        selective_boundary::benchmark()
    } else {
        selective_boundary::run()
    }
}

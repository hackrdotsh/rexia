//! Print the `client.keymap` request rexia sends: `cargo run --example keymap_request`.
fn main() {
    println!(
        "{}",
        rexia::ui::actions::keymap_request(&Default::default())
    );
}

fn main() {
    // ssh runs rexia to ask its questions; that rexia only passes them on.
    if let Some(status) = rexia::rex::askpass::helper() {
        std::process::exit(status);
    }
    if let Err(error) = rexia::app::run() {
        eprintln!("rexia: {error:#}");
        std::process::exit(1);
    }
}

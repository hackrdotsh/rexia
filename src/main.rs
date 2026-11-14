fn main() {
    if let Err(error) = rexia::app::run() {
        eprintln!("rexia: {error:#}");
        std::process::exit(1);
    }
}

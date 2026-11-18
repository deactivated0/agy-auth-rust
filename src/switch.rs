fn main() {
    if let Err(err) = agybuff_auth::app::run_switcher() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

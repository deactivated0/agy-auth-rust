fn main() {
    if let Err(err) = agybuff_auth::app::run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

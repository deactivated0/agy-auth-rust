fn main() {
    if let Err(err) = agy_auth_rust::app::run_switcher() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

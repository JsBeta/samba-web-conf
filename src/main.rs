fn main() {
    if let Err(message) = samba_ui::run(std::env::args().skip(1)) {
        eprintln!("samba-ui: {message}");
        std::process::exit(2);
    }
}

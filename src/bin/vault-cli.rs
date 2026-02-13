fn main() {
    if let Err(error) = vault_nova::cli::run() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}

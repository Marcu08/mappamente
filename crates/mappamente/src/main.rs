fn main() {
    if let Err(error) = mappamente::cli::run() {
        eprintln!("mappamente: {error:#}");
        std::process::exit(1);
    }
}

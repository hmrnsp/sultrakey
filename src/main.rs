fn main() {
    let code = match sultrakey::app::run() {
        Ok(code) => code,
        Err(err) => sultrakey::output::report(&err),
    };
    std::process::exit(code);
}

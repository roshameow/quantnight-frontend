fn main() {
    std::process::exit(quantnight_taskctl::run_os(std::env::args_os().skip(1)));
}

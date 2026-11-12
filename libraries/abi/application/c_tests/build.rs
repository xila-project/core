fn main() {
    cc::Build::new()
        .file("src/round_trip.c")
        .include(std::env::var("DEP_XILA_ABI_APPLICATION_INCLUDE").unwrap())
        .flag("-std=c11")
        .flag("-Wall")
        .flag("-Wextra")
        .flag("-Werror")
        .flag("-pedantic")
        .compile("application_c_test");
    println!("cargo:rerun-if-changed=src/round_trip.c");
}

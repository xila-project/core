fn main() {
    let mut build = cc::Build::new();
    if std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "xtensa") {
        build.flag("-mlongcalls");
    }
    build
        .file("src/clock_random.c")
        .file("src/process.c")
        .file("src/threads.c")
        .file("src/round_trip.c")
        .file("src/startup.c")
        .file("src/sdk.c")
        .file("../sdk/stubs.c")
        .file("../sdk/crt0.c")
        .include("../sdk")
        .include(std::env::var("DEP_XILA_ABI_APPLICATION_INCLUDE").unwrap())
        .flag("-std=c11")
        .flag("-Wall")
        .flag("-Wextra")
        .flag("-Werror")
        .flag("-pedantic")
        .compile("application_c_test");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=../sdk");
}

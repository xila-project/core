fn main() {
    #[cfg(feature = "iana-data")]
    {
        let version = jiff_tzdb::VERSION.expect("pinned timezone database must have a version");
        println!("cargo:rustc-env=XILA_TZDB_VERSION={version}");
    }
}

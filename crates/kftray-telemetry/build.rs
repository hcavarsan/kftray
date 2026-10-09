fn main() {
    // The target triple tells gnu from musl and the arch apart on reports.
    // Only build scripts see `TARGET`, so it is re-exported for `lib.rs`.
    let target = std::env::var("TARGET").expect("cargo sets TARGET for build scripts");
    println!("cargo:rustc-env=KFTRAY_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}

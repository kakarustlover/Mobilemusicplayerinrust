fn main() {
    // rodio -> cpal -> oboe (C++). The NDK C++ runtime must be linked into libvelora.so,
    // otherwise dlopen fails at startup with: cannot locate symbol "__cxa_pure_virtual".
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=static=c++_static");
        println!("cargo:rustc-link-lib=static=c++abi");
    }
}

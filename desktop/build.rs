use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Provide symlink for xkbcommon-x11 if only .so.0 is available on system
    let xkb_x11_src = PathBuf::from("/usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0");
    if xkb_x11_src.exists() {
        let target = out_dir.join("libxkbcommon-x11.so");
        if !target.exists() {
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(&xkb_x11_src, &target);
        }
    }

    let xkb_reg_src = PathBuf::from("/usr/lib/x86_64-linux-gnu/libxkbregistry.so.0");
    if xkb_reg_src.exists() {
        let target = out_dir.join("libxkbregistry.so");
        if !target.exists() {
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(&xkb_reg_src, &target);
        }
    }

    println!("cargo:rustc-link-search=native={}", out_dir.display());
}

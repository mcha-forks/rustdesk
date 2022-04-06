#[cfg(target_os = "linux")]
use pkg_config;
#[cfg(target_os = "linux")]
use std::env;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::path::Path;

#[cfg(target_os = "linux")]
fn main() {
    let libraries = [
        "xext",
        "gl",
        "xcursor",
        "xxf86vm",
        "xft",
        "xinerama",
        "xi",
        "x11",
        "xlib_xcb",
        "xmu",
        "xrandr",
        "xtst",
        "xrender",
        "xscrnsaver",
        "xt",
    ];

    let mut config = String::new();
    for lib in libraries.iter() {
        let libdir = match pkg_config::get_variable(lib, "libdir") {
            Ok(libdir) => format!("Some(\"{}\")", libdir),
            Err(_) => "None".to_string(),
        };
        config.push_str(&format!(
            "pub const {}: Option<&'static str> = {};\n",
            lib, libdir
        ));
    }
    let config = format!("pub mod config {{ pub mod libdir {{\n{}}}\n}}", config);
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("config.rs");
    let mut f = File::create(&dest_path).unwrap();
    f.write_all(&config.into_bytes()).unwrap();

    println!("cargo:rustc-link-lib=dl");
}

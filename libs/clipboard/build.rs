use cc;

fn build_c_impl() {
    let mut build = cc::Build::new();
    #[cfg(target_os = "linux")]
    build.file("src/x11/xf_cliprdr.c");

    build.flag_if_supported("-Wno-c++0x-extensions");
    build.flag_if_supported("-Wno-return-type-c-linkage");
    build.flag_if_supported("-Wno-invalid-offsetof");
    build.flag_if_supported("-Wno-unused-parameter");

    build.flag("-fPIC");
    // build.flag("-std=c++11");
    // build.flag("-include");
    // build.flag(&confdefs_path.to_string_lossy());


    build.compile("mycliprdr");
    
    #[cfg(target_os = "linux")]
    println!("cargo:rerun-if-changed=src/x11/xf_cliprdr.c");
}

fn main() {
    build_c_impl();
}

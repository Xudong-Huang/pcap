use std::path::Path;
use std::process::{self, Command};

fn main() {
    println!("debug:Running the build for libpcap");

    // we rerun the build if the `build.rs` file is changed.
    println!("cargo:rerun-if-changed=build.rs");

    // make sure that the Git submodule is checked out
    if !Path::new("libpcap/.git").exists() {
        let _ = Command::new("git")
            .args(&["submodule", "update", "--init"])
            .status();
    }

    // use and configure cmake to build the Paho C lib
    let mut cmk_cfg = cmake::Config::new("libpcap/");
    cmk_cfg
        .define("BUILD_SHARED_LIBS", "off")
        .define("DISABLE_USB", "on")
        .define("DISABLE_BLUETOOTH", "on")
        .define("DISABLE_NETMAP", "on")
        .define("DISABLE_DBUS", "on")
        .define("DISABLE_RDMA", "on")
        .define("DISABLE_DAG", "on")
        .define("DISABLE_SEPTEL", "on")
        .define("DISABLE_SNF", "on")
        .define("DISABLE_TC", "on");

    // 'cmk' is a PathBuf to the cmake install directory
    let cmk = cmk_cfg.build();
    println!("debug:CMake output dir: {}", cmk.display());

    // We check if the pcap library was compiled.
    let lib_path = if cmk.join("lib").exists() {
        "lib"
    } else if cmk.join("lib64").exists() {
        "lib64"
    } else {
        panic!("Unknown library directory.")
    };

    // Absolute path to pcap lib
    let lib_dir = cmk.join(lib_path);

    let lib_name = "pcap";
    let pcap_link_name = if cfg!(windows) {
        format!("{}.lib", lib_name)
    } else {
        format!("lib{}.a", lib_name)
    };

    let pcap_lib_path = lib_dir.join(pcap_link_name);
    println!("debug:Using pcap library at: {}", pcap_lib_path.display());

    if !pcap_lib_path.exists() {
        println!("Error building pcap library: '{}'", pcap_lib_path.display());
        process::exit(103);
    }

    // we add the folder where all the libraries are built to the path search
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static={}", lib_name);
}

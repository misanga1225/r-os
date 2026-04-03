use std::path::PathBuf;

fn main() {
    let kernel_path = std::env::var("CARGO_BIN_FILE_KERNEL_kernel").expect(
        "CARGO_BIN_FILE_KERNEL_kernel not set — is the kernel artifact dependency configured?",
    );
    let kernel_path = PathBuf::from(kernel_path);

    let bios_path = kernel_path.with_extension("img");

    bootloader::BiosBoot::new(&kernel_path)
        .create_disk_image(&bios_path)
        .unwrap();

    println!("cargo:rustc-env=BIOS_PATH={}", bios_path.display());
}

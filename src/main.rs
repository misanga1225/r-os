use std::process::Command;

fn main() {
    let bios_path = env!("BIOS_PATH");

    let mut cmd = Command::new("qemu-system-x86_64");
    cmd.arg("-drive")
        .arg(format!("format=raw,file={bios_path}"))
        .arg("-serial")
        .arg("mon:stdio")
        .arg("-display")
        .arg("none")
        .arg("-device")
        .arg("isa-debug-exit,iobase=0xf4,iosize=0x04");

    let status = cmd.status().expect("failed to launch QEMU");

    // isa-debug-exit maps exit code 0x10 -> (0x10 << 1) | 1 = 0x21 = 33
    let code = status.code().unwrap_or(1);
    std::process::exit(if code == 33 { 0 } else { code });
}

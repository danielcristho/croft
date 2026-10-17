use which::which;

// main reruns the build when bpf-linker changes; without it, the userspace tests still build with AYA_BUILD_SKIP=1.
fn main() {
    if let Ok(bpf_linker) = which("bpf-linker") {
        println!("cargo:rerun-if-changed={}", bpf_linker.display());
    }
}

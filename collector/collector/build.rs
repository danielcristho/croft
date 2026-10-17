use std::{env, fs, path::PathBuf};

use anyhow::{Context as _, anyhow};
use aya_build::Toolchain;

// With AYA_BUILD_SKIP=1 aya-build writes no object; an empty one lets the userspace code compile for tests.
const SKIP_ENV: &str = "AYA_BUILD_SKIP";
// The eBPF binary's name, the file src/ebpf.rs embeds from OUT_DIR.
const EBPF_OBJECT: &str = "collector";

// Build script for the collector-ebpf crate. This script is responsible for building the eBPF programs and ensuring that the bpf-linker tool is available.
fn main() -> anyhow::Result<()> {
    let cargo_metadata::Metadata { packages, .. } = cargo_metadata::MetadataCommand::new()
        .no_deps()
        .exec()
        .context("MetadataCommand::exec")?;
    let ebpf_package = packages
        .into_iter()
        .find(|cargo_metadata::Package { name, .. }| name.as_str() == "collector-ebpf")
        .ok_or_else(|| anyhow!("collector-ebpf package not found"))?;
    let cargo_metadata::Package {
        name,
        manifest_path,
        ..
    } = ebpf_package;
    let ebpf_package = aya_build::Package {
        name: name.as_str(),
        root_dir: manifest_path
            .parent()
            .ok_or_else(|| anyhow!("no parent for {manifest_path}"))?
            .as_str(),
        ..Default::default()
    };
    aya_build::build_ebpf([ebpf_package], Toolchain::default())?;
    if env::var(SKIP_ENV).is_ok_and(|skip| skip == "1" || skip.eq_ignore_ascii_case("true")) {
        let object = PathBuf::from(env::var("OUT_DIR")?).join(EBPF_OBJECT);
        if !object.exists() {
            fs::write(&object, []).with_context(|| format!("write {}", object.display()))?;
        }
    }
    Ok(())
}

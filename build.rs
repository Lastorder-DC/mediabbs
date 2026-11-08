//! Sets `MEDIABBS_VERSION`: the crate version plus `-<7-char commit hash>`
//! ("0.4.0-1a2b3c4"), or the plain version ("0.4.0") for release packages.
//!
//! - `MEDIABBS_GIT_HASH` set (Docker builds, which have no `.git`; pass it
//!   with `--build-arg`): that hash, or the plain version when empty.
//! - Otherwise a git checkout always shows its commit, even on a release tag:
//!   only a release tarball (no git) or release image gives the plain version.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}

fn main() {
    println!("cargo:rerun-if-env-changed=MEDIABBS_GIT_HASH");
    let version = env!("CARGO_PKG_VERSION");
    let hash = match std::env::var("MEDIABBS_GIT_HASH") {
        Ok(hash) => Some(hash.trim().to_owned()),
        Err(_) => {
            // Rebuild when the checked-out commit changes.
            if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
                println!("cargo:rerun-if-changed={dir}/HEAD");
                if let Some(head) = git(&["symbolic-ref", "-q", "HEAD"]) {
                    println!("cargo:rerun-if-changed={dir}/{head}");
                }
                println!("cargo:rerun-if-changed={dir}/packed-refs");
            }
            git(&["rev-parse", "--short=7", "HEAD"])
        }
    };
    let hash: String = hash
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(7)
        .collect();
    if hash.is_empty() {
        println!("cargo:rustc-env=MEDIABBS_VERSION={version}");
    } else {
        println!("cargo:rustc-env=MEDIABBS_VERSION={version}-{hash}");
    }
}

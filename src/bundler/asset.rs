use std::{ffi::OsStr, path::Path};

use anyhow::{Context, Result};
use walkdir::WalkDir;

fn is_asset_path(path: &Path) -> bool {
    path.extension()
        .map(|e| e != OsStr::new("scss") && e != OsStr::new("ts") && e != OsStr::new("js"))
        .unwrap_or_default()
}

pub fn copy_assets(src_dir: &Path, dst_dir: &Path) -> Result<()> {
    for entry in WalkDir::new(src_dir).min_depth(1) {
        let src_path = entry
            .context("unable to traverse public directory")?
            .into_path();
        if is_asset_path(&src_path) {
            let dst_path = dst_dir.join(src_path.strip_prefix(src_dir).expect(""));
            dst_path
                .parent()
                .map(std::fs::create_dir_all)
                .transpose()
                .context("unable to create asset directory")?;
            create_asset_link(&src_path, &dst_path).context("unable to copy asset")?;
        }
    }

    Ok(())
}

#[cfg(unix)]
pub fn create_asset_link(src: &Path, dst: &Path) -> std::io::Result<()> {
    dst.parent().map(std::fs::create_dir_all).transpose()?;
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
pub fn create_asset_link(src: &Path, dst: &Path) -> std::io::Result<()> {
    dst.parent().map(std::fs::create_dir_all).transpose()?;
    std::os::windows::fs::symlink_file(src, dst)
}

#[cfg(all(not(windows), not(unix)))]
pub fn create_asset_link(src: &Path, dst: &Path) -> std::io::Result<()> {
    dst.parent().map(std::fs::create_dir_all).transpose()?;
    std::fs::copy(src, dst)
}

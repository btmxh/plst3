use super::write_contents;
use anyhow::{Context, Result};
use rsass::output::Style;
use std::path::Path;
use walkdir::WalkDir;

pub fn compile_scss(src_path: &Path, dst_path: &Path) -> Result<()> {
    let result = rsass::compile_scss_path(
        src_path,
        rsass::output::Format {
            style: if cfg!(debug_assertions) {
                Style::Expanded
            } else {
                Style::Compressed
            },
            precision: 5,
        },
    )
    .context("Error compiling SCSS")?;
    let mut dst_path = dst_path.to_owned();
    dst_path.set_extension("css");
    write_contents(&dst_path, &result).context("unable to write css to file")?;
    Ok(())
}

pub fn compile_all_scss(src_dir: &Path, dst_dir: &Path) -> Result<()> {
    for entry in WalkDir::new(src_dir).min_depth(1) {
        let src_path = entry
            .context("unable to traverse src directory")?
            .into_path();
        let dst_path = dst_dir.join(
            src_path
                .strip_prefix(src_dir)
                .expect("src_path should start with src_dir"),
        );
        if src_path
            .extension()
            .map(|e| e.to_string_lossy() == "scss")
            .unwrap_or_default()
        {
            compile_scss(&src_path, &dst_path).context("unable to compile scss")?;
        }
    }

    Ok(())
}

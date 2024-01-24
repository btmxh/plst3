use std::{fs::OpenOptions, io::Write, path::Path, time::Duration};

use anyhow::{anyhow, Context, Result};
use notify::{RecommendedWatcher, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};

use crate::bundler::rsass::compile_scss;

use self::{asset::copy_assets, rsass::compile_all_scss, swc::compile_scripts};

mod asset;
mod rsass;
mod swc;

pub struct Bundler {
    debouncer: Debouncer<RecommendedWatcher, FileIdMap>,
}

pub fn launch_bundler() -> Result<Bundler> {
    let watch_dir = Path::new("public")
        .canonicalize()
        .context("unable to canonicalize public dir")?;
    let dest_dir = Path::new("dist");
    let mut debouncer = new_debouncer(Duration::from_secs(1), None, {
        let watch_dir = watch_dir.clone();
        move |result: DebounceEventResult| {
            let mut scripts_updated = false;
            match result {
            Ok(events) => events.iter().for_each(|event| {
                match event.event.kind {
                    notify::EventKind::Access(_) | notify::EventKind::Remove(_) => return,
                    notify::EventKind::Any | notify::EventKind::Create(_) | notify::EventKind::Modify(_) | notify::EventKind::Other => {},
                }
                event.event.paths
                    .iter()
                    .filter_map(|p| {
                        p.canonicalize()
                            .map_err(|e| tracing::warn!("error canonicalizing path: {e}"))
                            .ok()
                    })
                    .filter(|p| p.is_file())
                    .filter_map(|p| {
                        p.strip_prefix(&watch_dir)
                            .map_err(|_| {
                                tracing::warn!("watched file with canonical path not in public dir (likely a symlink), currently not supported")
                            })
                            .ok()
                            .map(|rel| dest_dir.join(rel)).map(|dst_path| (p, dst_path))
                    }).for_each(|(src_path, dst_path)| {
                        match src_path.extension().and_then(|s| s.to_str()) {
                            Some("ts") | Some("js") => {
                                scripts_updated = true;
                                Ok(())
                            }
                            Some("scss") => {
                                compile_scss(&src_path, &dst_path).context("failed attempting to transpiling scss")
                            }
                            _ => {
                                std::fs::copy(&src_path, &dst_path).context("error copying css file").map(|_| {})
                            }
                        }.map_err(|e| {
                            tracing::warn!("{e}");
                        }).ok();
                    });
            }),
            Err(errors) => errors
                .iter()
                .for_each(|e| tracing::warn!("error in filewatch: {e}")),
        }

        if scripts_updated {
            compile_scripts(&watch_dir, dest_dir).context("unable to compile scripts: {}").map_err(|e| tracing::warn!("{e}")).ok();
        }
    }}
    )
    .context("unable to create file watch")?;
    debouncer
        .watcher()
        .watch(Path::new("public"), notify::RecursiveMode::Recursive)
        .context("unable to add watch directory")?;

    compile_all_scss(&watch_dir, dest_dir)
        .context("unable to compile scss")
        .map_err(|e| tracing::warn!("{e}"))
        .ok();
    compile_scripts(&watch_dir, dest_dir)
        .context("unable to compile scripts")
        .map_err(|e| tracing::warn!("{e}"))
        .ok();
    copy_assets(&watch_dir, dest_dir)
        .context("unable to copy assets")
        .map_err(|e| tracing::warn!("{e}"))
        .ok();

    Ok(Bundler { debouncer })
}

fn write_contents(path: &Path, content: &[u8]) -> Result<()> {
    path.parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .context("unable to create parent for path")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .context("unable to open destination path")?;
    file.write_all(content).context("unable to write to file")?;
    Ok(())
}

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use swc::{
    config::{IsModule, SourceMapsConfig},
    Compiler,
};
use swc_common::{
    comments::SingleThreadedComments, errors::Handler, sync::Lrc, FilePathMapping, Mark,
    SourceFile, SourceMap, GLOBALS,
};
use swc_ecma_ast::EsVersion;
use swc_ecma_codegen::{text_writer::JsWriter, Config, Emitter};
use swc_ecma_parser::Syntax;
use swc_ecma_transforms_typescript::strip;
use swc_ecma_visit::FoldWith;
use walkdir::WalkDir;

use crate::bundler::write_contents;

struct ScriptFile {
    src_path: PathBuf,
    dst_path: PathBuf,
    source_map_path: PathBuf,
    swc_file: Lrc<SourceFile>,
    syntax: Syntax,
}

fn script_syntax(path: &Path) -> Option<Syntax> {
    match path
        .extension()
        .map(|e| e.to_string_lossy())
        .unwrap_or_default()
        .as_ref()
    {
        "js" => Some(Syntax::Es(Default::default())),
        "ts" => Some(Syntax::Typescript(Default::default())),
        _ => None,
    }
}

pub fn compile_scripts(src_dir: &Path, dst_dir: &Path) -> Result<()> {
    let cm = Lrc::new(SourceMap::new(FilePathMapping::new(vec![])));
    let compiler = Compiler::new(cm.clone());

    let mut scripts = Vec::new();
    for src_path in WalkDir::new(src_dir).min_depth(1) {
        let src_path = src_path.context("unable to glob script file")?.into_path();
        let syntax = script_syntax(&src_path);
        if let Some(syntax) = syntax {
            let mut dst_path = dst_dir.join(
                src_path
                    .strip_prefix(src_dir)
                    .expect("src_path should have the same prefix"),
            );
            let mut ext_changed = dst_path.set_extension("js");
            let mut source_map_path = dst_path.clone();
            ext_changed &= source_map_path.set_extension("js.map");
            assert!(ext_changed);
            let content =
                std::fs::read_to_string(&src_path).context("unable to read script file")?;
            let swc_file =
                cm.new_source_file(swc_common::FileName::Real(src_path.clone()), content);
            scripts.push(ScriptFile {
                src_path,
                dst_path,
                swc_file,
                source_map_path,
                syntax,
            });
        }
    }

    let handler =
        Handler::with_emitter_writer(Box::new(std::io::stderr()), Some(compiler.cm.clone()));
    let comments = SingleThreadedComments::default();
    GLOBALS.set(&Default::default(), || -> Result<()> {
        let compile_results = scripts
            .iter()
            .map(|script| {
                compiler
                    .parse_js(
                        script.swc_file.clone(),
                        &handler,
                        EsVersion::Es2022,
                        script.syntax,
                        IsModule::Bool(true),
                        Some(compiler.comments()),
                    )
                    .map(|prog| (script, prog))
            })
            .collect::<Result<Vec<_>>>()
            .context("unable to compile js/ts")?;
        for (script, program) in compile_results {
            let top_level_mark = Mark::new();
            let module = program
                .fold_with(&mut strip(top_level_mark))
                .module()
                .expect("module should be enabled");
            let filename = script.src_path.file_name().map(|s| s.to_string_lossy());
            let output = compiler
                .print(
                    &module,
                    filename.as_deref(),
                    script.dst_path.parent().map(|p| p.to_owned()),
                    false,
                    SourceMapsConfig::Bool(true),
                    &Default::default(),
                    None,
                    Some(compiler.comments()),
                    true,
                    "",
                    swc_ecma_codegen::Config::default().with_target(EsVersion::Es2022),
                )
                .context("unable to generate code for script")?;
            write_contents(&script.dst_path, output.code.as_bytes())
                .context("error writing script file")?;
            if let Some(source_map) = output.map {
                write_contents(&script.source_map_path, source_map.as_bytes())
                    .context("error writing source map")
                    .map_err(|e| tracing::warn!("{e}"))
                    .ok();
            }
        }
        Ok(())
    })
}

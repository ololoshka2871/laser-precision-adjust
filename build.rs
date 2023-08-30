use std::io;

use swc::{
    config::{IsModule, SourceMapsConfig},
    Compiler,
};
use swc_common::{errors::Handler, source_map::SourceMap, sync::Lrc, Mark, GLOBALS};
use swc_ecma_ast::EsVersion;
use swc_ecma_parser::Syntax;
use swc_ecma_transforms_typescript::strip;
use swc_ecma_visit::FoldWith;

// https://stackoverflow.com/a/76828821
/// Transforms typescript to javascript. Returns tuple (js string, source map)
fn ts_to_js(filename: &str, ts_code: &str) -> (String, String) {
    let cm = Lrc::new(SourceMap::new(swc_common::FilePathMapping::empty()));

    let compiler = Compiler::new(cm.clone());

    let source = cm.new_source_file(
        swc_common::FileName::Custom(filename.into()),
        ts_code.to_string(),
    );

    let handler = Handler::with_emitter_writer(Box::new(io::stderr()), Some(compiler.cm.clone()));

    return GLOBALS.set(&Default::default(), || {
        let res = compiler
            .parse_js(
                source,
                &handler,
                EsVersion::Es5,
                Syntax::Typescript(Default::default()),
                IsModule::Bool(true),
                Some(compiler.comments()),
            )
            .expect("parse_js failed");

        let module = res.module().unwrap();

        // Add TypeScript type stripping transform
        let top_level_mark = Mark::new();
        let module = module.fold_with(&mut strip(top_level_mark));

        // https://rustdoc.swc.rs/swc/struct.Compiler.html#method.print
        let ret = compiler
            .print(
                &module,                      // ast to print
                None,                         // source file name
                None,                         // output path
                false,                        // inline sources content
                EsVersion::EsNext,            // target ES version
                SourceMapsConfig::Bool(true), // source map config
                &Default::default(),          // source map names
                None,                         // original source map
                false,                        // minify
                Some(compiler.comments()),    // comments
                false,                        // emit source map columns
                false,                        // ascii only
                "",                           // preable
            )
            .expect("print failed");

        return (ret.code, ret.map.expect("no source map"));
    });
}

fn main() {
    // get all .ts files from www/ts, transform them to js and write to www/js
    let ts_files = std::fs::read_dir("www/ts").expect("Failed to read directory");
    let ts_files = ts_files.filter_map(|entry| match entry {
        Ok(entry) => {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "ts" {
                        Some(path)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        Err(_err) => None,
    });

    for ts_file in ts_files {
        let ts_file_name = ts_file
            .file_name()
            .expect("Failed to get file name")
            .to_str()
            .expect("Failed to convert file name to string");
        let ts_file_name = ts_file_name
            .strip_suffix(".ts")
            .expect("Failed to strip suffix")
            .to_string();

        let ts_file_path = ts_file.to_str().expect("Failed to convert path to string");

        let ts_code = std::fs::read_to_string(ts_file_path).expect("Failed to read file");

        let (js_code, source_map) = ts_to_js(&ts_file_name, &ts_code);

        let js_file_path = format!("www/js/{}.js", ts_file_name);
        let source_map_file_path = format!("www/js/{}.js.map", ts_file_name);

        std::fs::write(js_file_path, js_code).expect("Failed to write file");
        std::fs::write(source_map_file_path, source_map).expect("Failed to write file");

        println!("cargo:rerun-if-changed={}", ts_file_path);
    }
    println!("cargo:rerun-if-changed=build.rs");
}

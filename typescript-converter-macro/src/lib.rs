#![feature(proc_macro_span)]

use std::io;
use std::path::PathBuf;

use swc::{
    config::{IsModule, SourceMapsConfig},
    Compiler,
};
use swc_common::{errors::Handler, source_map::SourceMap, sync::Lrc, Mark, GLOBALS};
use swc_ecma_ast::EsVersion;
use swc_ecma_parser::Syntax;
use swc_ecma_transforms_typescript::strip;
use swc_ecma_visit::FoldWith;

use proc_macro::Span;
use quote::quote;
use syn::{parse_macro_input, LitStr};


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

#[proc_macro]
pub fn include_ts(file: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let span = Span::call_site();
    let source = span.source_file();

    let infile = parse_macro_input!(file as LitStr).value();
    let ts_file_name = source
        .path()
        .parent()
        .expect("Invalid path")
        .join(PathBuf::from(infile));

    if !ts_file_name.exists() {
        panic!(
            "file '{:?}' in '{:?}' not found",
            ts_file_name,
            std::env::current_dir().unwrap()
        );
    }

    let ts_file_name_str = ts_file_name.to_str().unwrap().to_owned();
    let ts_code = std::fs::read_to_string(ts_file_name).expect("Failed to read file");
    let (js_code, source_map) = ts_to_js(&ts_file_name_str, &ts_code);

    quote! {
        (#js_code, #source_map)
    }
    .into()
}

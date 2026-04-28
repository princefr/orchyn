use crate::parser::SymbolParser;
use crate::types::*;
use anyhow::Result;
use ignore::WalkBuilder;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub struct Indexer {
    root: PathBuf,
}

impl Indexer {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    pub fn index(&self) -> Result<AppMap> {
        info!("Indexing codebase at {:?}", self.root);

        let walker = WalkBuilder::new(&self.root)
            .hidden(false)
            .git_ignore(true)
            .build();

        let files: Vec<FileInfo> = walker
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .par_bridge() // Parallel processing
            .filter_map(|entry| {
                let path = entry.path().strip_prefix(&self.root).ok()?.to_path_buf();
                self.process_file(&self.root.join(&path), path).ok()
            })
            .collect();

        // Placeholder for route and state store extraction
        let routes = Vec::new();
        let state_stores = Vec::new();

        // Extract projects based on manifest files
        let mut projects = Vec::new();
        for file in &files {
            if let Some(file_name) = file.path.file_name().and_then(|n| n.to_str()) {
                let manifest_type = match file_name {
                    "package.json" => Some("npm"),
                    "Cargo.toml" => Some("cargo"),
                    "pubspec.yaml" => Some("flutter"),
                    "requirements.txt" | "pyproject.toml" => Some("python"),
                    "pom.xml" => Some("maven"),
                    "build.gradle" => Some("gradle"),
                    _ => None,
                };

                if let Some(mt) = manifest_type {
                    if let Some(parent) = file.path.parent() {
                        projects.push(ProjectInfo {
                            path: parent.to_path_buf(),
                            manifest_type: mt.to_string(),
                        });
                    }
                }
            }
        }

        Ok(AppMap {
            root: self.root.clone(),
            files,
            routes,
            state_stores,
            projects,
        })
    }

    fn process_file(&self, full_path: &Path, relative_path: PathBuf) -> Result<FileInfo> {
        let language = match relative_path.extension().and_then(|ext| ext.to_str()) {
            Some("ts") | Some("tsx") => Language::TypeScript,
            Some("js") | Some("jsx") => Language::JavaScript,
            Some("dart") => Language::Dart,
            Some("rs") => Language::Rust,
            _ => Language::Unknown,
        };

        let mut symbols = Vec::new();
        if matches!(language, Language::TypeScript | Language::JavaScript) {
            if let Ok(source) = std::fs::read_to_string(full_path) {
                if let Ok(mut parser) = SymbolParser::new(language.clone()) {
                    symbols = parser
                        .parse_symbols(&source, &relative_path)
                        .unwrap_or_default();
                }
            }
        }

        Ok(FileInfo {
            path: relative_path,
            language,
            symbols,
        })
    }
}

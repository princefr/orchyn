use orchyn_core::SourceRef;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMap {
    pub root: PathBuf,
    pub files: Vec<FileInfo>,
    pub routes: Vec<Route>,
    pub state_stores: Vec<StateStore>,
    pub projects: Vec<ProjectInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub path: PathBuf,
    pub manifest_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: PathBuf,
    pub language: Language,
    pub symbols: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Language {
    TypeScript,
    JavaScript,
    Dart,
    Rust,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: SourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Class,
    Interface,
    Variable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub path: String,
    pub handler: SourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateStore {
    pub name: String,
    pub definition: SourceRef,
}

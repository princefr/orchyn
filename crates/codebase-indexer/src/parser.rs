use crate::types::{Language, Symbol, SymbolKind};
use anyhow::{Context, Result};
use orchyn_core::SourceRef;
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

pub struct SymbolParser {
    ts_parser: Parser,
}

impl SymbolParser {
    pub fn new(language: Language) -> Result<Self> {
        let mut ts_parser = Parser::new();
        let ts_lang = match language {
            Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            _ => return Err(anyhow::anyhow!("Unsupported language for parsing")),
        };
        ts_parser.set_language(&ts_lang)?;
        Ok(Self { ts_parser })
    }

    pub fn parse_symbols(&mut self, source: &str, path: &Path) -> Result<Vec<Symbol>> {
        let tree = self
            .ts_parser
            .parse(source, None)
            .context("Failed to parse source code")?;

        // Simplified query for functions and classes
        let query_str = "(function_declaration name: (identifier) @name) @function
                        (method_definition name: (property_identifier) @name) @function
                        (class_declaration name: (type_identifier) @name) @class";

        let query = Query::new(&self.ts_parser.language().unwrap(), query_str)?;
        let mut cursor = QueryCursor::new();
        
        let mut symbols = Vec::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        
        // tree-sitter 0.25+ matches.next() returns &QueryMatch
        while let Some(m) = matches.next() {
            for cap in m.captures {
                let name = cap.node.utf8_text(source.as_bytes())?.to_string();
                let start = cap.node.start_position();
                let end = cap.node.end_position();

                let kind = if cap.index == 0 || cap.index == 1 {
                    SymbolKind::Function
                } else {
                    SymbolKind::Class
                };

                symbols.push(Symbol {
                    name,
                    kind,
                    range: SourceRef {
                        file: path.to_path_buf(),
                        line_start: start.row as usize,
                        line_end: end.row as usize,
                    },
                });
            }
        }

        Ok(symbols)
    }
}

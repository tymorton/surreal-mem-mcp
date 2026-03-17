use tree_sitter::{Language, Parser, Query, QueryCursor, Node};
use streaming_iterator::StreamingIterator;


#[derive(Debug, Clone)]
pub struct AstNode {
    pub name: String,
    pub kind: String,
    pub row: usize,
}

#[derive(Debug, Clone)]
pub struct AstResult {
    pub filepath: String,
    pub classes: Vec<AstNode>,
    pub functions: Vec<AstNode>,
    pub calls: Vec<String>,
    pub imports: Vec<String>,
}

pub struct AstParser {}

impl AstParser {
    pub fn new() -> Self {
        Self {}
    }

    pub fn parse_file(&self, filepath: &str, source_code: &str) -> Result<AstResult, String> {
        let ext = std::path::Path::new(filepath)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        let language: Language = match ext {
            "py" => tree_sitter_python::language(),
            "rs" => tree_sitter_rust::language(),
            "js" => tree_sitter_javascript::language(),
            "ts" => tree_sitter_typescript::language_typescript(),
            "tsx" => tree_sitter_typescript::language_tsx(),
            "go" => tree_sitter_go::language(),
            "java" => tree_sitter_java::language(),
            "cpp" | "cc" | "cxx" | "hpp" => tree_sitter_cpp::language(),
            "c" | "h" => tree_sitter_c::language(),
            "cs" => tree_sitter_c_sharp::language(),
            "rb" => tree_sitter_ruby::language(),
            "php" => tree_sitter_php::language(),
            "swift" => tree_sitter_swift::language(),
            _ => return Err(format!("Unsupported file extension: {}", ext)),
        };

        let mut parser = Parser::new();
        parser.set_language(language).map_err(|e| e.to_string())?;

        let tree = parser.parse(source_code, None).ok_or("Failed to parse tree")?;
        let root_node = tree.root_node();

        let mut classes = Vec::new();
        let mut functions = Vec::new();
        let mut calls = Vec::new();
        let mut imports = Vec::new();

        let (class_query, func_query, call_query, import_queries) = match ext {
            "py" => (
                "(class_definition name: (identifier) @class.name)",
                "(function_definition name: (identifier) @function.name)",
                "(call function: (identifier) @call.function)",
                vec![
                    "(import_from_statement module_name: (dotted_name) @import.module)",
                    "(import_statement name: (dotted_name) @import.name)"
                ]
            ),
            "rs" => (
                r#"
                [
                    (struct_item name: (type_identifier) @class.name)
                    (trait_item name: (type_identifier) @class.name)
                    (enum_item name: (type_identifier) @class.name)
                ]
                "#,
                r#"
                [
                    (function_item name: (identifier) @function.name)
                    (function_signature_item name: (identifier) @function.name)
                ]
                "#,
                r#"
                [
                    (call_expression function: (identifier) @call.function)
                    (call_expression function: (field_expression field: (field_identifier) @call.function))
                    (call_expression function: (scoped_identifier name: (identifier) @call.function))
                ]
                "#,
                vec![
                    "(use_declaration argument: (scoped_identifier name: (identifier) @import.name))",
                    "(use_declaration argument: (identifier) @import.name)",
                    "(use_declaration argument: (scoped_use_list path: (identifier) @import.name))"
                ]
            ),
            "js" | "ts" | "tsx" => (
                "(class_declaration name: (identifier) @class.name)",
                r#"
                [
                    (function_declaration name: (identifier) @function.name)
                    (method_definition name: (property_identifier) @function.name)
                    (arrow_function) @function.name
                ]
                "#,
                "(call_expression function: [(identifier) (member_expression property: (property_identifier))] @call.function)",
                vec![
                    "(import_statement source: (string) @import.name)",
                    "(call_expression function: (import) arguments: (arguments (string) @import.name))"
                ]
            ),
            "go" => (
                "(type_spec name: (type_identifier) @class.name)",
                r#"
                [
                    (function_declaration name: (identifier) @function.name)
                    (method_declaration name: (field_identifier) @function.name)
                ]
                "#,
                "(call_expression function: [(identifier) (selector_expression field: (field_identifier))] @call.function)",
                vec!["(import_spec path: (interpreted_string_literal) @import.name)"]
            ),
            "java" => (
                "(class_declaration name: (identifier) @class.name)",
                "(method_declaration name: (identifier) @function.name)",
                "(method_invocation name: (identifier) @call.function)",
                vec!["(import_declaration (scoped_identifier) @import.name)"]
            ),
            "cpp" | "cc" | "cxx" | "hpp" | "c" | "h" => (
                r#"
                [
                    (class_specifier name: (type_identifier) @class.name)
                    (struct_specifier name: (type_identifier) @class.name)
                ]
                "#,
                "(function_definition declarator: (function_declarator declarator: [(identifier) (field_identifier)] @function.name))",
                "(call_expression function: [(identifier) (field_expression field: (field_identifier))] @call.function)",
                vec!["(preproc_include path: [(string_literal) (system_lib_string)] @import.name)"]
            ),
            "cs" => (
                "(class_declaration name: (identifier) @class.name)",
                "(method_declaration name: (identifier) @function.name)",
                "(invocation_expression function: [(identifier) (member_access_expression name: (identifier))] @call.function)",
                vec!["(using_directive (identifier) @import.name)"]
            ),
            "rb" => (
                "(class name: [(constant) (scope_resolution name: (constant))] @class.name)",
                "(method name: (identifier) @function.name)",
                "(call method: (identifier) @call.function)",
                vec!["(call method: (identifier) @import.name (#match? @import.name \"^(require|require_relative)$\"))"]
            ),
            "php" => (
                "(class_declaration name: (name) @class.name)",
                "(method_declaration name: (name) @function.name)",
                "(function_call_expression function: [(name) (member_call_expression name: (name))] @call.function)",
                vec!["(namespace_use_clause (name) @import.name)"]
            ),
            "swift" => (
                "(class_declaration name: (type_identifier) @class.name)",
                "(function_declaration name: (identifier) @function.name)",
                "(call_expression function: [(identifier) (member_expression name: (identifier))] @call.function)",
                vec!["(import_declaration (identifier) @import.name)"]
            ),
            _ => return Err("Unsupported extension".to_string()),
        };

        self.extract_nodes(&language, class_query, root_node, source_code, &mut classes, "class");
        self.extract_nodes(&language, func_query, root_node, source_code, &mut functions, "function");
        self.extract_names(&language, call_query, root_node, source_code, &mut calls);
        for q in import_queries {
            self.extract_names(&language, q, root_node, source_code, &mut imports);
        }

        Ok(AstResult {
            filepath: filepath.to_string(),
            classes,
            functions,
            calls,
            imports,
        })
    }

    fn extract_nodes(&self, language: &Language, query_str: &str, root: Node, source: &str, dest: &mut Vec<AstNode>, kind: &str) {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        if let Ok(query) = Query::new(*language, query_str) {
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(&query, root, source.as_bytes());
            while let Some(m) = matches.next() {
                let mut current_caps: Vec<tree_sitter::Node> = Vec::new();
                for cap in m.captures {
                    current_caps.push(cap.node);
                }
                
                for node in current_caps {
                    if seen.insert(node.id()) {
                        if let Ok(text) = node.utf8_text(source.as_bytes()) {
                            dest.push(AstNode {
                                name: text.to_string(),
                                kind: kind.to_string(),
                                row: node.start_position().row,
                            });
                        }
                    }
                }
            }
        }
    }

    fn extract_names(&self, language: &Language, query_str: &str, root: Node, source: &str, dest: &mut Vec<String>) {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        if let Ok(query) = Query::new(*language, query_str) {
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(&query, root, source.as_bytes());
            while let Some(m) = matches.next() {
                let mut current_caps: Vec<tree_sitter::Node> = Vec::new();
                for cap in m.captures {
                    current_caps.push(cap.node);
                }
                
                for node in current_caps {
                    if seen.insert(node.id()) {
                        if let Ok(text) = node.utf8_text(source.as_bytes()) {
                            dest.push(text.to_string());
                        }
                    }
                }
            }
        }
    }
}

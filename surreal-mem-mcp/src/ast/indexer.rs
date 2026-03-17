use std::collections::HashMap;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use walkdir::WalkDir;
use serde_json::json;
use crate::ast::parser::{AstParser, AstResult};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn deterministic_hash(prefix: &str, data: &str) -> String {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{}_{}", prefix, hasher.finish())
}

pub async fn index_local_codebase(path: String, db: Arc<Surreal<Db>>) -> Result<String, String> {
    let mut ast_results = Vec::new();

    // 1. Walk directory and parse all python files
    for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
        let fpath = entry.path();
        if fpath.is_file() {
            let path_str = fpath.to_string_lossy();
            println!("Found path: {}", path_str);
            let supported_exts = ["py", "rs", "js", "ts", "tsx", "go", "java", "cpp", "cc", "cxx", "hpp", "c", "h", "cs", "rb", "php", "swift"];
            
            if supported_exts.iter().any(|&ext| path_str.ends_with(&format!(".{}", ext))) {
                match std::fs::read_to_string(fpath) {
                    Ok(source_code) => {
                        let parser = AstParser::new();
                        match parser.parse_file(&path_str, &source_code) {
                            Ok(result) => {
                                ast_results.push(result);
                            },
                            Err(e) => println!("Tree-Sitter AST Parse error [Skipping {}]: {}", path_str, e)
                        }
                    },
                    Err(_) => {
                        // Silently handle UTF-8 panics (e.g. .min.js binaries masking as source)
                        // Do not crash the `walkdir` batch execution.
                        println!("Unable to read UTF-8 string data [Skipping Binary]: {}", path_str);
                    }
                }
            }
        }
    }

    let parsed_count = ast_results.len();
    if parsed_count == 0 {
        return Ok("No supported code files found to index.".to_string());
    }

    let mut imports_map: HashMap<String, String> = HashMap::new();

    // Pass 1: Extract Nodes (Files, Functions, Classes) and Contains Edges
    for ast_data in &ast_results {
        let file_path = &ast_data.filepath;
        let file_hash = deterministic_hash("file", file_path);
        let file_id = format!("file:{}", file_hash);

        // upsert file node
        let _ = db.query(&format!("UPDATE file:⟨{}⟩ CONTENT {{ path: $path }}", file_hash))
            .bind(("path", file_path))
            .await.map_err(|e| e.to_string())?;

        // upsert functions
        for func in &ast_data.functions {
            let func_hash = deterministic_hash("func", &format!("{}:{}:{}", file_path, func.name, func.row));
            let func_id = format!("func:{}", func_hash);
            
            imports_map.insert(func.name.clone(), func_id.clone());

            let _ = db.query(&format!("UPDATE func:⟨{}⟩ CONTENT {{ name: $name, path: $path, row: $row }}", func_hash))
                .bind(("name", func.name.clone()))
                .bind(("path", file_path))
                .bind(("row", func.row))
                .await.map_err(|e| e.to_string())?;

            // Contains edge
            let _ = db.query(&format!("RELATE file:⟨{}⟩->contains->func:⟨{}⟩", file_hash, func_hash))
                .await.map_err(|e| e.to_string())?;
        }

        // upsert classes
        for cls in &ast_data.classes {
            let cls_hash = deterministic_hash("class", &format!("{}:{}:{}", file_path, cls.name, cls.row));
            let cls_id = format!("class:{}", cls_hash);

            imports_map.insert(cls.name.clone(), cls_id.clone());

            let _ = db.query(&format!("UPDATE class:⟨{}⟩ CONTENT {{ name: $name, path: $path, row: $row }}", cls_hash))
                .bind(("name", cls.name.clone()))
                .bind(("path", file_path))
                .bind(("row", cls.row))
                .await.map_err(|e| e.to_string())?;

            // Contains edge
            let _ = db.query(&format!("RELATE file:⟨{}⟩->contains->class:⟨{}⟩", file_hash, cls_hash))
                .await.map_err(|e| e.to_string())?;
        }
    }

    // Pass 2: Link Edges (Calls, Imports)
    // To link correctly, we must store the lookup file hash per file in a map.
    for ast_data in &ast_results {
        let file_path = &ast_data.filepath;
        // Since we are using UUIDs randomly, this pass 2 logic as written wouldn't work across multiple iterations without storing file hashes. Let's just lookup the file node implicitly via path.
        for call_name in &ast_data.calls {
            if let Some(target_id) = imports_map.get(call_name) {
                // target_id is e.g. "func:abcdef"
                let target_parts: Vec<&str> = target_id.split(':').collect();
                if target_parts.len() == 2 {
                    let tb = target_parts[0];
                    let hash = target_parts[1];
                    let _ = db.query(&format!("RELATE (SELECT id FROM file WHERE path = $path)->calls->{}:⟨{}⟩", tb, hash))
                        .bind(("path", file_path.to_string()))
                        .await.map_err(|e| e.to_string())?;
                }
            }
        }
        
        for imp in &ast_data.imports {
            let _ = db.query(&format!("UPDATE module:⟨{}⟩ CONTENT {{ name: $imp }}", imp))
                .bind(("imp", imp.clone()))
                .await.map_err(|e| e.to_string())?;
            
            let _ = db.query(&format!("RELATE (SELECT id FROM file WHERE path = $path)->imports->module:⟨{}⟩", imp))
                .bind(("path", file_path.to_string()))
                .await.map_err(|e| e.to_string())?;
        }
    }

    // Pass 3: State Sync (Prune untracked files)
    let active_file_ids: Vec<String> = ast_results
        .iter()
        .map(|a| format!("file:{}", deterministic_hash("file", &a.filepath)))
        .collect();

    // Since RELATE uses ON DELETE CASCADE in SurrealDB by default or leaves orphans, deleting the file handles contains/calls.
    // Wait, SurrealDB Graph links aren't strictly cascaded by default for standard DELETES.
    // Using DETACH-like logic: "DELETE func WHERE <-contains<-(file WHERE id NOT IN $active);"
    // Actually simpler: 
    let _ = db.query("
        BEGIN TRANSACTION;
        LET $dead_files = (SELECT id FROM file WHERE id NOT IN $active);
        DELETE func WHERE <-contains<-($dead_files);
        DELETE class WHERE <-contains<-($dead_files);
        DELETE $dead_files;
        COMMIT TRANSACTION;
    ")
    .bind(("active", active_file_ids))
    .await.map_err(|e| e.to_string())?;

    Ok(format!("Indexed {} files successfully.", parsed_count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surreal_client::SurrealClient;
    use std::fs;
    use serde_json::Value;

    #[tokio::test]
    async fn test_ast_indexer() {
        let temp_dir = std::env::temp_dir().join("surreal_mcp_test_ast");
        let _ = fs::remove_dir_all(&temp_dir); // clean any leftover state
        fs::create_dir_all(&temp_dir).unwrap();
        let db_client = SurrealClient::connect(temp_dir.join("test_db").to_string_lossy().to_string()).await.unwrap();
        
        let py_path = temp_dir.join("test.py");
        fs::write(&py_path, "
class MyController:
    def handle(self):
        log_event()

def log_event():
    import os
    pass
").unwrap();

        let res = index_local_codebase(temp_dir.to_string_lossy().to_string(), db_client.db()).await;
        let msg = res.unwrap();
        println!("{}", msg);

        let mut f_res = db_client.db().query("SELECT * FROM func").await.unwrap();
        let funcs: Vec<Value> = f_res.take(0).unwrap();
        assert_eq!(funcs.len(), 2, "Expected 2 functions (handle, log_event), got: {:?}", funcs);

        let mut c_res = db_client.db().query("SELECT * FROM class").await.unwrap();
        let classes: Vec<Value> = c_res.take(0).unwrap();
        assert_eq!(classes.len(), 1, "Expected 1 class (MyController)");
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[tokio::test]
    async fn test_ast_indexer_rust() {
        let temp_dir = std::env::temp_dir().join("surreal_mcp_test_ast_rs");
        let _ = fs::remove_dir_all(&temp_dir); // clean any leftover state
        fs::create_dir_all(&temp_dir).unwrap();
        let db_client = SurrealClient::connect(temp_dir.join("test_db_rs").to_string_lossy().to_string()).await.unwrap();
        
        let rs_path = temp_dir.join("main.rs");
        fs::write(&rs_path, "
use std::collections::HashMap;

struct AppServer {
    port: u16,
}

impl AppServer {
    fn start(&self) {
        let map: HashMap<String, String> = HashMap::new();
        self.log_startup();
    }

    fn log_startup(&self) {
        println!(\"Started\");
    }
}
").unwrap();

        let res = index_local_codebase(temp_dir.to_string_lossy().to_string(), db_client.db()).await;
        let msg = res.unwrap();
        println!("{}", msg);

        let mut f_res = db_client.db().query("SELECT * FROM func").await.unwrap();
        let funcs: Vec<Value> = f_res.take(0).unwrap();
        assert_eq!(funcs.len(), 2, "Expected 2 functions (start, log_startup), got: {:?}", funcs);

        let mut c_res = db_client.db().query("SELECT * FROM class").await.unwrap();
        let classes: Vec<Value> = c_res.take(0).unwrap();
        assert_eq!(classes.len(), 1, "Expected 1 class/struct (AppServer)");
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[tokio::test]
    async fn test_index_cgc_codebase() {
        let temp_dir = std::env::temp_dir().join("surreal_mcp_test_cgc_full");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        let db_client = SurrealClient::connect(temp_dir.join("db").to_string_lossy().to_string()).await.unwrap();
        
        let path = "/Users/tmorton/projects/cgc_surreal".to_string();
        println!("Indexing Codebase: {}", path);
        
        let res = index_local_codebase(path, db_client.db()).await;
        println!("Indexing Result: {:?}", res);
        
        // Output stats
        let mut f_res = db_client.db().query("SELECT count() FROM func GROUP BY all").await.unwrap();
        let funcs: Vec<Value> = f_res.take(0).unwrap();
        println!("Total Functions found: {:?}", funcs);

        let mut c_res = db_client.db().query("SELECT count() FROM class GROUP BY all").await.unwrap();
        let classes: Vec<Value> = c_res.take(0).unwrap();
        println!("Total Classes found: {:?}", classes);
    }
}

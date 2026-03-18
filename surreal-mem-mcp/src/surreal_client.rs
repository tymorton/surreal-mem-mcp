use chrono::Utc;
use serde_json::{Value, json};
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};
use uuid::Uuid;

pub struct SurrealClient {
    db: Arc<Surreal<Db>>,
}

impl SurrealClient {
    pub fn db(&self) -> Arc<Surreal<Db>> {
        self.db.clone()
    }
    pub async fn connect(db_path: String) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Surreal::new::<RocksDb>(db_path).await?;
        db.use_ns("surreal_mcp").use_db("memory").await?;

        // Initialize Analyzer and Index for BM25
        db.query("DEFINE ANALYZER puppy_analyzer TOKENIZERS blank,class,camel,punct FILTERS lowercase,snowball(english);")
            .await?.check()?;

        db.query(
            "DEFINE INDEX fts_content ON memory FIELDS text SEARCH ANALYZER puppy_analyzer BM25;",
        )
        .await?
        .check()?;

        db.query(
            "DEFINE INDEX fts_headline ON memory FIELDS headline SEARCH ANALYZER puppy_analyzer BM25;",
        )
        .await?
        .check()?;

        Ok(Self { db: Arc::new(db) })
    }

    pub async fn remember(
        &self,
        text: &str,
        headline: Option<&str>,
        embedding: Option<Vec<f32>>,
        headline_embedding: Option<Vec<f32>>,
        metadata: Value,
        scope: &str,
        agent_id: Option<&str>,
        session_id: Option<&str>,
        author_agent_id: Option<&str>,
        ttl_days: Option<u32>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        // Compute expires_at if ttl_days is provided
        let expires_at: Option<String> = ttl_days.map(|days| {
            (Utc::now() + chrono::Duration::days(days as i64)).to_rfc3339()
        });

        let mut data = json!({
            "text": text,
            "headline": headline.unwrap_or(""),
            "created_at": now,
            "accessed_at": now,
            "access_count": 0,
            "status": "active",
            "scope": scope,
            "agent_id": agent_id,
            "session_id": session_id,
            "author_agent_id": author_agent_id,
            "expires_at": expires_at,
            "metadata": metadata
        });

        if let Some(emb) = &embedding {
            data["embedding"] = json!(emb);
        }
        if let Some(h_emb) = &headline_embedding {
            data["headline_embedding"] = json!(h_emb);
        }

        self.db
            .query("CREATE type::thing('memory', $id) CONTENT $data")
            .bind(("id", id.clone()))
            .bind(("data", data))
            .await?
            .check()?;

        // Link to similar memories for Graph Belief Propagation
        if let Some(emb) = embedding {
            let scope_filter = match scope {
                "global" => "AND scope = 'global'",
                "agent" => "AND scope IN ['global', 'agent']",
                _ => "",
            };
            let related_q = format!(
                "SELECT id, vector::similarity::cosine(embedding, $query_emb) AS sim FROM memory WHERE id != type::thing('memory', $id) AND status = 'active' AND embedding IS NOT NONE {} ORDER BY sim DESC LIMIT 3",
                scope_filter
            );
            let mut related_res = self
                .db
                .query(&related_q)
                .bind(("id", id.clone()))
                .bind(("query_emb", emb))
                .await?;

            let similar_memories: Vec<Value> = related_res.take(0).unwrap_or_default();
            for mem in similar_memories {
                if let (Some(target_id), Some(sim)) = (mem.get("id"), mem.get("sim")) {
                    let relate_q =
                        "RELATE type::thing('memory', $id)->related_to->$target SET sim = $sim";
                    let _ = self
                        .db
                        .query(relate_q)
                        .bind(("id", id.clone()))
                        .bind(("target", target_id.clone()))
                        .bind(("sim", sim.clone()))
                        .await?;
                }
            }
        }

        // Phase 6: Orphaned Session TTL Cleanup + Global/Agent TTL Eviction.
        // Passively garbage collect:
        //   (a) Ephemeral sessions older than 24 hours (crash/abandoned agents)
        //   (b) Any memory whose expires_at has passed (opt-in global/agent TTL)
        let _ = self.db
            .query("
                DELETE memory WHERE scope = 'session' AND time::unix(time::now()) - time::unix(type::datetime(created_at)) > 86400;
                DELETE memory WHERE expires_at IS NOT NONE AND type::datetime(expires_at) < time::now();
            ")
            .await;

        Ok(id)
    }

    pub async fn bayesian_search(
        &self,
        query: &str,
        query_emb: Option<Vec<f32>>,
        limit: usize,
        _scope: &str,
        agent_id: Option<&str>,
        session_id: Option<&str>,
        compressed: bool,
    ) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let mut q = String::new();
        // When compressed=true, return the headline (1-2 sentence summary) instead of the
        // full memory text. This is the lossless context memory mode — precise signal,
        // lower token cost. The LLM can then call search_memory_graph for full fidelity
        // on the specific memories it needs to expand.
        let text_field = if compressed {
            "CASE WHEN headline != '' THEN headline ELSE text END AS text"
        } else {
            "text"
        };


        if query_emb.is_some() && !query.is_empty() {
            // Full Bayesian Query (Vector + BM25 Priors)
            q.push_str(&format!(r#"
            SELECT 
                id, 
                {text_field},
                headline,
                created_at,
                access_count,
                bm25_score,
                (vector::similarity::cosine(embedding, $query_emb) * 0.7 + bm25_score * 0.3) AS likelihood,
                (
                    math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                    * math::min([1.0, 0.1 + (related_count * 0.1)])
                    * (1.0 + (access_count * 0.1))
                ) AS prior_belief,
                ((vector::similarity::cosine(embedding, $query_emb) * 0.7 + bm25_score * 0.3) * (
                    math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                    * math::min([1.0, 0.1 + (related_count * 0.1)])
                    * (1.0 + (access_count * 0.1))
                )) AS final_posterior_score
            FROM (
                SELECT id, text, headline, embedding, created_at, access_count, type::number(count(->related_to)) AS related_count, search::score(1) AS bm25_score
                FROM memory 
                WHERE status = 'active' AND (scope = 'global' OR agent_id = $agent_id OR session_id = $session_id) AND embedding <|100|> $query_emb AND text @1@ $query
            )
            ORDER BY final_posterior_score DESC LIMIT $limit;
            "#, text_field = text_field));
        } else if query_emb.is_some() {
            // Vector Only Bayesian Query
            q.push_str(&format!(r#"
            SELECT 
                id, 
                {text_field},
                headline,
                created_at,
                access_count,
                (vector::similarity::cosine(embedding, $query_emb)) AS likelihood,
                (
                    math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                    * math::min([1.0, 0.1 + (related_count * 0.1)])
                    * (1.0 + (access_count * 0.1))
                ) AS prior_belief,
                (vector::similarity::cosine(embedding, $query_emb) * (
                    math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                    * math::min([1.0, 0.1 + (related_count * 0.1)])
                    * (1.0 + (access_count * 0.1))
                )) AS final_posterior_score
            FROM (
                SELECT id, text, headline, embedding, created_at, access_count, type::number(count(->related_to)) AS related_count
                FROM memory
                WHERE status = 'active' AND (scope = 'global' OR agent_id = $agent_id OR session_id = $session_id) AND embedding <|100|> $query_emb
            )
            ORDER BY final_posterior_score DESC LIMIT $limit;
            "#, text_field = text_field));
        } else {
            // BM25 Only Search
            q.push_str(&format!(r#"
            SELECT 
                id, 
                {text_field},
                headline,
                created_at,
                access_count,
                bm25_score AS likelihood,
                (
                    math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                    * math::min([1.0, 0.1 + (related_count * 0.1)])
                    * (1.0 + (access_count * 0.1))
                ) AS prior_belief,
                (bm25_score * (
                    math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                    * math::min([1.0, 0.1 + (related_count * 0.1)])
                    * (1.0 + (access_count * 0.1))
                )) AS final_posterior_score
            FROM (
                SELECT id, text, headline, created_at, access_count, type::number(count(->related_to)) AS related_count, search::score(1) AS bm25_score
                FROM memory
                WHERE status = 'active' AND (scope = 'global' OR agent_id = $agent_id OR session_id = $session_id) AND text @1@ $query
                LIMIT 100
            )
            ORDER BY final_posterior_score DESC LIMIT $limit;
            "#, text_field = text_field));
        }

        let mut stmt = self.db.query(&q).bind(("limit", limit));
        if !query.is_empty() {
            stmt = stmt.bind(("query", query));
        }
        if let Some(emb) = query_emb {
            stmt = stmt.bind(("query_emb", emb));
        }

        // Bind scope parameters securely
        let a_id = agent_id.unwrap_or("__NONE__");
        let s_id = session_id.unwrap_or("__NONE__");
        stmt = stmt.bind(("agent_id", a_id)).bind(("session_id", s_id));

        let mut res = stmt.await?;
        let chunks: Vec<Value> = res.take(0)?;

        // Entropy Logic (Epistemic Uncertainty)
        if !chunks.is_empty() && chunks.len() > 1 {
            let max_score = chunks[0]
                .get("final_posterior_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let min_score = chunks
                .last()
                .unwrap()
                .get("final_posterior_score")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            // Basic Entropy analog using range over max
            if max_score > 0.0 && (max_score - min_score) / max_score < 0.1 {
                // High Uncertainty
                return Ok(vec![json!({
                    "system_warning": "High contextual uncertainty detected. Current memory bounds are insufficient. Execute a broader exploratory search or ask the user a clarifying question to reduce hypothesis space.",
                    "entropy_metrics": {
                        "max_score": max_score,
                        "min_score": min_score
                    }
                })]);
            }
        }

        // Increment Access Count asynchronously
        let db_clone = self.db.clone();
        let chunks_clone = chunks.clone();
        tokio::spawn(async move {
            for chunk in chunks_clone {
                if let Some(id_val) = chunk.get("id") {
                    let update_q =
                        "UPDATE $id SET access_count = access_count + 1, accessed_at = time::now()";
                    let _ = db_clone.query(update_q).bind(("id", id_val.clone())).await;
                }
            }
        });

        Ok(chunks)
    }

    pub async fn bayesian_graph_search(
        &self,
        query: &str,
        query_emb: Option<Vec<f32>>,
        max_depth: usize,
        _scope: &str,
        agent_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let mut q = String::new();

        if query_emb.is_some() && !query.is_empty() {
            // Full Bayesian Query (Vector + BM25 Priors) to find the root node
            q.push_str(r#"
            SELECT 
                id
            FROM (
                SELECT id, text, embedding, created_at, access_count, type::number(count(->related_to)) AS related_count, search::score(1) AS bm25_score
                FROM memory 
                WHERE status = 'active' AND (scope = 'global' OR agent_id = $agent_id OR session_id = $session_id) AND embedding <|100|> $query_emb AND text @1@ $query
            )
            ORDER BY ((vector::similarity::cosine(embedding, $query_emb) * 0.7 + bm25_score * 0.3) * (
                math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                * math::min([1.0, 0.1 + (related_count * 0.1)])
                * (1.0 + (access_count * 0.1))
            )) DESC LIMIT 1;
            "#);
        } else if query_emb.is_some() {
            // Vector Only to find the root node
            q.push_str(r#"
            SELECT 
                id
            FROM (
                SELECT id, text, embedding, created_at, access_count, type::number(count(->related_to)) AS related_count
                FROM memory
                WHERE status = 'active' AND (scope = 'global' OR agent_id = $agent_id OR session_id = $session_id) AND embedding <|100|> $query_emb
            )
            ORDER BY (vector::similarity::cosine(embedding, $query_emb) * (
                math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                * math::min([1.0, 0.1 + (related_count * 0.1)])
                * (1.0 + (access_count * 0.1))
            )) DESC LIMIT 1;
            "#);
        } else {
            // BM25 Only to find the root node
            q.push_str(r#"
            SELECT 
                id
            FROM (
                SELECT id, text, created_at, access_count, type::number(count(->related_to)) AS related_count, search::score(1) AS bm25_score
                FROM memory
                WHERE status = 'active' AND (scope = 'global' OR agent_id = $agent_id OR session_id = $session_id) AND text @1@ $query
                LIMIT 100
            )
            ORDER BY (bm25_score * (
                math::pow(0.99, type::number(time::unix(time::now()) - time::unix(type::datetime(created_at))) / 86400.0) 
                * math::min([1.0, 0.1 + (related_count * 0.1)])
                * (1.0 + (access_count * 0.1))
            )) DESC LIMIT 1;
            "#);
        }

        let mut stmt = self.db.query(&q);
        if !query.is_empty() {
            stmt = stmt.bind(("query", query));
        }
        if let Some(emb) = query_emb {
            stmt = stmt.bind(("query_emb", emb));
        }

        let a_id = agent_id.unwrap_or("__NONE__");
        let s_id = session_id.unwrap_or("__NONE__");
        stmt = stmt.bind(("agent_id", a_id)).bind(("session_id", s_id));

        let mut res = stmt.await?;
        let mut roots: Vec<Value> = res.take(0)?;

        if roots.is_empty() {
            return Ok(vec![]);
        }

        let root_id = roots.pop().unwrap().get("id").unwrap().clone();

        // Dynamically build the graph traversal string based on max_depth
        let mut traversal_path = String::new();
        for _ in 0..max_depth {
            traversal_path.push_str("->related_to->memory");
        }

        // e.g SELECT array::distinct((->related_to->memory->related_to->memory) ?? []) AS graph_nodes FROM $root_id FETCH graph_nodes
        let traverse_q = format!(
            "SELECT array::distinct(({}) ?? []) AS graph_nodes FROM $root_id FETCH graph_nodes",
            traversal_path
        );

        let mut traverse_res = self
            .db
            .query(&traverse_q)
            .bind(("root_id", root_id))
            .await?;
        let mut traverse_data: Vec<Value> = traverse_res.take(0)?;

        if traverse_data.is_empty() {
            return Ok(vec![]);
        }

        let graph_nodes_val = traverse_data
            .pop()
            .unwrap()
            .get("graph_nodes")
            .unwrap_or(&json!([]))
            .clone();

        if let Some(nodes_arr) = graph_nodes_val.as_array() {
            let serialized = serde_json::to_string(nodes_arr).unwrap_or_default();
            if serialized.len() > 100_000 {
                let mut safe_string = serialized[..100_000].to_string();
                safe_string.push_str(r#"... [TRUNCATED_TO_PRESERVE_TOKEN_WINDOW]"}]"#);
                
                return Ok(vec![json!({
                    "_SYSTEM_WARNING": "The knowledge graph dependency tree was too dense. Output truncated at 100,000 characters to prevent crashing your LLM Token Context Window. Proceed with the data available.",
                    "tree": safe_string
                })]);
            }
            Ok(nodes_arr.clone())
        } else {
            Ok(vec![])
        }
    }

    pub async fn end_session(&self, session_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.db
            .query("DELETE memory WHERE scope = 'session' AND session_id = $session_id")
            .bind(("session_id", session_id))
            .await?
            .check()?;
        Ok(())
    }

    pub async fn promote_memory(
        &self,
        memory_id: &str,
        target_scope: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let q = "UPDATE type::thing($memory_id) SET scope = $target_scope";
        self.db
            .query(q)
            .bind(("memory_id", memory_id))
            .bind(("target_scope", target_scope))
            .await?
            .check()?;
        Ok(())
    }
}

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op")]
pub enum Request {
    Insert { data: Value },
    BatchInsert { data: Vec<Value> },
    GetById { id: String, hydrate: Option<bool> },
    GetByType {
        r#type: String,
        hydrate: Option<bool>,
        limit: Option<usize>,
        offset: Option<usize>,
        sort_by: Option<String>,
        sort_desc: Option<bool>,
    },
    Query {
        r#type: String,
        filters: HashMap<String, Value>,
        hydrate: Option<bool>,
        limit: Option<usize>,
        offset: Option<usize>,
        sort_by: Option<String>,
        sort_desc: Option<bool>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status")]
pub enum Response {
    Ok { data: Option<Value> },
    Error { message: String },
}

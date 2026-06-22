use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op")]
pub enum Request {
    Insert { data: Value },
    GetById { id: String },
    GetByType { r#type: String },
    Query { r#type: String, filters: HashMap<String, Value> },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "status")]
pub enum Response {
    Ok { data: Option<Value> },
    Error { message: String },
}

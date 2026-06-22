use sled::Db;
use serde_json::{Value, Map};
use std::collections::HashMap;
use std::sync::Arc;
use crate::definition_parser::SchemaDefinitions;

pub struct Storage {
    db: Arc<Db>,
    definitions: Arc<SchemaDefinitions>,
}

impl Storage {
    pub fn new(path: &str, definitions: Arc<SchemaDefinitions>) -> Result<Self, Box<dyn std::error::Error>> {
        let db = sled::open(path)?;
        Ok(Storage { db: Arc::new(db), definitions })
    }

    pub fn compress(&self, value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut new_map = Map::new();
                for (k, v) in map {
                    let key = if let Some(id) = self.definitions.get_id(k) {
                        id.to_string()
                    } else {
                        k.clone()
                    };
                    new_map.insert(key, self.compress(v));
                }
                Value::Object(new_map)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| self.compress(v)).collect())
            }
            _ => value.clone(),
        }
    }

    pub fn decompress(&self, value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut new_map = Map::new();
                for (k, v) in map {
                    let key = if let Ok(id) = k.parse::<u32>() {
                        if let Some(term) = self.definitions.get_term(id) {
                            term.clone()
                        } else {
                            k.clone()
                        }
                    } else {
                        k.clone()
                    };
                    new_map.insert(key, self.decompress(v));
                }
                Value::Object(new_map)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| self.decompress(v)).collect())
            }
            _ => value.clone(),
        }
    }

    pub fn insert(&self, data: &Value) -> Result<String, Box<dyn std::error::Error>> {
        let mut data = data.clone();
        let id = if let Some(id) = data.get("@id").and_then(|id| id.as_str()) {
            id.to_string()
        } else {
            let new_id = format!("https://schema.org/id/{}", uuid::Uuid::new_v4());
            if let Some(obj) = data.as_object_mut() {
                obj.insert("@id".to_string(), Value::String(new_id.clone()));
            }
            new_id
        };

        let compressed = self.compress(&data);
        let bytes = serde_json::to_vec(&compressed)?;
        self.db.insert(id.as_bytes(), bytes)?;

        // Index by @type
        if let Some(typ) = data.get("@type") {
            let types = if let Some(t) = typ.as_str() {
                vec![t]
            } else if let Some(arr) = typ.as_array() {
                arr.iter().filter_map(|v| v.as_str()).collect()
            } else {
                vec![]
            };

            for t in types {
                let type_tree = self.db.open_tree(format!("type:{}", t))?;
                type_tree.insert(id.as_bytes(), b"")?;
            }
        }

        Ok(id)
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<Value>, Box<dyn std::error::Error>> {
        if let Some(bytes) = self.db.get(id.as_bytes())? {
            let compressed: Value = serde_json::from_slice(&bytes)?;
            Ok(Some(self.decompress(&compressed)))
        } else {
            Ok(None)
        }
    }

    pub fn get_by_type(&self, typ: &str) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let type_tree = self.db.open_tree(format!("type:{}", typ))?;
        let mut results = Vec::new();
        for item in type_tree.iter() {
            let (id, _) = item?;
            if let Some(val) = self.get_by_id(std::str::from_utf8(&id)?)? {
                results.push(val);
            }
        }
        Ok(results)
    }

    pub fn query(&self, typ: &str, filters: &HashMap<String, Value>) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let type_tree = self.db.open_tree(format!("type:{}", typ))?;
        let mut results = Vec::new();

        // Convert filters to compressed form for faster comparison
        let mut compressed_filters = HashMap::new();
        for (k, v) in filters {
            let key = if let Some(id) = self.definitions.get_id(k) {
                id.to_string()
            } else {
                k.clone()
            };
            compressed_filters.insert(key, self.compress(v));
        }

        for item in type_tree.iter() {
            let (id, _) = item?;
            if let Some(bytes) = self.db.get(&id)? {
                let compressed: Value = serde_json::from_slice(&bytes)?;
                let obj = compressed.as_object().unwrap();

                let mut matches = true;
                for (k, v) in &compressed_filters {
                    if let Some(val) = obj.get(k) {
                        if val != v {
                            matches = false;
                            break;
                        }
                    } else {
                        matches = false;
                        break;
                    }
                }

                if matches {
                    results.push(self.decompress(&compressed));
                }
            }
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition_parser::SchemaDefinitions;
    use tempfile::tempdir;

    #[test]
    fn test_storage_compression() {
        let json_ld = r#"{
            "@graph": [
                { "@id": "https://schema.org/Person", "@type": "rdfs:Class" },
                { "@id": "https://schema.org/name", "@type": "rdf:Property" }
            ]
        }"#;
        let defs = Arc::new(SchemaDefinitions::parse(json_ld).unwrap());
        let dir = tempdir().unwrap();
        let storage = Storage::new(dir.path().to_str().unwrap(), defs.clone()).unwrap();

        let data = serde_json::json!({
            "@type": "Person",
            "name": "Jane Doe"
        });

        let id = storage.insert(&data).unwrap();

        // Verify compressed data in DB
        let raw_bytes = storage.db.get(id.as_bytes()).unwrap().unwrap();
        let compressed: Value = serde_json::from_slice(&raw_bytes).unwrap();

        let _name_id = defs.get_id("name").unwrap().to_string();
        let type_id = defs.get_id("@type").unwrap().to_string();

        assert!(compressed.as_object().unwrap().contains_key(&type_id));

        // Verify decompression
        let retrieved = storage.get_by_id(&id).unwrap().unwrap();
        assert_eq!(retrieved["name"], "Jane Doe");
        assert_eq!(retrieved["@type"], "Person");
    }

    #[test]
    fn test_query() {
        let json_ld = r#"{
            "@graph": [
                { "@id": "https://schema.org/Person", "@type": "rdfs:Class" },
                { "@id": "https://schema.org/name", "@type": "rdf:Property" },
                { "@id": "https://schema.org/jobTitle", "@type": "rdf:Property" }
            ]
        }"#;
        let defs = Arc::new(SchemaDefinitions::parse(json_ld).unwrap());
        let dir = tempdir().unwrap();
        let storage = Storage::new(dir.path().to_str().unwrap(), defs.clone()).unwrap();

        storage.insert(&serde_json::json!({ "@type": "Person", "name": "Alice", "jobTitle": "Engineer" })).unwrap();
        storage.insert(&serde_json::json!({ "@type": "Person", "name": "Bob", "jobTitle": "Designer" })).unwrap();

        let mut filters = HashMap::new();
        filters.insert("name".to_string(), serde_json::Value::String("Alice".to_string()));

        let results = storage.query("Person", &filters).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], "Alice");
    }
}

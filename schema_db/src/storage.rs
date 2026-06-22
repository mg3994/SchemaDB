use sled::Db;
use serde_json::{Value, Map};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use crate::definition_parser::SchemaDefinitions;
use lru::LruCache;
use std::num::NonZeroUsize;

pub struct Storage {
    db: Arc<Db>,
    definitions: Arc<SchemaDefinitions>,
    cache: Mutex<LruCache<String, Value>>,
}

impl Storage {
    pub fn new(path: &str, definitions: Arc<SchemaDefinitions>) -> Result<Self, Box<dyn std::error::Error>> {
        let db = sled::open(path)?;
        let cache = Mutex::new(LruCache::new(NonZeroUsize::new(1000).unwrap()));

        let defs_tree = db.open_tree("internal:definitions")?;
        if defs_tree.is_empty() {
            for (term, id) in &definitions.term_to_id {
                defs_tree.insert(term.as_bytes(), &id.to_be_bytes())?;
            }
        }

        Ok(Storage { db: Arc::new(db), definitions, cache })
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

    pub fn hydrate(&self, value: &mut Value, seen: &mut HashSet<String>) {
        match value {
            Value::Object(map) => {
                let mut hydrated_entries = Vec::new();
                for (k, v) in map.iter_mut() {
                    if k != "@id" {
                        if let Some(id_str) = v.as_str() {
                            if id_str.starts_with("https://schema.org/id/") {
                                if !seen.contains(id_str) {
                                    seen.insert(id_str.to_string());
                                    if let Ok(Some(linked_val)) = self.get_by_id(id_str) {
                                        let mut linked_val = linked_val;
                                        self.hydrate(&mut linked_val, seen);
                                        hydrated_entries.push((k.clone(), linked_val));
                                    }
                                }
                            }
                        } else {
                            self.hydrate(v, seen);
                        }
                    }
                }
                for (k, v) in hydrated_entries {
                    map.insert(k, v);
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    self.hydrate(v, seen);
                }
            }
            _ => {}
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
        // Using Bincode for super speed binary internal storage
        let bytes = bincode::serialize(&compressed)?;
        self.db.insert(id.as_bytes(), bytes)?;

        {
            let mut cache = self.cache.lock().unwrap();
            cache.put(id.clone(), data.clone());
        }

        if let Some(obj) = data.as_object() {
            for (key, val) in obj {
                let index_name = if key == "@type" { "type".to_string() } else { format!("idx:{}", key) };
                let idx_tree = self.db.open_tree(&index_name)?;

                let vals = if let Some(arr) = val.as_array() { arr.clone() } else { vec![val.clone()] };

                for v in vals {
                    if v.is_string() || v.is_number() || v.is_boolean() {
                        let val_str = match v {
                            Value::String(s) => s.clone(),
                            _ => v.to_string(),
                        };
                        let mut idx_key = val_str.as_bytes().to_vec();
                        idx_key.push(0);
                        idx_key.extend_from_slice(id.as_bytes());
                        idx_tree.insert(idx_key, b"")?;

                        let card_tree = self.db.open_tree("internal:cardinality")?;
                        card_tree.fetch_and_update(index_name.as_bytes(), |old| {
                            let count = old.map(|b| {
                                let mut arr = [0u8; 8];
                                arr.copy_from_slice(&b);
                                u64::from_be_bytes(arr)
                            }).unwrap_or(0);
                            Some((count + 1).to_be_bytes().to_vec())
                        })?;
                    }
                }
            }
        }

        Ok(id)
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<Value>, Box<dyn std::error::Error>> {
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(val) = cache.get(id) {
                return Ok(Some(val.clone()));
            }
        }

        if let Some(bytes) = self.db.get(id.as_bytes())? {
            // Deserialize from Bincode
            let compressed: Value = bincode::deserialize(&bytes)?;
            let decompressed = self.decompress(&compressed);
            let mut cache = self.cache.lock().unwrap();
            cache.put(id.to_string(), decompressed.clone());
            Ok(Some(decompressed))
        } else {
            Ok(None)
        }
    }

    pub fn get_by_type(&self, typ: &str) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        self.get_by_type_advanced(typ, None, None, None, false)
    }

    pub fn get_by_type_advanced(&self, typ: &str, limit: Option<usize>, offset: Option<usize>, sort_by: Option<&str>, sort_desc: bool) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        let type_tree = self.db.open_tree("type")?;
        let mut results = Vec::new();
        let mut prefix = typ.as_bytes().to_vec();
        prefix.push(0);
        for item in type_tree.scan_prefix(&prefix) {
            let (key, _) = item?;
            let id_bytes = &key[prefix.len()..];
            if let Some(val) = self.get_by_id(std::str::from_utf8(id_bytes)?)? {
                results.push(val);
            }
        }

        self.apply_sorting_and_pagination(&mut results, limit, offset, sort_by, sort_desc);
        Ok(results)
    }

    pub fn query_advanced(&self, typ: &str, filters: &HashMap<String, Value>, limit: Option<usize>, offset: Option<usize>, sort_by: Option<&str>, sort_desc: bool) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        if !filters.is_empty() {
            let card_tree = self.db.open_tree("internal:cardinality")?;
            let mut best_filter = None;
            let mut min_card = u64::MAX;

            for (key, val) in filters {
                if val.is_string() || val.is_number() || val.is_boolean() {
                    let index_name = format!("idx:{}", key);
                    let card = card_tree.get(&index_name)?.map(|b| {
                        let mut arr = [0u8; 8];
                        arr.copy_from_slice(&b);
                        u64::from_be_bytes(arr)
                    }).unwrap_or(u64::MAX);
                    if card < min_card {
                        min_card = card;
                        best_filter = Some((key, val));
                    }
                }
            }

            if let Some((key, val)) = best_filter {
                let val_str = match val { Value::String(s) => s.clone(), _ => val.to_string() };
                let idx_tree = self.db.open_tree(format!("idx:{}", key))?;
                let mut results = Vec::new();
                let mut prefix = val_str.as_bytes().to_vec();
                prefix.push(0);
                for item in idx_tree.scan_prefix(&prefix) {
                    let (idx_key, _) = item?;
                    let id_bytes = &idx_key[prefix.len()..];
                    if let Some(val) = self.get_by_id(std::str::from_utf8(id_bytes)?)? {
                        let mut matches = true;
                        if let Some(obj) = val.as_object() {
                            if let Some(typ_val) = obj.get("@type") {
                                let types = if let Some(t) = typ_val.as_str() { vec![t] } else if let Some(arr) = typ_val.as_array() { arr.iter().filter_map(|v| v.as_str()).collect() } else { vec![] };
                                if !types.contains(&typ) { matches = false; }
                            } else { matches = false; }

                            if matches {
                                for (k, v) in filters {
                                    if obj.get(k) != Some(v) { matches = false; break; }
                                }
                            }
                        } else { matches = false; }

                        if matches { results.push(val); }
                    }
                }
                self.apply_sorting_and_pagination(&mut results, limit, offset, sort_by, sort_desc);
                return Ok(results);
            }
        }

        let results = self.get_by_type_advanced(typ, None, None, None, false)?;
        let filtered: Vec<Value> = results.into_iter().filter(|val| {
            let obj = val.as_object().unwrap();
            filters.iter().all(|(k, v)| obj.get(k) == Some(v))
        }).collect();

        let mut final_results = filtered;
        self.apply_sorting_and_pagination(&mut final_results, limit, offset, sort_by, sort_desc);
        Ok(final_results)
    }

    fn apply_sorting_and_pagination(&self, results: &mut Vec<Value>, limit: Option<usize>, offset: Option<usize>, sort_by: Option<&str>, sort_desc: bool) {
        if let Some(field) = sort_by {
            results.sort_by(|a, b| {
                let va = a.get(field).unwrap_or(&Value::Null);
                let vb = b.get(field).unwrap_or(&Value::Null);
                let res = va.to_string().cmp(&vb.to_string());
                if sort_desc { res.reverse() } else { res }
            });
        }

        if let Some(off) = offset {
            if off < results.len() {
                *results = results.split_off(off);
            } else {
                results.clear();
            }
        }

        if let Some(lim) = limit {
            results.truncate(lim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition_parser::SchemaDefinitions;
    use tempfile::tempdir;

    #[test]
    fn test_exact_type_match() {
        let json_ld = r#"{ "@graph": [{ "@id": "https://schema.org/Person" }, { "@id": "https://schema.org/PersonalEvent" }] }"#;
        let defs = Arc::new(SchemaDefinitions::parse(json_ld).unwrap());
        let dir = tempdir().unwrap();
        let storage = Storage::new(dir.path().to_str().unwrap(), defs.clone()).unwrap();

        storage.insert(&serde_json::json!({ "@type": "Person", "name": "Alice" })).unwrap();
        storage.insert(&serde_json::json!({ "@type": "PersonalEvent", "name": "Party" })).unwrap();

        let results = storage.get_by_type("Person").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], "Alice");
    }

    #[test]
    fn test_advanced_query() {
        let json_ld = r#"{ "@graph": [{ "@id": "https://schema.org/Person" }, { "@id": "https://schema.org/name" }] }"#;
        let defs = Arc::new(SchemaDefinitions::parse(json_ld).unwrap());
        let dir = tempdir().unwrap();
        let storage = Storage::new(dir.path().to_str().unwrap(), defs.clone()).unwrap();

        storage.insert(&serde_json::json!({ "@type": "Person", "name": "C", "age": 30 })).unwrap();
        storage.insert(&serde_json::json!({ "@type": "Person", "name": "A", "age": 10 })).unwrap();
        storage.insert(&serde_json::json!({ "@type": "Person", "name": "B", "age": 20 })).unwrap();

        let results = storage.get_by_type_advanced("Person", Some(2), Some(0), Some("name"), false).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["name"], "A");
        assert_eq!(results[1]["name"], "B");
    }

    #[test]
    fn test_hydration_cycles() {
        let json_ld = r#"{ "@graph": [{ "@id": "https://schema.org/Person" }, { "@id": "https://schema.org/knows" }] }"#;
        let defs = Arc::new(SchemaDefinitions::parse(json_ld).unwrap());
        let dir = tempdir().unwrap();
        let storage = Storage::new(dir.path().to_str().unwrap(), defs.clone()).unwrap();

        let alice_id = "https://schema.org/id/alice";
        let bob_id = "https://schema.org/id/bob";

        storage.insert(&serde_json::json!({ "@id": alice_id, "@type": "Person", "name": "Alice", "knows": bob_id })).unwrap();
        storage.insert(&serde_json::json!({ "@id": bob_id, "@type": "Person", "name": "Bob", "knows": alice_id })).unwrap();

        let mut alice = storage.get_by_id(alice_id).unwrap().unwrap();
        let mut seen = HashSet::new();
        seen.insert(alice_id.to_string());
        storage.hydrate(&mut alice, &mut seen);

        assert_eq!(alice["knows"]["name"], "Bob");
        assert_eq!(alice["knows"]["knows"], alice_id);
    }
}

use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct SchemaDefinitions {
    pub terms: HashSet<String>,
    pub term_to_id: HashMap<String, u32>,
    pub id_to_term: HashMap<u32, String>,
}

impl SchemaDefinitions {
    pub fn parse(json_ld_str: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let root: Value = serde_json::from_str(json_ld_str)?;
        let mut terms = HashSet::new();

        if let Some(graph) = root.get("@graph").and_then(|g| g.as_array()) {
            for item in graph {
                if let Some(id) = item.get("@id").and_then(|i| i.as_str()) {
                    if id.starts_with("https://schema.org/") {
                        let term = id.trim_start_matches("https://schema.org/").to_string();
                        if !term.is_empty() {
                            terms.insert(term);
                        }
                    }
                }
            }
        }

        // Standard JSON-LD keywords
        terms.insert("@type".to_string());
        terms.insert("@id".to_string());
        terms.insert("@context".to_string());
        terms.insert("@value".to_string());
        terms.insert("@language".to_string());

        let mut sorted_terms: Vec<_> = terms.iter().cloned().collect();
        sorted_terms.sort();

        let mut term_to_id = HashMap::new();
        let mut id_to_term = HashMap::new();

        for (id, term) in sorted_terms.into_iter().enumerate() {
            let id = id as u32;
            term_to_id.insert(term.clone(), id);
            id_to_term.insert(id, term);
        }

        Ok(SchemaDefinitions {
            terms,
            term_to_id,
            id_to_term,
        })
    }

    pub fn is_valid_term(&self, term: &str) -> bool {
        self.terms.contains(term)
    }

    pub fn get_id(&self, term: &str) -> Option<u32> {
        self.term_to_id.get(term).copied()
    }

    pub fn get_term(&self, id: u32) -> Option<&String> {
        self.id_to_term.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_and_mapping() {
        let json_ld = r#"{
            "@graph": [
                { "@id": "https://schema.org/Person", "@type": "rdfs:Class" },
                { "@id": "https://schema.org/name", "@type": "rdf:Property" }
            ]
        }"#;
        let defs = SchemaDefinitions::parse(json_ld).unwrap();
        assert!(defs.is_valid_term("Person"));
        assert!(defs.is_valid_term("name"));
        assert!(defs.is_valid_term("@type"));

        let person_id = defs.get_id("Person").expect("Person should have ID");
        let term = defs.get_term(person_id).expect("ID should map back to Person");
        assert_eq!(term, "Person");
    }
}

use serde_json::Value;
use crate::definition_parser::SchemaDefinitions;

pub struct SchemaValidator<'a> {
    definitions: &'a SchemaDefinitions,
}

impl<'a> SchemaValidator<'a> {
    pub fn new(definitions: &'a SchemaDefinitions) -> Self {
        SchemaValidator { definitions }
    }

    pub fn validate(&self, value: &Value) -> Result<(), String> {
        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    if !self.definitions.is_valid_term(key) {
                        return Err(format!("Invalid field name: {}", key));
                    }
                    self.validate(val)?;
                }
                Ok(())
            }
            Value::Array(arr) => {
                for val in arr {
                    self.validate(val)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition_parser::SchemaDefinitions;

    #[test]
    fn test_validate_valid() {
        let json_ld = r#"{
            "@graph": [
                { "@id": "https://schema.org/Person", "@type": "rdfs:Class" },
                { "@id": "https://schema.org/name", "@type": "rdf:Property" }
            ]
        }"#;
        let defs = SchemaDefinitions::parse(json_ld).unwrap();
        let validator = SchemaValidator::new(&defs);

        let data = serde_json::json!({
            "@type": "Person",
            "name": "Jane Doe"
        });
        assert!(validator.validate(&data).is_ok());
    }

    #[test]
    fn test_validate_invalid() {
        let json_ld = r#"{
            "@graph": [
                { "@id": "https://schema.org/Person", "@type": "rdfs:Class" }
            ]
        }"#;
        let defs = SchemaDefinitions::parse(json_ld).unwrap();
        let validator = SchemaValidator::new(&defs);

        let data = serde_json::json!({
            "@type": "Person",
            "unknown_field": "error"
        });
        assert!(validator.validate(&data).is_err());
    }
}

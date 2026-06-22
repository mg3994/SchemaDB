use crate::definition_parser::SchemaDefinitions;
use crate::schema_validator::SchemaValidator;
use crate::storage::Storage;
use crate::protocol::{Request, Response};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_PAYLOAD_SIZE: usize = 10 * 1024 * 1024; // 10MB limit

pub async fn handle_client(stream: TcpStream, storage: Arc<Storage>, definitions: Arc<SchemaDefinitions>) {
    let (mut reader, mut writer) = stream.into_split();

    loop {
        let mut len_buf = [0u8; 4];
        if reader.read_exact(&mut len_buf).await.is_err() {
            break;
        }
        let len = u32::from_be_bytes(len_buf) as usize;

        if len > MAX_PAYLOAD_SIZE {
            let resp = Response::Error { message: "Payload too large".to_string() };
            let payload = serde_json::to_vec(&resp).unwrap();
            let len = payload.len() as u32;
            let _ = writer.write_all(&len.to_be_bytes()).await;
            let _ = writer.write_all(&payload).await;
            break;
        }

        let mut payload = vec![0u8; len];
        if reader.read_exact(&mut payload).await.is_err() {
            break;
        }

        let request: Request = match serde_json::from_slice(&payload) {
            Ok(req) => req,
            Err(e) => {
                let resp = Response::Error { message: format!("Invalid request: {}", e) };
                let payload = serde_json::to_vec(&resp).unwrap();
                let len = payload.len() as u32;
                let _ = writer.write_all(&len.to_be_bytes()).await;
                let _ = writer.write_all(&payload).await;
                continue;
            }
        };

        let response = match request {
            Request::Insert { data } => {
                let validator = SchemaValidator::new(&definitions);
                if let Err(e) = validator.validate(&data) {
                    Response::Error { message: e }
                } else {
                    match storage.insert(&data) {
                        Ok(id) => Response::Ok { data: Some(serde_json::Value::String(id)) },
                        Err(e) => Response::Error { message: e.to_string() },
                    }
                }
            }
            Request::GetById { id } => {
                match storage.get_by_id(&id) {
                    Ok(data) => Response::Ok { data },
                    Err(e) => Response::Error { message: e.to_string() },
                }
            }
            Request::GetByType { r#type } => {
                match storage.get_by_type(&r#type) {
                    Ok(items) => Response::Ok { data: Some(serde_json::Value::Array(items)) },
                    Err(e) => Response::Error { message: e.to_string() },
                }
            }
            Request::Query { r#type, filters } => {
                match storage.query(&r#type, &filters) {
                    Ok(items) => Response::Ok { data: Some(serde_json::Value::Array(items)) },
                    Err(e) => Response::Error { message: e.to_string() },
                }
            }
        };

        let payload = serde_json::to_vec(&response).unwrap();
        let len = payload.len() as u32;
        if writer.write_all(&len.to_be_bytes()).await.is_err() { break; }
        if writer.write_all(&payload).await.is_err() { break; }
    }
}

use schema_db::protocol::{Request, Response};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use schema_db::definition_parser::SchemaDefinitions;
use schema_db::storage::Storage;
use schema_db::server::handle_client;
use std::collections::HashMap;

#[tokio::test]
async fn test_server_integration() {
    let _ = std::fs::remove_dir_all("./test_server_data");

    let schema_json = r#"{
        "@graph": [
            { "@id": "https://schema.org/Person", "@type": "rdfs:Class" },
            { "@id": "https://schema.org/name", "@type": "rdf:Property" }
        ]
    }"#;

    let definitions = Arc::new(SchemaDefinitions::parse(schema_json).unwrap());
    let storage = Arc::new(Storage::new("./test_server_data", definitions.clone()).unwrap());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8900").await.unwrap();

    let storage_clone = storage.clone();
    let definitions_clone = definitions.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(handle_client(stream, storage_clone.clone(), definitions_clone.clone()));
        }
    });

    // Client
    let mut stream = TcpStream::connect("127.0.0.1:8900").await.unwrap();

    // Insert Alice
    let req = Request::Insert {
        data: serde_json::json!({
            "@type": "Person",
            "name": "Alice"
        }),
    };
    send_req(&mut stream, &req).await;
    let _resp_alice = recv_resp(&mut stream).await;

    // Insert Bob
    let req = Request::Insert {
        data: serde_json::json!({
            "@type": "Person",
            "name": "Bob"
        }),
    };
    send_req(&mut stream, &req).await;
    let _resp_bob = recv_resp(&mut stream).await;

    // Query for Alice
    let mut filters = HashMap::new();
    filters.insert("name".to_string(), serde_json::Value::String("Alice".to_string()));
    let req = Request::Query {
        r#type: "Person".to_string(),
        filters,
    };
    send_req(&mut stream, &req).await;
    let resp = recv_resp(&mut stream).await;

    if let Response::Ok { data: Some(serde_json::Value::Array(items)) } = resp {
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["name"], "Alice");
    } else {
        panic!("Expected Query Ok response, got {:?}", resp);
    }
}

async fn send_req(stream: &mut TcpStream, req: &Request) {
    let payload = serde_json::to_vec(req).unwrap();
    stream.write_all(&(payload.len() as u32).to_be_bytes()).await.unwrap();
    stream.write_all(&payload).await.unwrap();
}

async fn recv_resp(stream: &mut TcpStream) -> Response {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.unwrap();
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut resp_buf = vec![0u8; len];
    stream.read_exact(&mut resp_buf).await.unwrap();
    serde_json::from_slice(&resp_buf).unwrap()
}

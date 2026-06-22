use schema_db::definition_parser::SchemaDefinitions;
use schema_db::storage::Storage;
use schema_db::server::handle_client;
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cache_path = "schema_cache.json";
    let url = "https://schema.org/version/latest/schemaorg-current-https.jsonld";

    let definitions = Arc::new(SchemaDefinitions::fetch_or_load_cache(cache_path, url).await?);
    let storage = Arc::new(Storage::new("./schema_db_data", definitions.clone())?);

    let addr = "127.0.0.1:8888";
    let listener = TcpListener::bind(addr).await?;
    println!("SchemaDB listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;
        let storage = storage.clone();
        let definitions = definitions.clone();
        tokio::spawn(async move {
            handle_client(stream, storage, definitions).await;
        });
    }
}

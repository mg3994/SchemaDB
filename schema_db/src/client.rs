use crate::protocol::{Request, Response};
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct SchemaDbClient {
    stream: TcpStream,
}

impl SchemaDbClient {
    pub async fn connect(addr: &str) -> io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(SchemaDbClient { stream })
    }

    async fn send_request(&mut self, req: Request) -> io::Result<Response> {
        let payload = serde_json::to_vec(&req).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let len = payload.len() as u32;
        self.stream.write_all(&len.to_be_bytes()).await?;
        self.stream.write_all(&payload).await?;

        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut resp_buf = vec![0u8; len];
        self.stream.read_exact(&mut resp_buf).await?;

        let resp: Response = serde_json::from_slice(&resp_buf).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(resp)
    }

    pub async fn insert(&mut self, data: Value) -> io::Result<String> {
        match self.send_request(Request::Insert { data }).await? {
            Response::Ok { data: Some(Value::String(id)) } => Ok(id),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            _ => Err(io::Error::new(io::ErrorKind::Other, "Unexpected response")),
        }
    }

    pub async fn batch_insert(&mut self, data: Vec<Value>) -> io::Result<Vec<String>> {
        match self.send_request(Request::BatchInsert { data }).await? {
            Response::Ok { data: Some(Value::Array(ids)) } => {
                Ok(ids.into_iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            }
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            _ => Err(io::Error::new(io::ErrorKind::Other, "Unexpected response")),
        }
    }

    pub async fn get_by_id(&mut self, id: String, hydrate: bool) -> io::Result<Option<Value>> {
        match self.send_request(Request::GetById { id, hydrate: Some(hydrate) }).await? {
            Response::Ok { data } => Ok(data),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
        }
    }

    pub async fn get_by_type(
        &mut self,
        r#type: String,
        hydrate: bool,
        limit: Option<usize>,
        offset: Option<usize>,
        sort_by: Option<String>,
        sort_desc: Option<bool>,
    ) -> io::Result<Vec<Value>> {
        match self.send_request(Request::GetByType {
            r#type,
            hydrate: Some(hydrate),
            limit,
            offset,
            sort_by,
            sort_desc,
        }).await? {
            Response::Ok { data: Some(Value::Array(items)) } => Ok(items),
            Response::Ok { data: None } => Ok(vec![]),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            _ => Err(io::Error::new(io::ErrorKind::Other, "Unexpected response")),
        }
    }

    pub async fn query(
        &mut self,
        r#type: String,
        filters: HashMap<String, Value>,
        hydrate: bool,
        limit: Option<usize>,
        offset: Option<usize>,
        sort_by: Option<String>,
        sort_desc: Option<bool>,
    ) -> io::Result<Vec<Value>> {
        match self.send_request(Request::Query {
            r#type,
            filters,
            hydrate: Some(hydrate),
            limit,
            offset,
            sort_by,
            sort_desc,
        }).await? {
            Response::Ok { data: Some(Value::Array(items)) } => Ok(items),
            Response::Ok { data: None } => Ok(vec![]),
            Response::Error { message } => Err(io::Error::new(io::ErrorKind::Other, message)),
            _ => Err(io::Error::new(io::ErrorKind::Other, "Unexpected response")),
        }
    }
}

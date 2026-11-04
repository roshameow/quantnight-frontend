//! Lossless BSON only for explicitly declared Document fields. Never interpret opaque business JSON.
use bson::Document;
use serde::{Serialize,Deserialize,Serializer,Deserializer};
use serde_json::Value;
pub fn encode(document:&Document)->Result<Value,String> {
    serde_json::to_value(bson::to_vec(document).map_err(|_|"BSON document encoding failed")?).map_err(|_|"BSON wire encoding failed".into())
}
pub fn decode(value:Value)->Result<Document,String> {
    let bytes:Vec<u8>=serde_json::from_value(value).map_err(|_|"invalid BSON wire bytes")?;
    raw_decode(&bytes)
}
fn raw_decode(bytes:&[u8])->Result<Document,String>{
    bson::RawDocument::from_bytes(bytes).map_err(|_|"invalid BSON wire bytes")?.try_into().map_err(|_|"invalid BSON wire document".into())
}
pub mod optional {
    use super::*;
    pub fn serialize<S:Serializer>(value:&Option<Document>, serializer:S)->Result<S::Ok,S::Error>{
        let bytes=value.as_ref().map(bson::to_vec).transpose().map_err(serde::ser::Error::custom)?;
        bytes.serialize(serializer)
    }
    pub fn deserialize<'de,D:Deserializer<'de>>(deserializer:D)->Result<Option<Document>,D::Error>{
        Option::<Vec<u8>>::deserialize(deserializer)?.map(|v|raw_decode(&v).map_err(serde::de::Error::custom)).transpose()
    }
}

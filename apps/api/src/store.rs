use aws_sdk_dynamodb::{
    types::{AttributeValue, Put, TransactWriteItem},
    Client,
};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
#[error("{1}")]
pub struct ApiError(pub StatusCode, pub String);
pub type ApiResult<T> = Result<T, ApiError>;
impl ApiError {
    pub fn bad(s: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, s.into())
    }
    pub fn conflict(s: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, s.into())
    }
    pub fn unavailable(s: impl Into<String>) -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, s.into())
    }
    pub fn missing() -> Self {
        Self(StatusCode::NOT_FOUND, "Not found".into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({"error":self.1}))).into_response()
    }
}
fn internal(e: impl std::fmt::Debug) -> ApiError {
    tracing::error!(error=?e,"persistence operation failed");
    ApiError::unavailable("Persistence temporarily unavailable")
}
#[derive(Clone)]
pub struct Store {
    pub client: Client,
    pub table: String,
}
#[derive(Clone)]
pub struct Record<T> {
    pub value: T,
    pub version: u64,
}
pub struct Write {
    pub key: String,
    pub json: String,
    pub expected: Option<u64>,
}
impl Write {
    pub fn new(
        key: impl Into<String>,
        value: &impl Serialize,
        expected: Option<u64>,
    ) -> ApiResult<Self> {
        let key = key.into();
        let json = serde_json::to_string(value).map_err(internal)?;
        // Leave room below DynamoDB's 400 KiB item ceiling for keys/attributes.
        if json.len() + key.len() > 390_000 {
            return Err(ApiError::bad(
                "Record exceeds the immutable storage size limit",
            ));
        }
        Ok(Self {
            key,
            json,
            expected,
        })
    }
}
impl Store {
    fn keys(user: &str, key: &str) -> HashMap<String, AttributeValue> {
        HashMap::from([
            ("pk".into(), AttributeValue::S(format!("USER#{user}"))),
            ("sk".into(), AttributeValue::S(key.into())),
        ])
    }
    fn decode<T: DeserializeOwned>(item: HashMap<String, AttributeValue>) -> ApiResult<Record<T>> {
        let json = item
            .get("data")
            .and_then(|x| x.as_s().ok())
            .ok_or_else(|| ApiError::unavailable("Invalid stored record"))?;
        let version = item
            .get("version")
            .and_then(|x| x.as_n().ok())
            .and_then(|x| x.parse().ok())
            .ok_or_else(|| ApiError::unavailable("Invalid record version"))?;
        Ok(Record {
            value: serde_json::from_str(json).map_err(internal)?,
            version,
        })
    }
    pub async fn get<T: DeserializeOwned>(
        &self,
        user: &str,
        key: &str,
    ) -> ApiResult<Option<Record<T>>> {
        let response = self
            .client
            .get_item()
            .table_name(&self.table)
            .set_key(Some(Self::keys(user, key)))
            .consistent_read(true)
            .send()
            .await
            .map_err(internal)?;
        response.item.map(Self::decode).transpose()
    }
    pub async fn list<T: DeserializeOwned>(
        &self,
        user: &str,
        prefix: &str,
    ) -> ApiResult<Vec<Record<T>>> {
        let mut out = vec![];
        let mut cursor = None;
        loop {
            let page = self
                .client
                .query()
                .table_name(&self.table)
                .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
                .expression_attribute_values(":pk", AttributeValue::S(format!("USER#{user}")))
                .expression_attribute_values(":prefix", AttributeValue::S(prefix.into()))
                .consistent_read(true)
                .set_exclusive_start_key(cursor)
                .send()
                .await
                .map_err(internal)?;
            for item in page.items.unwrap_or_default() {
                out.push(Self::decode(item)?);
            }
            cursor = page.last_evaluated_key.filter(|x| !x.is_empty());
            if cursor.is_none() {
                break;
            }
        }
        Ok(out)
    }
    pub async fn transaction(&self, user: &str, writes: Vec<Write>) -> ApiResult<()> {
        if writes.is_empty()
            || writes.len() > 100
            || writes
                .iter()
                .map(|w| w.json.len() + w.key.len() + user.len() + 128)
                .sum::<usize>()
                > 3_900_000
        {
            return Err(ApiError::bad("Transaction exceeds the storage size limit"));
        }
        let mut items = Vec::with_capacity(writes.len());
        for write in writes {
            let mut item = Self::keys(user, &write.key);
            item.insert("data".into(), AttributeValue::S(write.json));
            item.insert(
                "version".into(),
                AttributeValue::N(write.expected.map_or(0, |n| n + 1).to_string()),
            );
            let mut put = Put::builder().table_name(&self.table).set_item(Some(item));
            if let Some(version) = write.expected {
                put = put
                    .condition_expression("#v = :v")
                    .expression_attribute_names("#v", "version")
                    .expression_attribute_values(":v", AttributeValue::N(version.to_string()));
            } else {
                put = put.condition_expression("attribute_not_exists(pk)");
            }
            items.push(
                TransactWriteItem::builder()
                    .put(put.build().map_err(internal)?)
                    .build(),
            );
        }
        let result = self
            .client
            .transact_write_items()
            .set_transact_items(Some(items))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                if e.as_service_error()
                    .is_some_and(|x| x.is_transaction_canceled_exception())
                {
                    Err(ApiError::conflict(
                        "Concurrent update; retry the same event",
                    ))
                } else {
                    Err(internal(e))
                }
            }
        }
    }
    pub async fn save<T: Serialize>(
        &self,
        user: &str,
        key: &str,
        value: &T,
        expected: Option<u64>,
    ) -> ApiResult<()> {
        self.transaction(user, vec![Write::new(key, value, expected)?])
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_records_are_rejected_before_dynamodb() {
        assert!(Write::new("QUESTION#large", &"a".repeat(390_000), None).is_err());
        assert!(Write::new("QUESTION#small", &"a".repeat(96_000), None).is_ok());
    }
}

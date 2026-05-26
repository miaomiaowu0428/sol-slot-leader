//! 数据库访问：查询 slot_leader 表

use crate::types::ClientType;
use sea_orm::{
    ConnectionTrait, Database, DatabaseConnection, DbErr, Statement, Value,
};

/// DB 连接配置。
#[derive(Debug, Clone)]
pub struct DbConfig {
    pub url: String,
}

impl DbConfig {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

/// 连接数据库。
pub async fn connect(cfg: &DbConfig) -> Result<DatabaseConnection, DbErr> {
    Database::connect(&cfg.url).await
}

/// 从 slot_leader 表中拉取 [from_slot, to_slot] 范围内的所有行。
///
/// 返回 `Vec<(slot, leader, client_type, name, region, ip, tpu_quic)>`。
pub async fn fetch_range(
    conn: &DatabaseConnection,
    from_slot: u64,
    to_slot: u64,
) -> Result<Vec<SlotLeaderRow>, DbErr> {
    let backend = conn.get_database_backend();
    let rows = conn
        .query_all(Statement::from_sql_and_values(
            backend,
            "SELECT slot, client_type, name \
             FROM slot_leader \
             WHERE slot >= ? AND slot <= ? \
             ORDER BY slot ASC",
            [
                Value::BigUnsigned(Some(from_slot)),
                Value::BigUnsigned(Some(to_slot)),
            ],
        ))
        .await?;

    let result = rows
        .into_iter()
        .filter_map(|row| {
            let slot: u64 = row.try_get_by_index::<i64>(0).ok()? as u64;
            let raw: Option<String> = row.try_get_by_index(1).ok();
            let name: Option<String> = row.try_get_by_index(2).ok();
            Some(SlotLeaderRow {
                slot,
                client_type: ClientType::from(raw),
                name,
            })
        })
        .collect();

    Ok(result)
}

/// 查询 slot_leader 表中最大的 slot 号。
pub async fn fetch_max_slot(conn: &DatabaseConnection) -> Result<Option<u64>, DbErr> {
    let backend = conn.get_database_backend();
    let row = conn
        .query_one(Statement::from_string(
            backend,
            "SELECT MAX(slot) FROM slot_leader".to_string(),
        ))
        .await?;

    let max = row.and_then(|r| {
        r.try_get_by_index::<Option<i64>>(0)
            .ok()
            .flatten()
            .map(|v| v as u64)
    });

    Ok(max)
}

/// slot_leader 表的行（精简版，只取决策所需字段）。
#[derive(Debug, Clone)]
pub struct SlotLeaderRow {
    pub slot: u64,
    pub client_type: ClientType,
    pub name: Option<String>,
}

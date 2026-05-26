//! 后台刷新逻辑：RPC 获取当前 slot + DB 拉取增量数据写入缓存

use crate::{CacheInner, LeaderInfo, db};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use std::sync::Arc;

/// 执行一次刷新：
/// 1. RPC getSlot → current_slot
/// 2. DB fetch_max_slot → max_db_slot
/// 3. DB fetch_range(current_slot, max_db_slot) → 写入 map
/// 4. 淘汰 current_slot 之前的旧条目
pub async fn do_refresh(inner: &Arc<CacheInner>) -> anyhow::Result<()> {
    let rpc = RpcClient::new_with_commitment(
        inner.rpc_url.clone(),
        CommitmentConfig::processed(),
    );

    // 1. 当前 slot
    let current_slot = rpc
        .get_slot()
        .await
        .map_err(|e| anyhow::anyhow!("getSlot 失败: {}", e))?;

    // 2. DB 最大 slot
    let conn = db::connect(&inner.db_config).await?;
    let max_db_slot = db::fetch_max_slot(&conn)
        .await?
        .ok_or_else(|| anyhow::anyhow!("slot_leader 表为空"))?;

    if max_db_slot < current_slot {
        log::warn!(
            "[SlotLeaderCache] DB 最大 slot={} < 当前 slot={}，数据可能滞后",
            max_db_slot,
            current_slot
        );
    }

    let to_slot = max_db_slot;

    // 3. 拉取范围数据写入 map
    let rows = db::fetch_range(&conn, current_slot, to_slot).await?;
    let fetched = rows.len();

    for row in rows {
        inner.map.insert(
            row.slot,
            LeaderInfo {
                client_type: row.client_type,
                name: row.name,
            },
        );
    }

    // 4. 淘汰已过期的旧 slot（节省内存）
    inner.map.retain(|slot, _| *slot >= current_slot);

    // 5. 更新 last_range
    *inner.last_range.write().await = Some((current_slot, to_slot));

    log::info!(
        "[SlotLeaderCache] 刷新完成: current_slot={} max_db_slot={} loaded={} cached_total={}",
        current_slot,
        max_db_slot,
        fetched,
        inner.map.len()
    );

    Ok(())
}

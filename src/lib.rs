//! sol-slot-leader
//!
//! 每 10 分钟从 RPC 获取当前 slot，再从数据库拉取
//! [current_slot, max_slot_in_db] 区间内的 (slot → client_type) 映射，
//! 缓存在内存 DashMap 中供上层同步查询。
//!
//! 公共接口：
//!   - [`SlotLeaderCache`]  — 带后台刷新的内存缓存，主要使用入口
//!   - [`LeaderInfo`]       — 单个 slot 的 leader 信息
//!   - [`SlotOracle`]       — 供 sol-tx-dispacher 使用的查询 trait
//!   - [`NoopOracle`]       — 无 DB 时的 fallback 实现

mod db;
mod refresh;
pub mod types;

pub use db::DbConfig;
pub use types::ClientType;

// ── AnyOracle —— 封装两种实现，供需要将 OnceLock 存储的场景使用 ────────────────

/// 统一封装 `SlotLeaderCache`（有 DB）和 `NoopOracle`（无 DB）。
///
/// 用途：`OnceLock<TxDispacher<AnyOracle>>` — 需要平衡“类型固定”与“fallback”两个需求时。
#[derive(Clone)]
pub enum AnyOracle {
    Db(SlotLeaderCache),
    Noop(NoopOracle),
}

impl SlotOracle for AnyOracle {
    fn leader_at(&self, slot: u64) -> Option<LeaderInfo> {
        match self {
            AnyOracle::Db(c) => c.leader_at(slot),
            AnyOracle::Noop(n) => n.leader_at(slot),
        }
    }
}

impl AnyOracle {
    /// 尝试连接 DB，失败时退化到 NoopOracle。
    pub async fn from_env(db_url: &str, rpc_url: &str) -> Self {
        match SlotLeaderCache::new(DbConfig::new(db_url), rpc_url).await {
            Ok(cache) => {
                log::info!("[AnyOracle] SlotLeaderCache 初始化成功");
                AnyOracle::Db(cache)
            }
            Err(e) => {
                log::warn!("[AnyOracle] DB 连接失败，退化到 NoopOracle: {}", e);
                AnyOracle::Noop(NoopOracle)
            }
        }
    }

    /// 是否已连接到真实 DB。
    pub fn is_db(&self) -> bool {
        matches!(self, AnyOracle::Db(_))
    }

    /// 如果是 Db 分支，启动后台刷新任务。
    pub fn spawn_refresh_task_if_db(&self) {
        if let AnyOracle::Db(cache) = self {
            cache.spawn_refresh_task();
        }
    }
}

use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ── 公共数据类型 ──────────────────────────────────────────────────────────────

/// 单个 slot 的 leader 元数据。
#[derive(Debug, Clone)]
pub struct LeaderInfo {
    pub client_type: ClientType,
    /// 节点名称（如 "Harmonic-SG"），用于辅助判断 client_type 为 Other 时是否为 Harmonic 节点。
    pub name: Option<String>,
}

impl LeaderInfo {
    /// 是否应走 Harmonic 发送策略。
    ///
    /// 两个条件任意一个成立即走 Harmonic 模式：
    /// 1. `client_type` 是已知的 Harmonic 变体（`HarmonicAgave` / `HarmonicFrankenDancer`）
    /// 2. `name` 字段包含 "harmonic"（大小写不敏感）
    pub fn is_harmonic(&self) -> bool {
        if self.client_type.is_harmonic() {
            return true;
        }
        self.name
            .as_deref()
            .map(|n| n.to_ascii_lowercase().contains("harmonic"))
            .unwrap_or(false)
    }

    /// 是否为 Jito 节点。
    pub fn is_jito(&self) -> bool {
        matches!(&self.client_type, ClientType::JitoLabs)
    }
}

// ── SlotOracle trait ──────────────────────────────────────────────────────────

/// 供 `sol-tx-dispacher` 使用的 slot leader 查询接口。
///
/// 实现者：[`SlotLeaderCache`]（有 DB）/ [`NoopOracle`]（无 DB fallback）。
pub trait SlotOracle: Send + Sync {
    /// 查询指定 slot 的 leader 信息。同步，内部走内存缓存，无 I/O。
    fn leader_at(&self, slot: u64) -> Option<LeaderInfo>;
}

// ── NoopOracle ────────────────────────────────────────────────────────────────

/// 无数据库时的 fallback oracle，始终返回 None。
///
/// 注入 `sol-tx-dispacher` 后，分发器会自动退化到默认发送策略。
#[derive(Clone)]
pub struct NoopOracle;

impl SlotOracle for NoopOracle {
    fn leader_at(&self, _slot: u64) -> Option<LeaderInfo> {
        None
    }
}

// ── SlotLeaderCache ───────────────────────────────────────────────────────────

/// 带后台刷新的内存缓存，实现 [`SlotOracle`]。
///
/// # 使用方式
///
/// ```rust,ignore
/// let cache = SlotLeaderCache::new(db_config, rpc_url).await?;
/// // 启动后台刷新任务（每 10 分钟）
/// cache.spawn_refresh_task();
/// // 查询
/// if let Some(info) = cache.leader_at(current_slot + 1) {
///     println!("{:?}", info);
/// }
/// ```
#[derive(Clone)]
pub struct SlotLeaderCache {
    inner: Arc<CacheInner>,
}

struct CacheInner {
    /// slot → LeaderInfo 的内存缓存
    map: DashMap<u64, LeaderInfo>,
    /// 上次刷新时缓存覆盖的 slot 范围（用于日志和监控）
    last_range: RwLock<Option<(u64, u64)>>,
    db_config: DbConfig,
    rpc_url: String,
}

impl SlotLeaderCache {
    /// 初始化并执行首次全量加载，返回已就绪的 cache。
    pub async fn new(db_config: DbConfig, rpc_url: impl Into<String>) -> anyhow::Result<Self> {
        let inner = Arc::new(CacheInner {
            map: DashMap::new(),
            last_range: RwLock::new(None),
            db_config,
            rpc_url: rpc_url.into(),
        });

        let cache = Self { inner };
        cache.refresh_once().await?;

        Ok(cache)
    }

    /// 启动后台刷新任务（每 10 分钟）。
    ///
    /// 刷新失败时只打 warn 日志，不 panic，保证现有缓存继续可用。
    pub fn spawn_refresh_task(&self) {
        let cache = self.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(10 * 60));
            interval.tick().await; // 跳过第一次立即触发（new() 里已经加载过了）
            loop {
                interval.tick().await;
                if let Err(e) = cache.refresh_once().await {
                    log::warn!("[SlotLeaderCache] 刷新失败: {:#}", e);
                }
            }
        });
    }

    /// 手动触发一次刷新（测试 / 强制更新场景）。
    pub async fn refresh_once(&self) -> anyhow::Result<()> {
        refresh::do_refresh(&self.inner).await
    }

    /// 当前缓存的 slot 范围，None 表示尚未加载。
    pub async fn cached_range(&self) -> Option<(u64, u64)> {
        *self.inner.last_range.read().await
    }

    /// 当前缓存条目数。
    pub fn len(&self) -> usize {
        self.inner.map.len()
    }

    /// 缓存是否为空。
    pub fn is_empty(&self) -> bool {
        self.inner.map.is_empty()
    }
}

impl SlotOracle for SlotLeaderCache {
    fn leader_at(&self, slot: u64) -> Option<LeaderInfo> {
        self.inner.map.get(&slot).map(|r| r.clone())
    }
}

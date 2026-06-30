//! 客户端类型枚举

use std::sync::Arc;

/// Solana validator 客户端类型。
///
/// # Other 变体
/// 用 `Arc<str>` 保存未知字符串：
/// - 比 `String` 省一个 capacity 字段（24 → 16 字节）
/// - clone 是 O(1) 原子操作，适合高频从 DashMap 读出
/// - 比 `Arc<Cow<str>>` 少一层包装，语义更直接
///
/// SQL NULL 也映射到 `Other(Arc::from(""))` 而非单独 variant，
/// `is_harmonic()` 对空串返回 false，行为安全。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientType {
    Agave,
    AgaveBam,
    FireDancer,
    FrankenDancer,
    HarmonicAgave,
    HarmonicFrankenDancer,
    JitoLabs,
    Rakurai,
    /// 未知或 NULL，原始字符串以 `Arc<str>` 保存。
    Other(Arc<str>),
}

impl ClientType {
    /// 是否为 Harmonic 系节点。
    pub fn is_harmonic(&self) -> bool {
        matches!(
            self,
            ClientType::HarmonicAgave | ClientType::HarmonicFrankenDancer
        )
    }
}

impl From<&str> for ClientType {
    fn from(s: &str) -> Self {
        // 去首尾空白后做大小写不敏感匹配
        match s.trim() {
            t if t.eq_ignore_ascii_case("agave") => ClientType::Agave,
            t if t.eq_ignore_ascii_case("agave_bam") || t.eq_ignore_ascii_case("agavebam") => {
                ClientType::AgaveBam
            }
            t if t.eq_ignore_ascii_case("firedancer") || t.eq_ignore_ascii_case("fire_dancer") => {
                ClientType::FireDancer
            }
            t if t.eq_ignore_ascii_case("frankendancer")
                || t.eq_ignore_ascii_case("franken_dancer") =>
            {
                ClientType::FrankenDancer
            }
            t if t.eq_ignore_ascii_case("harmonicagave")
                || t.eq_ignore_ascii_case("harmonic_agave") =>
            {
                ClientType::HarmonicAgave
            }
            t if t.eq_ignore_ascii_case("harmonicfrankendancer")
                || t.eq_ignore_ascii_case("harmonic_frankendancer")
                || t.eq_ignore_ascii_case("harmonic_franken_dancer") =>
            {
                ClientType::HarmonicFrankenDancer
            }
            t if t.eq_ignore_ascii_case("jitolabs")
                || t.eq_ignore_ascii_case("jito_labs")
                || t.eq_ignore_ascii_case("jito") =>
            {
                ClientType::JitoLabs
            }
            t if t.eq_ignore_ascii_case("rakurai") => ClientType::Rakurai,
            other => ClientType::Other(Arc::from(other)),
        }
    }
}

impl From<String> for ClientType {
    fn from(s: String) -> Self {
        ClientType::from(s.as_str())
    }
}

impl From<Option<String>> for ClientType {
    fn from(opt: Option<String>) -> Self {
        match opt {
            Some(s) => ClientType::from(s.as_str()),
            None => ClientType::Other(Arc::from("")),
        }
    }
}

impl std::fmt::Display for ClientType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientType::Agave => write!(f, "Agave"),
            ClientType::AgaveBam => write!(f, "AgaveBam"),
            ClientType::FireDancer => write!(f, "FireDancer"),
            ClientType::FrankenDancer => write!(f, "FrankenDancer"),
            ClientType::HarmonicAgave => write!(f, "HarmonicAgave"),
            ClientType::HarmonicFrankenDancer => write!(f, "HarmonicFrankenDancer"),
            ClientType::JitoLabs => write!(f, "JitoLabs"),
            ClientType::Rakurai => write!(f, "Rakurai"),
            ClientType::Other(s) => write!(f, "Other({})", s),
        }
    }
}

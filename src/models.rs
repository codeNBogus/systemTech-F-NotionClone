use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::fmt;

pub fn default_actor_nickname() -> String {
    "anonymous".to_string()
}

pub fn normalize_actor(actor: Option<String>) -> String {
    actor
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(default_actor_nickname)
}

mod kst_serde {
    use chrono::{DateTime, FixedOffset, Utc};
    use serde::{Deserializer, Serializer, Deserialize};

    pub fn serialize<S>(dt: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let kst = dt.with_timezone(&FixedOffset::east_opt(9 * 3600).unwrap());
        s.serialize_str(&kst.to_rfc3339())
    }

    pub fn deserialize<'de, D>(d: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

/// 카드 수정 연산 종류
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModificationOperation {
    Created,
    Updated,
    StatusChanged,
    Moved,
    Reordered,
}

/// 카드 수정 이력 항목
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModificationLog {
    /// 이 변경 후의 version 번호
    pub version: u64,
    pub operation: ModificationOperation,
    #[serde(with = "kst_serde")]
    pub timestamp: DateTime<Utc>,
    /// 변경 내용 요약 (동시성 문제 분석용)
    pub detail: String,
}

/// 카드 상태 (미진행 / 진행중 / 완료)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardStatus {
    /// 미진행
    Todo,
    /// 진행중
    InProgress,
    /// 완료
    Done,
}

impl Default for CardStatus {
    fn default() -> Self {
        CardStatus::Todo
    }
}

impl fmt::Display for CardStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CardStatus::Todo => write!(f, "미진행"),
            CardStatus::InProgress => write!(f, "진행중"),
            CardStatus::Done => write!(f, "완료"),
        }
    }
}

/// 보드 (Board) - 최상위 컨테이너
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub id: String,
    pub title: String,
    #[serde(default = "default_actor_nickname")]
    pub owner_nickname: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 컬럼 (Column) - 보드 내 카드 그룹
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub id: String,
    pub board_id: String,
    pub title: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
}

/// 카드 (Card) - 개별 작업 항목
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub column_id: String,
    pub title: String,
    pub description: String,
    pub status: CardStatus,
    pub position: i32,
    /// Optimistic Locking을 위한 버전 필드
    pub version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// 수정 이력 — 동시성 문제 분석용
    pub modification_logs: Vec<ModificationLog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLog {
    pub id: String,
    pub board_id: String,
    pub actor_nickname: String,
    pub action: String,
    pub target_type: String,
    pub target_id: String,
    #[serde(with = "kst_serde")]
    pub timestamp: DateTime<Utc>,
    pub detail: String,
}

// === Request / Response DTOs ===

#[derive(Debug, Deserialize)]
pub struct CreateBoardRequest {
    pub title: String,
    #[serde(default)]
    pub actor_nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ActorRequest {
    #[serde(default)]
    pub actor_nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateColumnRequest {
    pub title: String,
    #[serde(default)]
    pub actor_nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCardRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub actor_nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCardRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<CardStatus>,
    /// 클라이언트가 보유한 버전 (Optimistic Locking)
    pub version: u64,
    #[serde(default)]
    pub actor_nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCardStatusRequest {
    pub status: CardStatus,
    pub version: u64,
    #[serde(default)]
    pub actor_nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MoveCardRequest {
    pub target_column_id: String,
    pub target_position: i32,
    /// 클라이언트가 보유한 버전 (Optimistic Locking)
    pub version: u64,
    #[serde(default)]
    pub actor_nickname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderCardRequest {
    pub target_position: i32,
    pub version: u64,
    #[serde(default)]
    pub actor_nickname: Option<String>,
}

/// 보드 전체 조회 응답
#[derive(Debug, Serialize)]
pub struct BoardDetailResponse {
    pub board: Board,
    pub columns: Vec<ColumnWithCards>,
}

#[derive(Debug, Serialize)]
pub struct ColumnWithCards {
    pub column: Column,
    pub cards: Vec<Card>,
}

/// 에러 응답
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: String,
}

/// WebSocket 브로드캐스트 이벤트 (WAL replay를 위해 Deserialize 추가)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WsEvent {
    BoardCreated { board: Board },
    BoardDeleted { board_id: String },
    AuditLogged { log: AuditLog },
    CardCreated { card: Card },
    CardUpdated { card: Card },
    CardDeleted { card_id: String },
    CardMoved { card: Card },
    CardStatusChanged { card: Card },
    CardReordered { card: Card },
    ColumnCreated { column: Column },
    ColumnDeleted { column_id: String },
}


// === 팩토리 메서드 ===

impl Board {
    pub fn new(title: String, owner_nickname: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            owner_nickname,
            created_at: now,
            updated_at: now,
        }
    }
}

impl AuditLog {
    pub fn new(
        board_id: String,
        actor_nickname: String,
        action: impl Into<String>,
        target_type: impl Into<String>,
        target_id: String,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            board_id,
            actor_nickname,
            action: action.into(),
            target_type: target_type.into(),
            target_id,
            timestamp: Utc::now(),
            detail: detail.into(),
        }
    }
}

impl Column {
    pub fn new(board_id: String, title: String, position: i32) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            board_id,
            title,
            position,
            created_at: Utc::now(),
        }
    }
}

impl Card {
    pub fn new(column_id: String, title: String, description: String, position: i32) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            column_id,
            title,
            description,
            status: CardStatus::default(),
            position,
            version: 1,
            created_at: now,
            updated_at: now,
            modification_logs: vec![ModificationLog {
                version: 1,
                operation: ModificationOperation::Created,
                timestamp: now,
                detail: "카드 생성".to_string(),
            }],
        }
    }

    /// 수정 로그를 기록하고 version을 올린다.
    pub fn push_log(&mut self, operation: ModificationOperation, detail: String) {
        let now = Utc::now();
        self.version += 1;
        self.updated_at = now;
        self.modification_logs.push(ModificationLog {
            version: self.version,
            operation,
            timestamp: now,
            detail,
        });
    }
}

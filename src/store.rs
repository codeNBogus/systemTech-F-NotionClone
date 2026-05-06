use crate::wal::{AuditLogWriter, WalWriter};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::errors::AppError;
use crate::models::*;

/// 메모리 기반 데이터 저장소
/// Arc<RwLock<...>>을 사용하여 다중 사용자 동시 접근을 안전하게 처리
#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<StoreInner>>,
    /// WebSocket 브로드캐스트 채널 — write lock 안에서 send하여 이벤트 순서를 보장
    tx: broadcast::Sender<Arc<String>>,
    /// Write-Ahead Log (선택적) — Some이면 모든 이벤트를 디스크에 기록
    wal: Option<Arc<WalWriter>>,
    audit_log: Option<Arc<AuditLogWriter>>,
}

#[derive(Debug)]
struct StoreInner {
    boards: HashMap<String, Board>,
    columns: HashMap<String, Column>,
    cards: HashMap<String, Card>,
    audit_logs: HashMap<String, Vec<AuditLog>>,
}

impl AppState {
    /// WAL 없이 생성 (테스트용)
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(RwLock::new(StoreInner {
                boards: HashMap::new(),
                columns: HashMap::new(),
                cards: HashMap::new(),
                audit_logs: HashMap::new(),
            })),
            tx,
            wal: None,
            audit_log: None,
        }
    }

    /// WAL과 함께 생성 (프로덕션용)
    pub fn with_wal(wal: Arc<WalWriter>) -> Self {
        Self::with_wal_and_audit(wal, None)
    }

    pub fn with_wal_and_audit(
        wal: Arc<WalWriter>,
        audit_log: Option<Arc<AuditLogWriter>>,
    ) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(RwLock::new(StoreInner {
                boards: HashMap::new(),
                columns: HashMap::new(),
                cards: HashMap::new(),
                audit_logs: HashMap::new(),
            })),
            tx,
            wal: Some(wal),
            audit_log,
        }
    }

    /// WAL replay된 이벤트들을 순서대로 메모리에 적용
    pub async fn apply_events(&self, events: Vec<WsEvent>) {
        let mut store = self.inner.write().await;
        for event in events {
            match event {
                WsEvent::BoardCreated { board } => {
                    store.boards.insert(board.id.clone(), board);
                }
                WsEvent::BoardDeleted { board_id } => {
                    store.boards.remove(&board_id);
                    // 삭제된 보드의 컬럼 ID들을 먼저 수집
                    let deleted_column_ids: Vec<String> = store
                        .columns
                        .values()
                        .filter(|c| c.board_id == board_id)
                        .map(|c| c.id.clone())
                        .collect();
                    // 그 컬럼들 제거
                    store.columns.retain(|_, c| c.board_id != board_id);
                    // 그 컬럼에 속한 카드들도 제거
                    store
                        .cards
                        .retain(|_, c| !deleted_column_ids.contains(&c.column_id));
                }
                WsEvent::AuditLogged { .. } => {}

                WsEvent::ColumnCreated { column } => {
                    store.columns.insert(column.id.clone(), column);
                }
                WsEvent::ColumnDeleted { column_id } => {
                    store.columns.remove(&column_id);
                    store.cards.retain(|_, c| c.column_id != column_id);
                }
                WsEvent::CardCreated { card }
                | WsEvent::CardUpdated { card }
                | WsEvent::CardMoved { card }
                | WsEvent::CardStatusChanged { card }
                | WsEvent::CardReordered { card } => {
                    store.cards.insert(card.id.clone(), card);
                }
                WsEvent::CardDeleted { card_id } => {
                    store.cards.remove(&card_id);
                }
            }
        }
    }

    pub async fn apply_audit_logs(&self, logs: Vec<AuditLog>) {
        let mut store = self.inner.write().await;
        for log in logs {
            store
                .audit_logs
                .entry(log.board_id.clone())
                .or_default()
                .push(log);
        }
    }

    /// WS 클라이언트가 구독할 Receiver 반환
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<String>> {
        self.tx.subscribe()
    }

    /// WsEvent를 JSON 직렬화하여 브로드캐스트 (sync — write lock 안에서 호출 가능)
    fn emit(&self, event: WsEvent) {
        // 1) WAL에 먼저 기록 (durability 보장)
        if let Some(wal) = &self.wal {
            if let Err(e) = wal.append(&event) {
                eprintln!("⚠️  WAL append failed: {}", e);
            }
        }
        // 2) 브로드캐스트 (구독자 없으면 skip)
        if self.tx.receiver_count() == 0 {
            return;
        }
        if let Ok(json) = serde_json::to_string(&event) {
            let _ = self.tx.send(Arc::new(json));
        }
    }

    fn record_audit(&self, store: &mut StoreInner, log: AuditLog) {
        store
            .audit_logs
            .entry(log.board_id.clone())
            .or_default()
            .push(log.clone());
        if let Some(audit_log) = &self.audit_log {
            if let Err(e) = audit_log.append(&log) {
                eprintln!("Audit log append failed: {}", e);
            }
        }
    }

    // ========== Board ==========

    pub async fn create_board(&self, title: String) -> Board {
        self.create_board_as(title, None).await
    }

    pub async fn create_board_as(&self, title: String, actor_nickname: Option<String>) -> Board {
        let actor = normalize_actor(actor_nickname);
        let board = Board::new(title, actor.clone());
        let mut store = self.inner.write().await;
        store.boards.insert(board.id.clone(), board.clone());
        self.emit(WsEvent::BoardCreated {
            board: board.clone(),
        });
        self.record_audit(
            &mut store,
            AuditLog::new(
                board.id.clone(),
                actor,
                "board_created",
                "board",
                board.id.clone(),
                format!("board \"{}\" created", board.title),
            ),
        );
        board
    }

    pub async fn list_boards(&self) -> Vec<Board> {
        let store = self.inner.read().await;
        let mut boards: Vec<Board> = store.boards.values().cloned().collect();
        boards.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        boards
    }

    pub async fn get_board_detail(&self, board_id: &str) -> Result<BoardDetailResponse, AppError> {
        let store = self.inner.read().await;

        let board = store
            .boards
            .get(board_id)
            .ok_or_else(|| AppError::BoardNotFound(board_id.to_string()))?
            .clone();

        let mut columns: Vec<Column> = store
            .columns
            .values()
            .filter(|c| c.board_id == board_id)
            .cloned()
            .collect();
        columns.sort_by_key(|c| c.position);

        let columns_with_cards: Vec<ColumnWithCards> = columns
            .into_iter()
            .map(|col| {
                let mut cards: Vec<Card> = store
                    .cards
                    .values()
                    .filter(|card| card.column_id == col.id)
                    .cloned()
                    .collect();
                cards.sort_by_key(|c| c.position);
                ColumnWithCards { column: col, cards }
            })
            .collect();

        Ok(BoardDetailResponse {
            board,
            columns: columns_with_cards,
        })
    }

    // ========== Column ==========

    pub async fn create_column(&self, board_id: &str, title: String) -> Result<Column, AppError> {
        self.create_column_as(board_id, title, None).await
    }

    pub async fn create_column_as(
        &self,
        board_id: &str,
        title: String,
        actor_nickname: Option<String>,
    ) -> Result<Column, AppError> {
        let actor = normalize_actor(actor_nickname);
        let mut store = self.inner.write().await;

        if !store.boards.contains_key(board_id) {
            return Err(AppError::BoardNotFound(board_id.to_string()));
        }

        let max_pos = store
            .columns
            .values()
            .filter(|c| c.board_id == board_id)
            .map(|c| c.position)
            .max()
            .unwrap_or(-1);

        let column = Column::new(board_id.to_string(), title, max_pos + 1);
        store.columns.insert(column.id.clone(), column.clone());
        self.emit(WsEvent::ColumnCreated {
            column: column.clone(),
        });
        self.record_audit(
            &mut store,
            AuditLog::new(
                board_id.to_string(),
                actor,
                "column_created",
                "column",
                column.id.clone(),
                format!("column \"{}\" created", column.title),
            ),
        );
        Ok(column)
    }

    /// 보드 삭제 - 보드에 속한 모든 컬럼과 카드도 함께 삭제 (cascade)
    pub async fn delete_board(&self, board_id: &str) -> Result<(), AppError> {
        self.delete_board_as(board_id, None).await
    }

    pub async fn delete_board_as(
        &self,
        board_id: &str,
        actor_nickname: Option<String>,
    ) -> Result<(), AppError> {
        let actor = normalize_actor(actor_nickname);
        let mut store = self.inner.write().await;

        let board = store
            .boards
            .get(board_id)
            .ok_or_else(|| AppError::BoardNotFound(board_id.to_string()))?
            .clone();

        // 삭제할 컬럼 ID들을 먼저 수집 (borrow checker 회피)
        let deleted_column_ids: Vec<String> = store
            .columns
            .values()
            .filter(|c| c.board_id == board_id)
            .map(|c| c.id.clone())
            .collect();

        // 보드 제거
        store.boards.remove(board_id);
        // 보드의 컬럼 제거
        store.columns.retain(|_, c| c.board_id != board_id);
        // 그 컬럼들에 속한 카드 제거
        store
            .cards
            .retain(|_, c| !deleted_column_ids.contains(&c.column_id));

        self.emit(WsEvent::BoardDeleted {
            board_id: board_id.to_string(),
        });
        self.record_audit(
            &mut store,
            AuditLog::new(
                board_id.to_string(),
                actor,
                "board_deleted",
                "board",
                board_id.to_string(),
                format!("board \"{}\" deleted", board.title),
            ),
        );
        Ok(())
    }

    pub async fn delete_column(&self, column_id: &str) -> Result<(), AppError> {
        self.delete_column_as(column_id, None).await
    }

    pub async fn delete_column_as(
        &self,
        column_id: &str,
        actor_nickname: Option<String>,
    ) -> Result<(), AppError> {
        let actor = normalize_actor(actor_nickname);
        let mut store = self.inner.write().await;

        let column = store
            .columns
            .remove(column_id)
            .ok_or_else(|| AppError::ColumnNotFound(column_id.to_string()))?;

        store.cards.retain(|_, card| card.column_id != column_id);
        self.emit(WsEvent::ColumnDeleted {
            column_id: column_id.to_string(),
        });
        self.record_audit(
            &mut store,
            AuditLog::new(
                column.board_id,
                actor,
                "column_deleted",
                "column",
                column_id.to_string(),
                format!("column \"{}\" deleted", column.title),
            ),
        );
        Ok(())
    }

    // ========== Card ==========

    /// 카드 생성 - 동일 컬럼에 동시 추가 시 position 충돌 방지
    /// write lock으로 atomic하게 처리
    pub async fn create_card(
        &self,
        column_id: &str,
        title: String,
        description: String,
    ) -> Result<Card, AppError> {
        self.create_card_as(column_id, title, description, None).await
    }

    pub async fn create_card_as(
        &self,
        column_id: &str,
        title: String,
        description: String,
        actor_nickname: Option<String>,
    ) -> Result<Card, AppError> {
        let actor = normalize_actor(actor_nickname);
        let mut store = self.inner.write().await;

        let board_id = store
            .columns
            .get(column_id)
            .ok_or_else(|| AppError::ColumnNotFound(column_id.to_string()))?
            .board_id
            .clone();

        let max_pos = store
            .cards
            .values()
            .filter(|c| c.column_id == column_id)
            .map(|c| c.position)
            .max()
            .unwrap_or(-1);

        let card = Card::new(column_id.to_string(), title, description, max_pos + 1);
        store.cards.insert(card.id.clone(), card.clone());
        self.emit(WsEvent::CardCreated { card: card.clone() });
        self.record_audit(
            &mut store,
            AuditLog::new(
                board_id,
                actor,
                "card_created",
                "card",
                card.id.clone(),
                format!("card \"{}\" created", card.title),
            ),
        );
        Ok(card)
    }

    pub async fn get_card(&self, card_id: &str) -> Result<Card, AppError> {
        let store = self.inner.read().await;
        store
            .cards
            .get(card_id)
            .cloned()
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))
    }

    /// 카드 수정 - Optimistic Locking (버전 기반 충돌 감지)
    /// 동일 카드를 동시에 수정할 때 version 불일치 시 충돌 에러 반환
    pub async fn update_card(
        &self,
        card_id: &str,
        req: UpdateCardRequest,
    ) -> Result<Card, AppError> {
        let actor = normalize_actor(req.actor_nickname.clone());
        let mut store = self.inner.write().await;

        let card = store
            .cards
            .get_mut(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        if card.version != req.version {
            return Err(AppError::VersionConflict {
                expected: req.version,
                actual: card.version,
            });
        }

        let mut changed = Vec::new();
        if let Some(ref title) = req.title {
            changed.push(format!("title → \"{}\"", title));
            card.title = title.clone();
        }
        if let Some(ref description) = req.description {
            changed.push(format!("description → \"{}\"", description));
            card.description = description.clone();
        }
        if let Some(status) = req.status {
            changed.push(format!("status → {}", status));
            card.status = status;
        }

        let detail = if changed.is_empty() {
            "변경 없음".to_string()
        } else {
            changed.join(", ")
        };
        card.push_log(ModificationOperation::Updated, detail);
        let updated_card = card.clone();
        let board_id = store
            .columns
            .get(&updated_card.column_id)
            .map(|column| column.board_id.clone())
            .unwrap_or_default();
        self.emit(WsEvent::CardUpdated {
            card: updated_card.clone(),
        });
        if !board_id.is_empty() {
            self.record_audit(
                &mut store,
                AuditLog::new(
                    board_id,
                    actor,
                    "card_updated",
                    "card",
                    updated_card.id.clone(),
                    format!("card \"{}\" updated", updated_card.title),
                ),
            );
        }
        Ok(updated_card)
    }

    /// 카드 상태 변경 - Optimistic Locking 적용
    pub async fn update_card_status(
        &self,
        card_id: &str,
        req: UpdateCardStatusRequest,
    ) -> Result<Card, AppError> {
        let actor = normalize_actor(req.actor_nickname.clone());
        let mut store = self.inner.write().await;

        let card = store
            .cards
            .get_mut(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        if card.version != req.version {
            return Err(AppError::VersionConflict {
                expected: req.version,
                actual: card.version,
            });
        }

        let detail = format!("status → {}", req.status);
        card.status = req.status;
        card.push_log(ModificationOperation::StatusChanged, detail);
        let updated_card = card.clone();
        let board_id = store
            .columns
            .get(&updated_card.column_id)
            .map(|column| column.board_id.clone())
            .unwrap_or_default();
        self.emit(WsEvent::CardStatusChanged {
            card: updated_card.clone(),
        });
        if !board_id.is_empty() {
            self.record_audit(
                &mut store,
                AuditLog::new(
                    board_id,
                    actor,
                    "card_status_changed",
                    "card",
                    updated_card.id.clone(),
                    format!("card \"{}\" status changed", updated_card.title),
                ),
            );
        }
        Ok(updated_card)
    }

    /// 카드 삭제 - 삭제 후 같은 컬럼 내 position 재정렬
    pub async fn delete_card(&self, card_id: &str) -> Result<(), AppError> {
        self.delete_card_as(card_id, None).await
    }

    pub async fn delete_card_as(
        &self,
        card_id: &str,
        actor_nickname: Option<String>,
    ) -> Result<(), AppError> {
        let actor = normalize_actor(actor_nickname);
        let mut store = self.inner.write().await;

        let card = store
            .cards
            .remove(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;
        let board_id = store
            .columns
            .get(&card.column_id)
            .map(|column| column.board_id.clone())
            .unwrap_or_default();

        for c in store.cards.values_mut() {
            if c.column_id == card.column_id && c.position > card.position {
                c.position -= 1;
            }
        }

        self.emit(WsEvent::CardDeleted {
            card_id: card.id.clone(),
        });
        if !board_id.is_empty() {
            self.record_audit(
                &mut store,
                AuditLog::new(
                    board_id,
                    actor,
                    "card_deleted",
                    "card",
                    card.id.clone(),
                    format!("card \"{}\" deleted", card.title),
                ),
            );
        }
        Ok(())
    }

    /// 카드 이동 (컬럼 간) - atomic 처리로 이동+삭제 동시 발생 방지
    pub async fn move_card(&self, card_id: &str, req: MoveCardRequest) -> Result<Card, AppError> {
        let actor = normalize_actor(req.actor_nickname.clone());
        let mut store = self.inner.write().await;

        if !store.columns.contains_key(&req.target_column_id) {
            return Err(AppError::ColumnNotFound(req.target_column_id.clone()));
        }

        let card = store
            .cards
            .get(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        if card.version != req.version {
            return Err(AppError::VersionConflict {
                expected: req.version,
                actual: card.version,
            });
        }

        let old_column_id = card.column_id.clone();
        let old_position = card.position;

        for c in store.cards.values_mut() {
            if c.column_id == old_column_id && c.position > old_position && c.id != card_id {
                c.position -= 1;
            }
        }

        let target_pos = req.target_position;
        for c in store.cards.values_mut() {
            if c.column_id == req.target_column_id && c.position >= target_pos && c.id != card_id {
                c.position += 1;
            }
        }

        let card = store.cards.get_mut(card_id).unwrap();
        let detail = format!(
            "column {} → {}, position {} → {}",
            old_column_id, req.target_column_id, old_position, target_pos
        );
        card.column_id = req.target_column_id;
        card.position = target_pos;
        card.push_log(ModificationOperation::Moved, detail);
        let updated_card = card.clone();
        let board_id = store
            .columns
            .get(&updated_card.column_id)
            .map(|column| column.board_id.clone())
            .unwrap_or_default();
        self.emit(WsEvent::CardMoved {
            card: updated_card.clone(),
        });
        if !board_id.is_empty() {
            self.record_audit(
                &mut store,
                AuditLog::new(
                    board_id,
                    actor,
                    "card_moved",
                    "card",
                    updated_card.id.clone(),
                    format!("card \"{}\" moved", updated_card.title),
                ),
            );
        }
        Ok(updated_card)
    }

    /// 카드 순서 변경 (같은 컬럼 내) - atomic 처리
    pub async fn reorder_card(
        &self,
        card_id: &str,
        req: ReorderCardRequest,
    ) -> Result<Card, AppError> {
        let actor = normalize_actor(req.actor_nickname.clone());
        let mut store = self.inner.write().await;

        let card = store
            .cards
            .get(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        if card.version != req.version {
            return Err(AppError::VersionConflict {
                expected: req.version,
                actual: card.version,
            });
        }

        let column_id = card.column_id.clone();
        let old_pos = card.position;
        let new_pos = req.target_position;

        if old_pos == new_pos {
            return Ok(card.clone());
        }

        if new_pos > old_pos {
            for c in store.cards.values_mut() {
                if c.column_id == column_id
                    && c.id != card_id
                    && c.position > old_pos
                    && c.position <= new_pos
                {
                    c.position -= 1;
                }
            }
        } else {
            for c in store.cards.values_mut() {
                if c.column_id == column_id
                    && c.id != card_id
                    && c.position >= new_pos
                    && c.position < old_pos
                {
                    c.position += 1;
                }
            }
        }

        let card = store.cards.get_mut(card_id).unwrap();
        let detail = format!("position {} → {}", old_pos, new_pos);
        card.position = new_pos;
        card.push_log(ModificationOperation::Reordered, detail);
        let updated_card = card.clone();
        let board_id = store
            .columns
            .get(&updated_card.column_id)
            .map(|column| column.board_id.clone())
            .unwrap_or_default();
        self.emit(WsEvent::CardReordered {
            card: updated_card.clone(),
        });
        if !board_id.is_empty() {
            self.record_audit(
                &mut store,
                AuditLog::new(
                    board_id,
                    actor,
                    "card_reordered",
                    "card",
                    updated_card.id.clone(),
                    format!("card \"{}\" reordered", updated_card.title),
                ),
            );
        }
        Ok(updated_card)
    }

    pub async fn get_card_logs(&self, card_id: &str) -> Result<Vec<ModificationLog>, AppError> {
        let store = self.inner.read().await;
        let card = store
            .cards
            .get(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;
        Ok(card.modification_logs.clone())
    }

    pub async fn get_board_audit_logs(&self, board_id: &str) -> Result<Vec<AuditLog>, AppError> {
        let store = self.inner.read().await;
        if !store.boards.contains_key(board_id) {
            return Err(AppError::BoardNotFound(board_id.to_string()));
        }
        Ok(store
            .audit_logs
            .get(board_id)
            .cloned()
            .unwrap_or_default())
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::errors::AppError;
use crate::models::*;
use chrono::Utc;

/// 메모리 기반 데이터 저장소
/// Arc<RwLock<...>>을 사용하여 다중 사용자 동시 접근을 안전하게 처리
#[derive(Debug, Clone)]
pub struct AppState {
    inner: Arc<RwLock<StoreInner>>,
}

#[derive(Debug)]
struct StoreInner {
    boards: HashMap<String, Board>,
    columns: HashMap<String, Column>,
    cards: HashMap<String, Card>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(StoreInner {
                boards: HashMap::new(),
                columns: HashMap::new(),
                cards: HashMap::new(),
            })),
        }
    }

    // ========== Board ==========

    pub async fn create_board(&self, title: String) -> Board {
        let board = Board::new(title);
        let mut store = self.inner.write().await;
        store.boards.insert(board.id.clone(), board.clone());
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

        // 해당 보드의 컬럼 조회 (position 순)
        let mut columns: Vec<Column> = store
            .columns
            .values()
            .filter(|c| c.board_id == board_id)
            .cloned()
            .collect();
        columns.sort_by_key(|c| c.position);

        // 각 컬럼의 카드 조회 (position 순)
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
                ColumnWithCards {
                    column: col,
                    cards,
                }
            })
            .collect();

        Ok(BoardDetailResponse {
            board,
            columns: columns_with_cards,
        })
    }

    // ========== Column ==========

    pub async fn create_column(
        &self,
        board_id: &str,
        title: String,
    ) -> Result<Column, AppError> {
        let mut store = self.inner.write().await;

        // 보드 존재 확인
        if !store.boards.contains_key(board_id) {
            return Err(AppError::BoardNotFound(board_id.to_string()));
        }

        // 현재 컬럼 수를 기반으로 position 결정
        let max_pos = store
            .columns
            .values()
            .filter(|c| c.board_id == board_id)
            .map(|c| c.position)
            .max()
            .unwrap_or(-1);

        let column = Column::new(board_id.to_string(), title, max_pos + 1);
        store.columns.insert(column.id.clone(), column.clone());
        Ok(column)
    }

    pub async fn delete_column(&self, column_id: &str) -> Result<(), AppError> {
        let mut store = self.inner.write().await;

        if store.columns.remove(column_id).is_none() {
            return Err(AppError::ColumnNotFound(column_id.to_string()));
        }

        // 해당 컬럼의 모든 카드 삭제
        store.cards.retain(|_, card| card.column_id != column_id);

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
        let mut store = self.inner.write().await;

        // 컬럼 존재 확인
        if !store.columns.contains_key(column_id) {
            return Err(AppError::ColumnNotFound(column_id.to_string()));
        }

        // 현재 컬럼 내 최대 position 계산 → 충돌 없이 순서 부여
        let max_pos = store
            .cards
            .values()
            .filter(|c| c.column_id == column_id)
            .map(|c| c.position)
            .max()
            .unwrap_or(-1);

        let card = Card::new(column_id.to_string(), title, description, max_pos + 1);
        store.cards.insert(card.id.clone(), card.clone());
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
        let mut store = self.inner.write().await;

        let card = store
            .cards
            .get_mut(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        // Optimistic Locking: 버전 비교
        if card.version != req.version {
            return Err(AppError::VersionConflict {
                expected: req.version,
                actual: card.version,
            });
        }

        // 필드 업데이트
        if let Some(title) = req.title {
            card.title = title;
        }
        if let Some(description) = req.description {
            card.description = description;
        }
        if let Some(status) = req.status {
            card.status = status;
        }

        card.version += 1;
        card.updated_at = Utc::now();

        Ok(card.clone())
    }

    /// 카드 상태 변경 - Optimistic Locking 적용
    /// 미진행(Todo) / 진행중(InProgress) / 완료(Done) 간 전환
    pub async fn update_card_status(
        &self,
        card_id: &str,
        req: UpdateCardStatusRequest,
    ) -> Result<Card, AppError> {
        let mut store = self.inner.write().await;

        let card = store
            .cards
            .get_mut(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        // Optimistic Locking: 버전 비교
        if card.version != req.version {
            return Err(AppError::VersionConflict {
                expected: req.version,
                actual: card.version,
            });
        }

        card.status = req.status;
        card.version += 1;
        card.updated_at = Utc::now();

        Ok(card.clone())
    }

    /// 카드 삭제 - 삭제 후 같은 컬럼 내 position 재정렬
    pub async fn delete_card(&self, card_id: &str) -> Result<(), AppError> {
        let mut store = self.inner.write().await;

        let card = store
            .cards
            .remove(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        // 삭제된 카드보다 뒤에 있던 카드들의 position 재정렬
        for c in store.cards.values_mut() {
            if c.column_id == card.column_id && c.position > card.position {
                c.position -= 1;
            }
        }

        Ok(())
    }

    /// 카드 이동 (컬럼 간) - atomic 처리로 이동+삭제 동시 발생 방지
    /// write lock 내에서 카드 존재 확인 → 이전 컬럼 재정렬 → 새 컬럼 삽입
    pub async fn move_card(
        &self,
        card_id: &str,
        req: MoveCardRequest,
    ) -> Result<Card, AppError> {
        let mut store = self.inner.write().await;

        // 대상 컬럼 존재 확인
        if !store.columns.contains_key(&req.target_column_id) {
            return Err(AppError::ColumnNotFound(req.target_column_id.clone()));
        }

        let card = store
            .cards
            .get(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        // Optimistic Locking
        if card.version != req.version {
            return Err(AppError::VersionConflict {
                expected: req.version,
                actual: card.version,
            });
        }

        let old_column_id = card.column_id.clone();
        let old_position = card.position;

        // 이전 컬럼에서 position 재정렬
        for c in store.cards.values_mut() {
            if c.column_id == old_column_id && c.position > old_position && c.id != card_id {
                c.position -= 1;
            }
        }

        // 새 컬럼에서 삽입 위치 확보
        let target_pos = req.target_position;
        for c in store.cards.values_mut() {
            if c.column_id == req.target_column_id && c.position >= target_pos && c.id != card_id {
                c.position += 1;
            }
        }

        // 카드 이동
        let card = store.cards.get_mut(card_id).unwrap();
        card.column_id = req.target_column_id;
        card.position = target_pos;
        card.version += 1;
        card.updated_at = Utc::now();

        Ok(card.clone())
    }

    /// 카드 순서 변경 (같은 컬럼 내) - atomic 처리
    pub async fn reorder_card(
        &self,
        card_id: &str,
        req: ReorderCardRequest,
    ) -> Result<Card, AppError> {
        let mut store = self.inner.write().await;

        let card = store
            .cards
            .get(card_id)
            .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

        // Optimistic Locking
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

        // 같은 컬럼 내 카드들의 position 재정렬
        if new_pos > old_pos {
            // 아래로 이동: old_pos < pos <= new_pos인 카드들을 -1
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
            // 위로 이동: new_pos <= pos < old_pos인 카드들을 +1
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
        card.position = new_pos;
        card.version += 1;
        card.updated_at = Utc::now();

        Ok(card.clone())
    }
}

//! 동시성 및 Race Condition 테스트
//!
//! 이 테스트 모듈은 다중 사용자 환경에서 발생할 수 있는
//! 동시성 문제를 의도적으로 재현하고, 시스템이 이를 올바르게
//! 처리하는지 검증합니다.

use collab_board::store::AppState;
use collab_board::models::*;

/// 헬퍼: 보드 + 컬럼 + 카드 N개 생성
async fn setup_board_with_cards(state: &AppState, card_count: usize) -> (String, String, Vec<Card>) {
    let board = state.create_board("Test Board".into()).await;
    let column = state.create_column(&board.id, "Test Column".into()).await.unwrap();
    let mut cards = Vec::new();
    for i in 0..card_count {
        let card = state
            .create_card(&column.id, format!("Card {}", i), format!("Desc {}", i))
            .await
            .unwrap();
        cards.push(card);
    }
    (board.id, column.id, cards)
}

// ============================================================
// 6.1 동일 카드 동시 수정 (Optimistic Locking 검증)
// ============================================================

#[tokio::test]
async fn test_concurrent_card_update_version_conflict() {
    // 상황: 사용자 A와 B가 같은 카드를 동시에 수정
    // 기대: 먼저 완료된 수정은 성공, 나중 수정은 VERSION_CONFLICT
    let state = AppState::new();
    let (_, _col_id, cards) = setup_board_with_cards(&state, 1).await;
    let card = &cards[0];
    let card_id = card.id.clone();
    let initial_version = card.version; // v1

    let state_a = state.clone();
    let state_b = state.clone();
    let id_a = card_id.clone();
    let id_b = card_id.clone();

    // 사용자 A: version 1로 수정 시도
    let handle_a = tokio::spawn(async move {
        state_a
            .update_card(
                &id_a,
                UpdateCardRequest {
                    title: Some("Updated by A".into()),
                    description: None,
                    status: None,
                    version: initial_version,
                    actor_nickname: None,
                },
            )
            .await
    });

    // 사용자 B: 동일한 version 1로 수정 시도
    let handle_b = tokio::spawn(async move {
        state_b
            .update_card(
                &id_b,
                UpdateCardRequest {
                    title: Some("Updated by B".into()),
                    description: None,
                    status: None,
                    version: initial_version,
                    actor_nickname: None,
                },
            )
            .await
    });

    let result_a = handle_a.await.unwrap();
    let result_b = handle_b.await.unwrap();

    // 둘 중 하나는 성공, 하나는 충돌
    let success_count = [&result_a, &result_b].iter().filter(|r| r.is_ok()).count();
    let conflict_count = [&result_a, &result_b]
        .iter()
        .filter(|r| matches!(r, Err(collab_board::errors::AppError::VersionConflict { .. })))
        .count();

    assert_eq!(success_count, 1, "정확히 하나의 수정만 성공해야 함");
    assert_eq!(conflict_count, 1, "정확히 하나의 수정은 충돌이어야 함");

    // 최종 카드 버전 확인
    let final_card = state.get_card(&card_id).await.unwrap();
    assert_eq!(final_card.version, 2, "버전이 1 증가해야 함");
}

#[tokio::test]
async fn test_sequential_card_updates_succeed() {
    // 순차적 수정은 모두 성공해야 함
    let state = AppState::new();
    let (_, _, cards) = setup_board_with_cards(&state, 1).await;
    let card_id = cards[0].id.clone();

    // 첫 번째 수정 (v1 → v2)
    let updated = state
        .update_card(
            &card_id,
            UpdateCardRequest {
                title: Some("First update".into()),
                description: None,
                status: None,
                version: 1,
                actor_nickname: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.version, 2);

    // 두 번째 수정 (v2 → v3)
    let updated = state
        .update_card(
            &card_id,
            UpdateCardRequest {
                title: Some("Second update".into()),
                description: None,
                status: None,
                version: 2,
                actor_nickname: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.version, 3);
    assert_eq!(updated.title, "Second update");
}

// ============================================================
// 6.2 카드 이동과 삭제 동시 발생
// ============================================================

#[tokio::test]
async fn test_concurrent_move_and_delete() {
    // 상황: 한 사용자는 카드 이동, 다른 사용자는 카드 삭제
    // 기대: 하나는 성공, 다른 하나는 CardNotFound
    let state = AppState::new();
    let board = state.create_board("Board".into()).await;
    let col_a = state.create_column(&board.id, "Col A".into()).await.unwrap();
    let col_b = state.create_column(&board.id, "Col B".into()).await.unwrap();
    let card = state
        .create_card(&col_a.id, "Card".into(), "".into())
        .await
        .unwrap();

    let card_id = card.id.clone();
    let version = card.version;

    let state_move = state.clone();
    let state_delete = state.clone();
    let id_move = card_id.clone();
    let id_delete = card_id.clone();
    let target_col = col_b.id.clone();

    let handle_move = tokio::spawn(async move {
        state_move
            .move_card(
                &id_move,
                MoveCardRequest {
                    target_column_id: target_col,
                    target_position: 0,
                    version,
                    actor_nickname: None,
                },
            )
            .await
    });

    let handle_delete = tokio::spawn(async move {
        state_delete.delete_card(&id_delete).await
    });

    let result_move = handle_move.await.unwrap();
    let result_delete = handle_delete.await.unwrap();

    // 둘 중 하나만 성공
    let total_success = result_move.is_ok() as usize + result_delete.is_ok() as usize;
    assert!(
        total_success >= 1,
        "최소 하나의 작업은 성공해야 함"
    );

    // 카드가 삭제되었으면 조회 불가
    if result_delete.is_ok() {
        assert!(state.get_card(&card_id).await.is_err());
    }
}

// ============================================================
// 6.3 동일 컬럼에 카드 동시 추가
// ============================================================

#[tokio::test]
async fn test_concurrent_card_creation_position_integrity() {
    // 상황: 여러 사용자가 동시에 같은 컬럼에 카드 삽입
    // 기대: 모든 카드가 고유한 position을 가져야 함
    let state = AppState::new();
    let board = state.create_board("Board".into()).await;
    let column = state.create_column(&board.id, "Column".into()).await.unwrap();
    let col_id = column.id.clone();

    let num_tasks = 20;
    let mut handles = Vec::new();

    for i in 0..num_tasks {
        let s = state.clone();
        let cid = col_id.clone();
        handles.push(tokio::spawn(async move {
            s.create_card(&cid, format!("Card {}", i), "".into()).await
        }));
    }

    let mut created_cards = Vec::new();
    for h in handles {
        let result = h.await.unwrap();
        assert!(result.is_ok(), "모든 카드 생성이 성공해야 함");
        created_cards.push(result.unwrap());
    }

    // position 고유성 검증
    let mut positions: Vec<i32> = created_cards.iter().map(|c| c.position).collect();
    positions.sort();
    positions.dedup();
    assert_eq!(
        positions.len(),
        num_tasks,
        "모든 카드의 position이 고유해야 함 (중복 없음)"
    );

    // position 연속성 검증 (0, 1, 2, ..., N-1)
    for (i, pos) in positions.iter().enumerate() {
        assert_eq!(*pos, i as i32, "position이 연속적이어야 함");
    }
}

// ============================================================
// 6.4 카드 순서 동시 변경
// ============================================================

#[tokio::test]
async fn test_concurrent_reorder_version_conflict() {
    // 상황: 여러 사용자가 동시에 같은 카드의 순서를 변경
    // 기대: 하나만 성공, 나머지는 VERSION_CONFLICT
    let state = AppState::new();
    let (_, _col_id, cards) = setup_board_with_cards(&state, 5).await;
    let target_card = &cards[2]; // position 2인 카드
    let card_id = target_card.id.clone();
    let version = target_card.version;

    let state1 = state.clone();
    let state2 = state.clone();
    let id1 = card_id.clone();
    let id2 = card_id.clone();

    // 사용자 1: position 0으로 이동
    let h1 = tokio::spawn(async move {
        state1
            .reorder_card(
                &id1,
                ReorderCardRequest {
                    target_position: 0,
                    version,
                    actor_nickname: None,
                },
            )
            .await
    });

    // 사용자 2: position 4로 이동
    let h2 = tokio::spawn(async move {
        state2
            .reorder_card(
                &id2,
                ReorderCardRequest {
                    target_position: 4,
                    version,
                    actor_nickname: None,
                },
            )
            .await
    });

    let r1 = h1.await.unwrap();
    let r2 = h2.await.unwrap();

    let success = r1.is_ok() as usize + r2.is_ok() as usize;
    assert_eq!(success, 1, "정확히 하나의 순서 변경만 성공해야 함");
}

// ============================================================
// 일관성 테스트
// ============================================================

#[tokio::test]
async fn test_position_consistency_after_delete() {
    // 카드 삭제 후 position이 연속적으로 재정렬되는지 확인
    let state = AppState::new();
    let (board_id, _col_id, cards) = setup_board_with_cards(&state, 5).await;

    // 중간 카드 (position 2) 삭제
    state.delete_card(&cards[2].id).await.unwrap();

    let detail = state.get_board_detail(&board_id).await.unwrap();
    let remaining_cards = &detail.columns[0].cards;

    assert_eq!(remaining_cards.len(), 4);

    // position이 0, 1, 2, 3으로 연속적인지 확인
    for (i, card) in remaining_cards.iter().enumerate() {
        assert_eq!(
            card.position, i as i32,
            "삭제 후 position이 연속적이어야 함"
        );
    }
}

#[tokio::test]
async fn test_deleted_card_not_accessible() {
    // 삭제된 카드에 접근 불가 확인
    let state = AppState::new();
    let (_, _, cards) = setup_board_with_cards(&state, 1).await;
    let card_id = cards[0].id.clone();

    state.delete_card(&card_id).await.unwrap();

    // 조회 시도
    assert!(state.get_card(&card_id).await.is_err());

    // 수정 시도
    assert!(state
        .update_card(
            &card_id,
            UpdateCardRequest {
                title: Some("Ghost".into()),
                description: None,
                status: None,
                version: 1,
                actor_nickname: None,
            },
        )
        .await
        .is_err());

    // 이동 시도
    assert!(state
        .move_card(
            &card_id,
            MoveCardRequest {
                target_column_id: "any".into(),
                target_position: 0,
                version: 1,
                actor_nickname: None,
            },
        )
        .await
        .is_err());
}

#[tokio::test]
async fn test_column_delete_cascades_cards() {
    // 컬럼 삭제 시 해당 컬럼의 모든 카드도 삭제
    let state = AppState::new();
    let (_, col_id, cards) = setup_board_with_cards(&state, 3).await;

    state.delete_column(&col_id).await.unwrap();

    for card in &cards {
        assert!(
            state.get_card(&card.id).await.is_err(),
            "컬럼 삭제 시 카드도 삭제되어야 함"
        );
    }
}

#[tokio::test]
async fn test_move_card_position_integrity() {
    // 카드 이동 후 양쪽 컬럼의 position 정합성 확인
    let state = AppState::new();
    let board = state.create_board("Board".into()).await;
    let col_a = state.create_column(&board.id, "A".into()).await.unwrap();
    let col_b = state.create_column(&board.id, "B".into()).await.unwrap();

    // Col A에 카드 3개 생성
    let _c0 = state.create_card(&col_a.id, "C0".into(), "".into()).await.unwrap();
    let c1 = state.create_card(&col_a.id, "C1".into(), "".into()).await.unwrap();
    let _c2 = state.create_card(&col_a.id, "C2".into(), "".into()).await.unwrap();

    // Col B에 카드 2개 생성
    let _b0 = state.create_card(&col_b.id, "B0".into(), "".into()).await.unwrap();
    let _b1 = state.create_card(&col_b.id, "B1".into(), "".into()).await.unwrap();

    // C1 (position 1)을 Col B의 position 1로 이동
    state
        .move_card(
            &c1.id,
            MoveCardRequest {
                target_column_id: col_b.id.clone(),
                target_position: 1,
                version: c1.version,
                actor_nickname: None,
            },
        )
        .await
        .unwrap();

    let detail = state.get_board_detail(&board.id).await.unwrap();

    // Col A: C0(0), C2(1) → 2개, position 연속
    let col_a_cards = &detail.columns[0].cards;
    assert_eq!(col_a_cards.len(), 2);
    assert_eq!(col_a_cards[0].position, 0);
    assert_eq!(col_a_cards[1].position, 1);

    // Col B: B0(0), C1(1), B1(2) → 3개, position 연속
    let col_b_cards = &detail.columns[1].cards;
    assert_eq!(col_b_cards.len(), 3);
    for (i, card) in col_b_cards.iter().enumerate() {
        assert_eq!(card.position, i as i32);
    }
}

// ============================================================
// 부하 테스트
// ============================================================

#[tokio::test]
async fn test_high_concurrency_mixed_operations() {
    // 다수의 동시 요청 (생성, 수정, 삭제 혼합)에서 데이터 무결성 유지
    let state = AppState::new();
    let board = state.create_board("Load Test".into()).await;
    let column = state.create_column(&board.id, "Col".into()).await.unwrap();
    let col_id = column.id.clone();

    // Phase 1: 50개 카드 동시 생성
    let mut create_handles = Vec::new();
    for i in 0..50 {
        let s = state.clone();
        let cid = col_id.clone();
        create_handles.push(tokio::spawn(async move {
            s.create_card(&cid, format!("Card {}", i), "".into()).await
        }));
    }

    let mut card_ids = Vec::new();
    for h in create_handles {
        let card = h.await.unwrap().unwrap();
        card_ids.push(card.id);
    }

    assert_eq!(card_ids.len(), 50);

    // Phase 2: 처음 10개 카드 동시 삭제
    let mut delete_handles = Vec::new();
    for id in card_ids.iter().take(10) {
        let s = state.clone();
        let cid = id.clone();
        delete_handles.push(tokio::spawn(async move {
            s.delete_card(&cid).await
        }));
    }

    for h in delete_handles {
        h.await.unwrap().unwrap();
    }

    // 최종 검증: 40개 카드 남아있어야 함
    let detail = state.get_board_detail(&board.id).await.unwrap();
    let remaining = &detail.columns[0].cards;
    assert_eq!(remaining.len(), 40, "삭제 후 40개 카드가 남아야 함");

    // position 연속성 검증
    for (i, card) in remaining.iter().enumerate() {
        assert_eq!(
            card.position, i as i32,
            "부하 테스트 후에도 position이 연속적이어야 함"
        );
    }
}

// ============================================================
// 카드 상태 변경 테스트
// ============================================================

#[tokio::test]
async fn test_card_default_status_is_todo() {
    // 카드 생성 시 기본 상태는 미진행(Todo)
    let state = AppState::new();
    let (_, _, cards) = setup_board_with_cards(&state, 1).await;
    assert_eq!(cards[0].status, CardStatus::Todo);
}

#[tokio::test]
async fn test_card_status_transitions() {
    // 상태 전환: Todo → InProgress → Done
    let state = AppState::new();
    let (_, _, cards) = setup_board_with_cards(&state, 1).await;
    let card_id = cards[0].id.clone();

    // Todo → InProgress
    let updated = state
        .update_card_status(
            &card_id,
            UpdateCardStatusRequest {
                status: CardStatus::InProgress,
                version: 1,
                actor_nickname: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.status, CardStatus::InProgress);
    assert_eq!(updated.version, 2);

    // InProgress → Done
    let updated = state
        .update_card_status(
            &card_id,
            UpdateCardStatusRequest {
                status: CardStatus::Done,
                version: 2,
                actor_nickname: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.status, CardStatus::Done);
    assert_eq!(updated.version, 3);

    // Done → Todo (되돌리기)
    let updated = state
        .update_card_status(
            &card_id,
            UpdateCardStatusRequest {
                status: CardStatus::Todo,
                version: 3,
                actor_nickname: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.status, CardStatus::Todo);
    assert_eq!(updated.version, 4);
}

#[tokio::test]
async fn test_concurrent_status_update_version_conflict() {
    // 동시에 같은 카드의 상태를 변경하면 하나만 성공
    let state = AppState::new();
    let (_, _, cards) = setup_board_with_cards(&state, 1).await;
    let card_id = cards[0].id.clone();
    let version = cards[0].version;

    let state1 = state.clone();
    let state2 = state.clone();
    let id1 = card_id.clone();
    let id2 = card_id.clone();

    let h1 = tokio::spawn(async move {
        state1
            .update_card_status(
                &id1,
                UpdateCardStatusRequest {
                    status: CardStatus::InProgress,
                    version,
                    actor_nickname: None,
                },
            )
            .await
    });

    let h2 = tokio::spawn(async move {
        state2
            .update_card_status(
                &id2,
                UpdateCardStatusRequest {
                    status: CardStatus::Done,
                    version,
                    actor_nickname: None,
                },
            )
            .await
    });

    let r1 = h1.await.unwrap();
    let r2 = h2.await.unwrap();

    let success = r1.is_ok() as usize + r2.is_ok() as usize;
    assert_eq!(success, 1, "동시 상태 변경 시 하나만 성공해야 함");

    let final_card = state.get_card(&card_id).await.unwrap();
    assert_eq!(final_card.version, 2);
    assert!(
        final_card.status == CardStatus::InProgress || final_card.status == CardStatus::Done,
        "성공한 쪽의 상태가 반영되어야 함"
    );
}

#[tokio::test]
async fn test_update_card_with_status_field() {
    // update_card API로도 상태 변경 가능
    let state = AppState::new();
    let (_, _, cards) = setup_board_with_cards(&state, 1).await;
    let card_id = cards[0].id.clone();

    let updated = state
        .update_card(
            &card_id,
            UpdateCardRequest {
                title: Some("Updated title".into()),
                description: None,
                status: Some(CardStatus::InProgress),
                version: 1,
                actor_nickname: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(updated.title, "Updated title");
    assert_eq!(updated.status, CardStatus::InProgress);
    assert_eq!(updated.version, 2);
}

// ============================================================
// HTTP API 통합 테스트
// ============================================================

#[tokio::test]
async fn test_api_board_crud() {
    let state = AppState::new();
    let app = collab_board::create_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // 서버 시작 대기
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let base = format!("http://{}", addr);

    // 보드 생성
    let res: serde_json::Value = client
        .post(format!("{}/api/boards", base))
        .json(&serde_json::json!({"title": "API Test Board"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let board_id = res["id"].as_str().unwrap().to_string();
    assert_eq!(res["title"], "API Test Board");

    // 보드 목록 조회
    let boards: Vec<serde_json::Value> = client
        .get(format!("{}/api/boards", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(boards.len(), 1);

    // 컬럼 생성
    let col: serde_json::Value = client
        .post(format!("{}/api/boards/{}/columns", base, board_id))
        .json(&serde_json::json!({"title": "To Do"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let col_id = col["id"].as_str().unwrap().to_string();

    // 카드 생성
    let card: serde_json::Value = client
        .post(format!("{}/api/columns/{}/cards", base, col_id))
        .json(&serde_json::json!({"title": "Task 1", "description": "Do something"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let card_id = card["id"].as_str().unwrap().to_string();
    assert_eq!(card["version"], 1);
    assert_eq!(card["status"], "todo"); // 기본 상태: 미진행

    // 카드 상태 변경 (todo → in_progress)
    let status_updated: serde_json::Value = client
        .patch(format!("{}/api/cards/{}/status", base, card_id))
        .json(&serde_json::json!({"status": "in_progress", "version": 1}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status_updated["status"], "in_progress");
    assert_eq!(status_updated["version"], 2);

    // 카드 수정 (version은 이제 2)
    let updated: serde_json::Value = client
        .put(format!("{}/api/cards/{}", base, card_id))
        .json(&serde_json::json!({"title": "Task 1 Updated", "version": 2}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(updated["version"], 3);
    assert_eq!(updated["title"], "Task 1 Updated");
    assert_eq!(updated["status"], "in_progress"); // 상태 유지

    // 버전 충돌 테스트 (이전 버전으로 수정 시도)
    let conflict_res = client
        .put(format!("{}/api/cards/{}", base, card_id))
        .json(&serde_json::json!({"title": "Stale update", "version": 1}))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict_res.status(), 409); // Conflict

    // 보드 상세 조회
    let detail: serde_json::Value = client
        .get(format!("{}/api/boards/{}/detail", base, board_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["columns"][0]["cards"].as_array().unwrap().len(), 1);

    // 카드 삭제
    let del_res = client
        .delete(format!("{}/api/cards/{}", base, card_id))
        .send()
        .await
        .unwrap();
    assert_eq!(del_res.status(), 204); // No Content

    // 삭제 후 조회 실패
    let not_found = client
        .get(format!("{}/api/cards/{}", base, card_id))
        .send()
        .await
        .unwrap();
    assert_eq!(not_found.status(), 404);

    server.abort();
}

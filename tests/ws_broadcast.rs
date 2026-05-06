//! WebSocket 브로드캐스트 일관성·순서 검증 테스트
//!
//! 10개 클라이언트를 동시 접속시킨 뒤 HTTP로 카드를 순차 생성하고,
//! 모든 클라이언트가 동일한 수의 이벤트를 동일한 순서로 수신하는지 검증한다.
//!
//! 순서 보장 근거: emit()은 write lock을 보유한 채로 호출되므로
//! 뮤테이션 직렬화 순서 = 브로드캐스트 발송 순서가 항상 일치한다.

use std::time::Duration;

use futures_util::StreamExt;
use serde_json::Value;
use tokio_tungstenite::connect_async;

const NUM_CLIENTS: usize = 10;
const NUM_CARDS: usize = 8;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_broadcast_consistency_and_order() {
    // ── 서버 시작 (임의 포트) ──────────────────────────────────────────
    let state = collab_board::AppState::new();
    let app = collab_board::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let http_base = format!("http://{}/api", addr);
    let ws_url = format!("ws://{}/ws", addr);
    let http = reqwest::Client::new();

    // ── 보드 + 컬럼 생성 ──────────────────────────────────────────────
    let board: Value = http
        .post(format!("{}/boards", http_base))
        .json(&serde_json::json!({"title": "ws-test-board"}))
        .send().await.unwrap()
        .json().await.unwrap();
    let board_id = board["id"].as_str().unwrap();

    let col: Value = http
        .post(format!("{}/boards/{}/columns", http_base, board_id))
        .json(&serde_json::json!({"title": "col"}))
        .send().await.unwrap()
        .json().await.unwrap();
    let col_id = col["id"].as_str().unwrap().to_string();

    // ── WS 클라이언트 NUM_CLIENTS개 연결 ─────────────────────────────
    let mut ws_streams = Vec::with_capacity(NUM_CLIENTS);
    for i in 0..NUM_CLIENTS {
        let (ws, _) = connect_async(&ws_url)
            .await
            .unwrap_or_else(|e| panic!("client {i} connect failed: {e}"));
        let (_, read) = ws.split();
        ws_streams.push(read);
    }

    // 연결 안정화 대기
    tokio::time::sleep(Duration::from_millis(150)).await;

    // ── HTTP로 카드 NUM_CARDS개 순차 생성 ────────────────────────────
    let mut expected_titles: Vec<String> = Vec::with_capacity(NUM_CARDS);
    for i in 0..NUM_CARDS {
        let title = format!("card-{:02}", i);
        expected_titles.push(title.clone());
        http.post(format!("{}/columns/{}/cards", http_base, col_id))
            .json(&serde_json::json!({"title": title}))
            .send().await.unwrap();
    }

    // ── 각 클라이언트에서 card_created 이벤트 수집 ───────────────────
    let collect_timeout = Duration::from_secs(5);

    let handles: Vec<_> = ws_streams
        .into_iter()
        .enumerate()
        .map(|(idx, mut stream)| {
            let n = NUM_CARDS;
            tokio::spawn(async move {
                let mut titles: Vec<String> = Vec::new();
                let result = tokio::time::timeout(collect_timeout, async {
                    while titles.len() < n {
                        match stream.next().await {
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                let v: Value = serde_json::from_str(&text)
                                    .expect("invalid JSON from server");
                                if v["event"] == "card_created" {
                                    let title = v["card"]["title"]
                                        .as_str()
                                        .expect("missing card.title")
                                        .to_string();
                                    titles.push(title);
                                }
                            }
                            None => break,
                            _ => {}
                        }
                    }
                })
                .await;

                if result.is_err() {
                    eprintln!(
                        "[client {idx}] timeout: collected {}/{n} events",
                        titles.len()
                    );
                }
                (idx, titles)
            })
        })
        .collect();

    let results: Vec<(usize, Vec<String>)> = futures_util::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.expect("collect task panicked"))
        .collect();

    // ── 검증 ─────────────────────────────────────────────────────────
    for (idx, titles) in &results {
        assert_eq!(
            titles.len(),
            NUM_CARDS,
            "client {idx}: received {}/{NUM_CARDS} events",
            titles.len()
        );
        assert_eq!(
            titles, &expected_titles,
            "client {idx}: ordering or content mismatch\n  got:      {titles:?}\n  expected: {expected_titles:?}"
        );
    }

    println!(
        "✅ {} clients × {} card_created events — all consistent and in order",
        NUM_CLIENTS, NUM_CARDS
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ws_broadcast_mixed_events() {
    // ── 서버 시작 ─────────────────────────────────────────────────────
    let state = collab_board::AppState::new();
    let app = collab_board::create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let http_base = format!("http://{}/api", addr);
    let ws_url = format!("ws://{}/ws", addr);
    let http = reqwest::Client::new();

    // 보드 + 컬럼 2개 생성
    let board: Value = http
        .post(format!("{}/boards", http_base))
        .json(&serde_json::json!({"title": "mixed-test"}))
        .send().await.unwrap()
        .json().await.unwrap();
    let board_id = board["id"].as_str().unwrap();

    let col_a: Value = http
        .post(format!("{}/boards/{}/columns", http_base, board_id))
        .json(&serde_json::json!({"title": "A"}))
        .send().await.unwrap()
        .json().await.unwrap();
    let col_b: Value = http
        .post(format!("{}/boards/{}/columns", http_base, board_id))
        .json(&serde_json::json!({"title": "B"}))
        .send().await.unwrap()
        .json().await.unwrap();
    let col_a_id = col_a["id"].as_str().unwrap().to_string();
    let col_b_id = col_b["id"].as_str().unwrap().to_string();

    // WS 클라이언트 연결
    let mut ws_streams = Vec::with_capacity(NUM_CLIENTS);
    for _ in 0..NUM_CLIENTS {
        let (ws, _) = connect_async(&ws_url).await.unwrap();
        let (_, read) = ws.split();
        ws_streams.push(read);
    }
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 카드 생성 → 상태 변경 → 이동 순으로 이벤트 발생
    // 예상 이벤트 순서: card_created, card_status_changed, card_moved
    let card: Value = http
        .post(format!("{}/columns/{}/cards", http_base, col_a_id))
        .json(&serde_json::json!({"title": "target"}))
        .send().await.unwrap()
        .json().await.unwrap();
    let card_id = card["id"].as_str().unwrap();

    http.patch(format!("{}/cards/{}/status", http_base, card_id))
        .json(&serde_json::json!({"status": "in_progress", "version": 1}))
        .send().await.unwrap();

    http.patch(format!("{}/cards/{}/move", http_base, card_id))
        .json(&serde_json::json!({
            "target_column_id": col_b_id,
            "target_position": 0,
            "version": 2
        }))
        .send().await.unwrap();

    // 각 클라이언트에서 이벤트 3개 수집
    let expected_events = vec!["card_created", "card_status_changed", "card_moved"];

    let handles: Vec<_> = ws_streams
        .into_iter()
        .enumerate()
        .map(|(idx, mut stream)| {
            let target_id = card_id.to_string();
            tokio::spawn(async move {
                let mut events: Vec<String> = Vec::new();
                let _ = tokio::time::timeout(Duration::from_secs(5), async {
                    while events.len() < 3 {
                        if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) =
                            stream.next().await
                        {
                            let v: Value = serde_json::from_str(&text).unwrap();
                            let event = v["event"].as_str().unwrap_or("").to_string();
                            // card 관련 이벤트만 수집 (column_created 제외)
                            let is_target = v["card"]["id"].as_str() == Some(&target_id)
                                || event == "card_created";
                            if is_target && event.starts_with("card_") {
                                events.push(event);
                            }
                        }
                    }
                })
                .await;
                (idx, events)
            })
        })
        .collect();

    let results: Vec<_> = futures_util::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    for (idx, events) in &results {
        assert_eq!(
            events.len(), 3,
            "client {idx}: received {}/3 events: {events:?}",
            events.len()
        );
        assert_eq!(
            events.as_slice(),
            expected_events,
            "client {idx}: event order mismatch\n  got:      {events:?}\n  expected: {expected_events:?}"
        );
    }

    println!(
        "✅ {} clients — card_created → card_status_changed → card_moved 순서 일치",
        NUM_CLIENTS
    );
}

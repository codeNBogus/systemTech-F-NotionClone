use std::sync::Arc;
use collab_board::{create_router, AppState};
use collab_board::wal::WalWriter;

#[tokio::main]
async fn main() {
    // 1) WAL 파일 열기 (./data/wal.jsonl)
    let wal_path = "./data/wal.jsonl";

    // 2) 기존 WAL 파일에서 이벤트 replay
    let events = WalWriter::replay(wal_path).expect("WAL replay 실패");
    println!("📂 WAL replay: {} events loaded", events.len());

    // 3) WAL 파일 열기 (append 모드)
    let wal = Arc::new(
        WalWriter::open(wal_path).expect("WAL 파일 열기 실패")
    );

    // 4) WAL과 함께 AppState 생성
    let state = AppState::with_wal(wal);

    // 5) Replay된 이벤트를 state에 적용
    state.apply_events(events).await;

    // 6) 서버 실행
    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("🚀 Collab Board server running on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}

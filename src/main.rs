use std::sync::Arc;

use collab_board::models::WsEvent;
use collab_board::wal::{AuditLogWriter, WalWriter};
use collab_board::{create_router, AppState};

#[tokio::main]
async fn main() {
    let wal_path = "./data/wal.jsonl";
    let audit_path = "./data/audit.jsonl";

    let events = WalWriter::replay(wal_path).expect("WAL replay failed");
    println!("WAL replay: {} events loaded", events.len());

    let mut audit_logs = AuditLogWriter::replay(audit_path).expect("audit replay failed");
    audit_logs.extend(events.iter().filter_map(|event| match event {
        WsEvent::AuditLogged { log } => Some(log.clone()),
        _ => None,
    }));
    println!("Audit replay: {} logs loaded", audit_logs.len());

    let wal = Arc::new(WalWriter::open(wal_path).expect("WAL open failed"));
    let audit_log = Arc::new(AuditLogWriter::open(audit_path).expect("audit log open failed"));

    let state = AppState::with_wal_and_audit(wal, Some(audit_log));
    state.apply_events(events).await;
    state.apply_audit_logs(audit_logs).await;

    let app = create_router(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();
    println!("Collab Board server running on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}

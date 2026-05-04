use collab_board::{create_router, AppState};

#[tokio::main]
async fn main() {
    let state = AppState::new();
    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("🚀 Collab Board server running on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}

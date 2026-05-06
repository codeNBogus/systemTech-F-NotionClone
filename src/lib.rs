pub mod errors;
pub mod handlers;
pub mod models;
pub mod store;
pub mod ws;

pub use crate::store::AppState;

/// 라우터 생성 (테스트에서도 사용)
pub fn create_router(state: AppState) -> axum::Router {
    use axum::routing::{delete, get, patch, post};
    use axum::Router;
    use tower_http::cors::CorsLayer;
    use tower_http::services::ServeDir;

    Router::new()
        // Board routes
        .route("/api/boards", get(handlers::list_boards).post(handlers::create_board))
        .route("/api/boards/:board_id/detail", get(handlers::get_board))
        .route("/api/boards/:board_id/columns", post(handlers::create_column))
        // Column routes
        .route("/api/columns/:column_id", delete(handlers::delete_column))
        .route("/api/columns/:column_id/cards", post(handlers::create_card))
        // Card routes
        .route(
            "/api/cards/:card_id",
            get(handlers::get_card)
                .put(handlers::update_card)
                .delete(handlers::delete_card),
        )
        .route("/api/cards/:card_id/move", patch(handlers::move_card))
        .route("/api/cards/:card_id/status", patch(handlers::update_card_status))
        .route("/api/cards/:card_id/reorder", patch(handlers::reorder_card))
        .route("/api/cards/:card_id/logs", get(handlers::get_card_logs))
        .route("/ws", get(ws::ws_handler))
        .fallback_service(ServeDir::new("static"))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

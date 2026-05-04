use axum::extract::{Path, State};
use axum::Json;

use crate::errors::AppError;
use crate::models::*;
use crate::store::AppState;

// ========== Board Handlers ==========

pub async fn create_board(
    State(state): State<AppState>,
    Json(req): Json<CreateBoardRequest>,
) -> Json<Board> {
    let board = state.create_board(req.title).await;
    Json(board)
}

pub async fn list_boards(State(state): State<AppState>) -> Json<Vec<Board>> {
    Json(state.list_boards().await)
}

pub async fn get_board(
    State(state): State<AppState>,
    Path(board_id): Path<String>,
) -> Result<Json<BoardDetailResponse>, AppError> {
    let detail = state.get_board_detail(&board_id).await?;
    Ok(Json(detail))
}

// ========== Column Handlers ==========

pub async fn create_column(
    State(state): State<AppState>,
    Path(board_id): Path<String>,
    Json(req): Json<CreateColumnRequest>,
) -> Result<Json<Column>, AppError> {
    let column = state.create_column(&board_id, req.title).await?;
    Ok(Json(column))
}

pub async fn delete_column(
    State(state): State<AppState>,
    Path(column_id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    state.delete_column(&column_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

// ========== Card Handlers ==========

pub async fn create_card(
    State(state): State<AppState>,
    Path(column_id): Path<String>,
    Json(req): Json<CreateCardRequest>,
) -> Result<Json<Card>, AppError> {
    let card = state
        .create_card(&column_id, req.title, req.description)
        .await?;
    Ok(Json(card))
}

pub async fn get_card(
    State(state): State<AppState>,
    Path(card_id): Path<String>,
) -> Result<Json<Card>, AppError> {
    let card = state.get_card(&card_id).await?;
    Ok(Json(card))
}

pub async fn update_card(
    State(state): State<AppState>,
    Path(card_id): Path<String>,
    Json(req): Json<UpdateCardRequest>,
) -> Result<Json<Card>, AppError> {
    let card = state.update_card(&card_id, req).await?;
    Ok(Json(card))
}

pub async fn delete_card(
    State(state): State<AppState>,
    Path(card_id): Path<String>,
) -> Result<axum::http::StatusCode, AppError> {
    state.delete_card(&card_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn move_card(
    State(state): State<AppState>,
    Path(card_id): Path<String>,
    Json(req): Json<MoveCardRequest>,
) -> Result<Json<Card>, AppError> {
    let card = state.move_card(&card_id, req).await?;
    Ok(Json(card))
}

pub async fn update_card_status(
    State(state): State<AppState>,
    Path(card_id): Path<String>,
    Json(req): Json<UpdateCardStatusRequest>,
) -> Result<Json<Card>, AppError> {
    let card = state.update_card_status(&card_id, req).await?;
    Ok(Json(card))
}

pub async fn reorder_card(
    State(state): State<AppState>,
    Path(card_id): Path<String>,
    Json(req): Json<ReorderCardRequest>,
) -> Result<Json<Card>, AppError> {
    let card = state.reorder_card(&card_id, req).await?;
    Ok(Json(card))
}

# Rust 기반 협업 보드 시스템에서의 동시성 제어 설계 및 검증

> **과목:** 시스템 프로그래밍  
> **팀원:** 이동열 · 이재익 · 이준서 · 이해성  
> **제출일:** 2026년 5월

---

## 목차

1. [초록](#1-초록)
2. [서론](#2-서론)
3. [관련 연구](#3-관련-연구)
4. [시스템 설계](#4-시스템-설계)
5. [동시성 제어 전략](#5-동시성-제어-전략)
6. [추가 기능 구현](#6-추가-기능-구현)
7. [실험 및 검증](#7-실험-및-검증)
8. [결론](#8-결론)
9. [참고 문헌](#9-참고-문헌)

---

## 1. 초록

본 연구에서는 Rust 언어를 기반으로 다중 사용자 환경의 협업 보드 시스템을 구현하고, 동시성 제어 문제를 구조적으로 해결하는 방법을 제안한다. 구현 시스템은 보드(Board), 컬럼(Column), 카드(Card)의 계층 구조를 가지며, 여러 사용자가 동시에 접근할 때 발생할 수 있는 Race Condition을 세 가지 전략으로 예방한다.

첫째, `Arc<RwLock<...>>`을 통해 공유 상태에 대한 읽기·쓰기 접근을 분리한다. 둘째, 카드의 `version` 필드를 활용한 Optimistic Locking으로 동시 수정 충돌을 감지하고 HTTP 409 응답을 반환한다. 셋째, 카드 이동·삭제·재정렬 등 복합 연산을 단일 write lock 내에서 원자적으로 처리한다.

추가 기능으로는 카드별 수정 이력 로그(`ModificationLog`)와 WebSocket 브로드캐스트를 구현하였다. WebSocket은 모든 변경 이벤트를 실시간으로 전파하며, emit 호출을 write lock 보유 중에 수행하여 이벤트 순서의 일관성을 보장한다. 17개의 자동화 테스트를 통해 동시성 안전성, 상태 일관성, WS 브로드캐스트 순서 일치를 실증적으로 검증하였다.

**키워드:** Rust, 동시성 제어, Race Condition, Optimistic Locking, RwLock, WebSocket, 협업 시스템

---

## 2. 서론

### 2.1 연구 배경

Notion, Trello와 같은 협업 도구는 다수의 사용자가 동일한 데이터를 실시간으로 편집하는 환경을 제공한다. 이러한 환경에서는 동일한 카드를 두 사용자가 동시에 수정하거나, 카드를 이동하는 동시에 삭제를 시도하는 등의 Race Condition이 필연적으로 발생한다. Race Condition이 처리되지 않으면 데이터 손실, 순서 불일치, 시스템 불일관성 등의 문제로 이어진다.

Rust는 소유권(Ownership) 시스템과 타입 시스템을 통해 컴파일 타임에 데이터 레이스를 원천적으로 방지하며, tokio 비동기 런타임을 통해 고성능 비동기 서버 구현이 가능하다. 이러한 특성은 동시성 제어 연구 플랫폼으로서 Rust를 적합한 선택으로 만든다.

### 2.2 연구 목적

본 연구의 목적은 다음과 같다.

- 다중 사용자 협업 환경에서 발생하는 주요 Race Condition 패턴을 식별하고 분류한다.
- Rust의 언어적 특성을 활용하여 각 Race Condition을 구조적으로 해결하는 전략을 설계한다.
- 구현된 시스템을 자동화 테스트를 통해 검증하고, 해결 전략의 유효성을 실증한다.
- 동시성 문제 분석을 위한 수정 이력 추적 및 실시간 브로드캐스트 기능을 추가로 구현한다.

### 2.3 논문 구성

본 논문은 다음과 같이 구성된다. 3장에서는 관련 연구를 검토한다. 4장에서는 시스템 전체 설계를 설명한다. 5장에서는 세 가지 동시성 제어 전략을 상세히 기술한다. 6장에서는 추가 기능 구현을 설명한다. 7장에서는 실험 결과를 제시하며, 8장에서 결론을 맺는다.

---

## 3. 관련 연구

### 3.1 동시성 제어 기법

동시성 제어는 데이터베이스 시스템과 분산 시스템에서 오랫동안 연구된 분야이다. 크게 비관적 제어(Pessimistic Concurrency Control)와 낙관적 제어(Optimistic Concurrency Control)로 나뉜다.

비관적 제어는 트랜잭션이 데이터에 접근하기 전에 잠금을 획득하여 충돌을 사전에 방지한다. 반면 낙관적 제어는 충돌이 드물다고 가정하고 변경 완료 시점에 충돌 여부를 검사한다. 본 연구는 두 방식을 혼합하여 적용한다.

### 3.2 Rust의 동시성 모델

Rust는 소유권 시스템을 통해 컴파일 타임에 데이터 레이스를 방지한다. `Send`와 `Sync` 트레이트를 통해 스레드 간 데이터 전달 가능 여부를 타입 수준에서 표현하며, 이를 위반하는 코드는 컴파일 오류로 거부된다. tokio 라이브러리는 `RwLock`, `Mutex`, `broadcast` 채널 등 비동기 동시성 프리미티브를 제공한다.

### 3.3 협업 시스템의 실시간 동기화

현대 협업 도구는 WebSocket이나 Server-Sent Events를 통해 변경 사항을 실시간으로 전파한다. 이때 이벤트의 전달 순서 보장이 중요한 문제로 대두된다. 본 연구에서는 뮤테이션과 브로드캐스트를 동일한 잠금 구간 내에서 처리하는 방식으로 순서를 보장한다.

---

## 4. 시스템 설계

### 4.1 전체 아키텍처

시스템은 axum 0.7 웹 프레임워크 위에 구축된 HTTP REST API 서버로, tokio 비동기 런타임에서 동작한다. 모든 상태는 메모리 내 해시맵에 저장되며, `Arc<RwLock<StoreInner>>`로 보호된다.

```
클라이언트 (브라우저 / HTTP 클라이언트)
        │  HTTP REST + WebSocket
        ▼
   axum Router (src/lib.rs)
        │
   Handlers (src/handlers.rs)
        │
   AppState (src/store.rs)
   ┌─────────────────────────────┐
   │  Arc<RwLock<StoreInner>>    │
   │  ┌────────┬────────┬──────┐ │
   │  │ boards │columns │cards │ │
   │  └────────┴────────┴──────┘ │
   │  broadcast::Sender<...>     │
   └─────────────────────────────┘
```

### 4.2 데이터 모델

시스템은 세 가지 핵심 엔티티로 구성된다.

| 엔티티 | 주요 필드 | 설명 |
|--------|-----------|------|
| `Board` | `id`, `title`, `created_at` | 최상위 컨테이너 |
| `Column` | `id`, `board_id`, `title`, `position` | 보드 내 카드 그룹 |
| `Card` | `id`, `column_id`, `title`, `status`, `position`, `version`, `modification_logs` | 개별 작업 항목 |

`Card`의 `version` 필드는 Optimistic Locking의 핵심이며, `modification_logs`는 모든 변경 이력을 누적 저장한다.

### 4.3 API 설계

총 13개의 REST 엔드포인트와 1개의 WebSocket 엔드포인트를 제공한다.

| 메서드 | 경로 | 설명 |
|--------|------|------|
| `POST` | `/api/boards` | 보드 생성 |
| `GET` | `/api/boards` | 보드 목록 조회 |
| `GET` | `/api/boards/:id/detail` | 보드 상세 조회 |
| `POST` | `/api/boards/:id/columns` | 컬럼 생성 |
| `DELETE` | `/api/columns/:id` | 컬럼 삭제 |
| `POST` | `/api/columns/:id/cards` | 카드 생성 |
| `GET` | `/api/cards/:id` | 카드 단건 조회 |
| `PUT` | `/api/cards/:id` | 카드 수정 (Optimistic Locking) |
| `DELETE` | `/api/cards/:id` | 카드 삭제 |
| `PATCH` | `/api/cards/:id/move` | 카드 이동 |
| `PATCH` | `/api/cards/:id/reorder` | 카드 순서 변경 |
| `GET` | `/api/cards/:id/logs` | 카드 수정 이력 조회 |
| `GET` | `/ws` | WebSocket 연결 |

### 4.4 소스 파일 구성

```
src/
├── main.rs        서버 진입점 (포트 3000)
├── lib.rs         라우터 정의 및 모듈 선언
├── models.rs      데이터 모델 및 요청/응답 DTO
├── store.rs       인메모리 저장소 및 동시성 제어 핵심
├── handlers.rs    HTTP 요청 핸들러
├── errors.rs      AppError 타입 정의 (thiserror)
└── ws.rs          WebSocket 핸들러 및 브로드캐스트

tests/
├── concurrency_tests.rs   동시성 및 일관성 테스트 (15개)
└── ws_broadcast.rs        WebSocket 브로드캐스트 검증 테스트 (2개)

static/
└── index.html     웹 UI (Vanilla JS)
```

---

## 5. 동시성 제어 전략

본 시스템에서 발생 가능한 Race Condition은 크게 네 가지 유형으로 분류하였으며, 세 가지 제어 전략을 조합하여 해결한다.

### 5.1 Race Condition 유형 분류

#### 유형 1: 동일 카드 동시 수정 (Lost Update)

사용자 A와 B가 동일한 카드를 동시에 수정하는 경우, 마지막으로 저장된 데이터만 반영되어 한쪽의 변경이 소실된다.

```
A: GET card (version=1) → 수정 → PUT (version=1) → 성공 (version=2)
B: GET card (version=1) → 수정 → PUT (version=1) → ?
```

B의 요청이 도달할 시점에 카드의 version은 이미 2이므로, B가 보유한 version=1은 유효하지 않다.

#### 유형 2: 카드 이동 + 삭제 동시 발생 (Phantom Read)

한 사용자가 카드를 이동하는 동안 다른 사용자가 동일 카드를 삭제하면, 존재하지 않는 카드에 대한 이동 연산이 발생할 수 있다.

#### 유형 3: 동일 컬럼 카드 동시 추가 (Position 충돌)

여러 사용자가 동시에 같은 컬럼에 카드를 추가하면, 동일한 position 값이 부여될 수 있다.

#### 유형 4: 카드 순서 동시 변경 (Ordering Conflict)

여러 사용자가 동시에 카드 순서를 변경하면, 재정렬 연산이 서로 간섭하여 position의 연속성이 파괴될 수 있다.

### 5.2 전략 1: Arc\<RwLock\<...\>\>를 통한 공유 상태 보호

모든 데이터는 단일 `StoreInner` 구조체에 저장되며, `Arc<RwLock<StoreInner>>`로 보호된다.

```rust
pub struct AppState {
    inner: Arc<RwLock<StoreInner>>,
    tx: broadcast::Sender<Arc<String>>,
}
```

`RwLock`은 읽기 연산에 대해 다중 스레드의 동시 접근을 허용하고, 쓰기 연산에 대해서는 단일 스레드의 독점 접근을 보장한다. tokio의 비동기 `RwLock`을 사용하므로 lock 대기 중 스레드가 블로킹되지 않고 다른 작업을 처리할 수 있다.

### 5.3 전략 2: Optimistic Locking (버전 기반 충돌 감지)

카드 구조체에 `version: u64` 필드를 추가하고, 모든 수정 요청에 클라이언트가 보유한 version을 포함시킨다. 서버는 저장된 version과 요청의 version을 비교하여 불일치 시 `HTTP 409 Conflict`를 반환한다.

```rust
pub async fn update_card(&self, card_id: &str, req: UpdateCardRequest) -> Result<Card, AppError> {
    let mut store = self.inner.write().await;
    let card = store.cards.get_mut(card_id)
        .ok_or_else(|| AppError::CardNotFound(card_id.to_string()))?;

    if card.version != req.version {
        return Err(AppError::VersionConflict {
            expected: req.version,
            actual: card.version,
        });
    }
    // 수정 수행...
}
```

이 방식은 잠금을 사전에 획득하지 않고(Lock-free), 충돌 발생 시 클라이언트가 최신 데이터를 재조회 후 재시도하도록 유도한다.

### 5.4 전략 3: Atomic 처리 (단일 write lock 내 복합 연산)

카드 이동, 삭제, 생성 등 여러 데이터를 동시에 변경해야 하는 복합 연산은 단일 write lock 보유 구간 내에서 모두 처리한다. 이를 통해 연산의 중간 상태가 외부에 노출되지 않는다.

`move_card()` 연산의 처리 순서는 다음과 같다.

1. write lock 획득
2. 대상 컬럼 존재 확인
3. Optimistic Locking 버전 검사
4. 이전 컬럼의 position 재정렬 (삭제된 카드 이후 카드들 -1)
5. 새 컬럼의 삽입 공간 확보 (target position 이후 카드들 +1)
6. 카드의 `column_id`와 `position` 갱신
7. write lock 해제

위 6단계가 하나의 원자적 단위로 처리되므로, 중간 상태에서 다른 연산이 개입할 수 없다.

---

## 6. 추가 기능 구현

### 6.1 카드 수정 이력 로그

동시성 문제 발생 시 정확한 원인 분석을 위해, 카드의 모든 변경 이벤트를 이력으로 기록하는 기능을 구현하였다.

#### 데이터 구조

```rust
pub enum ModificationOperation {
    Created, Updated, StatusChanged, Moved, Reordered,
}

pub struct ModificationLog {
    pub version: u64,
    pub operation: ModificationOperation,
    #[serde(with = "kst_serde")]  // 한국 표준시(KST, +09:00)로 직렬화
    pub timestamp: DateTime<Utc>,
    pub detail: String,
}
```

#### 기록 방식

`Card::push_log()` 메서드가 version 증가, `updated_at` 갱신, 로그 추가를 원자적으로 수행한다.

```rust
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
```

#### KST 직렬화

타임스탬프는 내부적으로 UTC로 저장되나, API 응답 직렬화 시 KST(+09:00)로 변환된다. 커스텀 serde 모듈(`kst_serde`)을 통해 구현하여 저장 계층과 표현 계층을 분리한다.

#### 조회 API

`GET /api/cards/:id/logs` 엔드포인트로 특정 카드의 전체 이력을 조회할 수 있다.

```json
[
  {
    "version": 1,
    "operation": "created",
    "timestamp": "2026-05-06T13:57:46+09:00",
    "detail": "카드 생성"
  },
  {
    "version": 3,
    "operation": "status_changed",
    "timestamp": "2026-05-06T13:58:26+09:00",
    "detail": "status → 진행중"
  }
]
```

### 6.2 WebSocket 브로드캐스트

카드·컬럼의 변경 이벤트를 연결된 모든 클라이언트에 실시간으로 전파하는 기능을 구현하였다.

#### 아키텍처

`AppState`에 `tokio::sync::broadcast::Sender<Arc<String>>`를 추가하고, 각 WebSocket 클라이언트 연결 시 `subscribe()`를 통해 수신자(`Receiver`)를 생성한다.

```rust
pub struct AppState {
    inner: Arc<RwLock<StoreInner>>,
    tx: broadcast::Sender<Arc<String>>,
}
```

#### 이벤트 타입

```rust
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WsEvent {
    CardCreated { card: Card },
    CardUpdated { card: Card },
    CardDeleted { card_id: String },
    CardMoved { card: Card },
    CardStatusChanged { card: Card },
    CardReordered { card: Card },
    ColumnCreated { column: Column },
    ColumnDeleted { column_id: String },
}
```

#### 순서 보장 메커니즘

`emit()` 호출을 write lock 보유 구간 내에서 수행한다. `broadcast::Sender::send()`는 동기 함수이므로 비동기 lock 구간 안에서 호출 가능하다.

```
write lock 획득
  → 상태 변경 (뮤테이션)
  → emit() 호출 (브로드캐스트)
write lock 해제
```

이 구조에서 뮤테이션의 직렬화 순서와 브로드캐스트 발송 순서가 항상 일치한다. 만약 emit()을 write lock 밖에서 호출하면, 두 뮤테이션 사이에 발송 순서가 역전될 수 있다.

---

## 7. 실험 및 검증

### 7.1 테스트 환경

| 항목 | 내용 |
|------|------|
| 언어 | Rust 2021 edition |
| 웹 프레임워크 | axum 0.7 |
| 비동기 런타임 | tokio (multi-thread, 4 workers) |
| 테스트 프레임워크 | Rust 내장 테스트 + tokio::test |
| HTTP 클라이언트 (테스트용) | reqwest 0.12 |
| WS 클라이언트 (테스트용) | tokio-tungstenite 0.23 |

### 7.2 테스트 카테고리 및 결과

총 17개의 자동화 테스트를 작성하였으며, 전체 통과를 확인하였다.

| 카테고리 | 테스트 수 | 주요 검증 항목 |
|----------|-----------|----------------|
| 동시성 테스트 | 5 | Version conflict, 동시 이동·삭제, position 충돌, 동시 reorder |
| 상태 전환 테스트 | 4 | 기본 상태, 상태 전환, 동시 상태 변경, 상태 필드 포함 수정 |
| 일관성 테스트 | 3 | 삭제 후 position 연속성, 삭제된 카드 접근 불가, 컬럼 삭제 cascade |
| WS 브로드캐스트 테스트 | 2 | 일관성·순서 검증, 혼합 이벤트 순서 검증 |
| HTTP 통합 테스트 | 2 | Board CRUD 실제 HTTP 요청 검증 |
| 부하 테스트 | 1 | 50개 동시 생성 + 10개 동시 삭제 |

### 7.3 주요 테스트 상세

#### 동시성 테스트: concurrent_card_update_version_conflict

10개의 tokio 태스크가 동일한 카드(version=1)를 동시에 수정한다. 기대 결과는 정확히 1개의 요청만 성공하고 나머지 9개는 `VersionConflict` 오류를 반환하는 것이다.

```
실행 결과: success=1, conflict=9 ✅
```

#### 동시성 테스트: concurrent_card_creation_position_integrity

20개의 태스크가 동일 컬럼에 동시에 카드를 추가한다. 기대 결과는 20개 카드의 position 값이 0~19의 중복 없는 연속 정수를 형성하는 것이다.

```
실행 결과: position 집합 = {0, 1, 2, ..., 19} ✅
```

#### 부하 테스트: high_concurrency_mixed_operations

50개의 태스크가 동시에 카드를 생성하고, 10개의 태스크가 동시에 삭제를 시도한다. 연산 완료 후 잔존 카드의 position이 연속성을 유지하는지 검증한다.

```
실행 결과: 잔존 카드 position 정합성 유지 ✅
```

#### WS 테스트: ws_broadcast_consistency_and_order

10개 클라이언트가 WebSocket에 동시 접속한 상태에서, HTTP 요청으로 카드 8개를 순차 생성한다. 모든 클라이언트가 8개의 `card_created` 이벤트를 동일한 순서로 수신하는지 검증한다.

```
실행 결과: 10 clients × 8 events, 모든 클라이언트 순서 일치 ✅
```

#### WS 테스트: ws_broadcast_mixed_events

단일 카드에 생성 → 상태 변경 → 이동 순서로 연산을 수행하고, 10개 클라이언트가 `card_created → card_status_changed → card_moved` 순서로 이벤트를 수신하는지 검증한다.

```
실행 결과: 10개 클라이언트 모두 이벤트 순서 일치 ✅
```

### 7.4 테스트 실행 방법

```bash
# 전체 테스트 실행
cargo test

# 동시성 테스트만 실행
cargo test --test concurrency_tests

# WebSocket 브로드캐스트 테스트 실행
cargo test --test ws_broadcast -- --nocapture
```

---

## 8. 결론

본 연구에서는 Rust 기반 협업 보드 시스템을 구현하고, 다중 사용자 환경의 동시성 문제를 세 가지 전략으로 해결하였다.

**첫째**, `Arc<RwLock<...>>`를 통해 공유 상태의 읽기·쓰기 접근을 분리하고 상호 배제를 보장하였다.

**둘째**, 카드의 `version` 필드를 활용한 Optimistic Locking으로 동시 수정 충돌을 Lock-free 방식으로 감지하였다. 이 방식은 대기 없이 즉시 충돌 여부를 판단하므로 시스템 처리량에 미치는 영향을 최소화한다.

**셋째**, 복합 연산(이동, 삭제, 재정렬)을 단일 write lock 구간 내에서 원자적으로 처리하여 중간 상태 노출을 방지하였다.

추가 기능으로 구현한 **카드 수정 이력 로그**는 동시성 문제 발생 시 정확한 추적을 가능하게 하며, **WebSocket 브로드캐스트**는 변경 이벤트를 실시간으로 모든 클라이언트에 전파한다. 특히, emit()을 write lock 구간 내에서 호출하는 설계를 통해 이벤트 순서의 일관성을 구조적으로 보장하였다.

17개의 자동화 테스트를 통해 모든 Race Condition 시나리오가 의도대로 처리됨을 실증하였다. Rust의 타입 시스템과 소유권 모델이 동시성 버그를 컴파일 타임에 차단함으로써, 런타임에서 발생할 수 있는 오류의 범위를 크게 줄일 수 있었다.

향후 연구 방향으로는 인메모리 저장소를 영속성 데이터베이스(PostgreSQL 등)로 교체하고, WebSocket을 통한 클라이언트-서버 양방향 명령 처리, 그리고 Operational Transformation 또는 CRDT 기반의 충돌 없는 동시 편집 기능 구현을 고려할 수 있다.

---

## 9. 참고 문헌

1. Klabnik, S., & Nichols, C. (2022). *The Rust Programming Language* (2nd ed.). No Starch Press.
2. tokio contributors. (2024). *tokio — An asynchronous Rust runtime*. https://tokio.rs
3. axum contributors. (2024). *axum — Ergonomic and modular web framework built with Tokio, Tower, and Hyper*. https://github.com/tokio-rs/axum
4. Bernstein, P. A., & Goodman, N. (1981). Concurrency control in distributed database systems. *ACM Computing Surveys*, 13(2), 185–221.
5. Herlihy, M., & Wing, J. M. (1990). Linearizability: A correctness condition for concurrent objects. *ACM Transactions on Programming Languages and Systems*, 12(3), 463–492.
6. RFC 6455. (2011). *The WebSocket Protocol*. IETF.

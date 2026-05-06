# 🦀 Rust 기반 Notion 스타일 협업 보드 클론 시스템

## 📋 프로젝트 개요

여러 사용자가 동시에 보드, 컬럼, 카드에 접근하고 수정할 수 있는 **협업 보드 시스템**을 Rust로 구현하는 프로젝트입니다.

Notion 및 Trello의 보드 기능을 참고하여 카드 생성/수정/삭제, 컬럼 이동, 카드 순서 변경 등의 기능을 제공하며, 특히 **동시 접근 상황에서 발생하는 Race Condition을 분석하고 해결하는 것**에 초점을 둡니다.

## 🎯 프로젝트 목표

- Rust를 활용한 서버 기반 협업 시스템 구현
- 다중 사용자 환경에서의 동시성 문제 이해 및 해결
- Race Condition 발생 사례 분석
- Lock, Version Control 등의 기법을 활용한 해결
- 테스트를 통한 시스템의 일관성 및 안정성 검증

## ⚙️ 주요 기능

### 기본 기능
- 보드 생성 및 조회
- 컬럼 생성 및 관리
- 카드 생성 / 수정 / 삭제
- 카드 컬럼 간 이동
- 카드 순서 변경

### 협업 기능
- 여러 사용자의 동시 접속
- 동일 자원 동시 수정 처리
- 변경 사항 실시간 반영

### 카드 수정 이력 로그 (추가 기능)
- 카드 생성·수정·상태변경·이동·순서변경 시 자동으로 이력 기록
- 각 이력 항목에 버전 번호, 작업 종류, 변경 시간(KST), 변경 내용 포함
- `GET /api/cards/:card_id/logs` API로 전체 이력 조회 가능
- 웹 UI에서 카드 hover 시 🕐 버튼을 클릭해 수정 이력 모달로 확인 가능
- 동시성 문제 발생 시 어느 시점에 어떤 변경이 일어났는지 추적하는 데 활용

### WebSocket 브로드캐스트 (추가 기능)
- 카드·컬럼 변경 시 연결된 모든 클라이언트에 실시간 이벤트 전송
- 이벤트 종류: `card_created`, `card_updated`, `card_deleted`, `card_moved`, `card_status_changed`, `card_reordered`, `column_created`, `column_deleted`
- `emit()`을 write lock 안에서 호출하여 뮤테이션 순서 = 브로드캐스트 순서 보장
- `/ws` 엔드포인트로 WebSocket 연결 (`ws://localhost:3000/ws`)

#### 테스트 실행 방법

**전체 WS 테스트 실행**
```bash
cargo test --test ws_broadcast -- --nocapture
```

**테스트별 개별 실행**
```bash
# 테스트 1: 10개 클라이언트 일관성·순서 검증
cargo test --test ws_broadcast ws_broadcast_consistency_and_order -- --nocapture

# 테스트 2: 혼합 이벤트 순서 검증
cargo test --test ws_broadcast ws_broadcast_mixed_events -- --nocapture
```

#### 확인 포인트

**테스트 1 — `ws_broadcast_consistency_and_order`**

10개 클라이언트가 카드 8개를 빠짐없이, 동일한 순서로 수신했는지 검증한다.

정상 출력:
```
✅ 10 clients × 8 card_created events — all consistent and in order
```

실패 시 출력 예시:
```
# 일부 이벤트 누락
client 3: received 6/8 events

# 수신 순서 불일치
client 5: ordering mismatch
  got:      ["card-00", "card-02", "card-01", ...]
  expected: ["card-00", "card-01", "card-02", ...]
```

**테스트 2 — `ws_broadcast_mixed_events`**

카드 하나에 대해 생성→상태변경→이동 이벤트가 항상 이 순서로 전달되는지 검증한다.

정상 출력:
```
✅ 10 clients — card_created → card_status_changed → card_moved 순서 일치
```

실패 시 출력 예시:
```
client 7: event order mismatch
  got:      ["card_created", "card_moved", "card_status_changed"]
  expected: ["card_created", "card_status_changed", "card_moved"]
```

#### 순서 보장 원리 확인

`src/store.rs`에서 `emit()`의 위치가 순서를 결정한다.

| 위치 | 결과 |
|------|------|
| write lock **안** (현재) | 뮤테이션 순서 = 브로드캐스트 순서 → 테스트 통과 |
| write lock **밖**으로 이동 | 동시 요청 시 발송 순서 역전 가능 → 테스트 간헐적 실패 |

## 🏗️ 시스템 구조

### 백엔드
- **언어**: Rust
- **웹 프레임워크**: axum 또는 actix-web
- **비동기 처리**: tokio

### 데이터 관리
- **초기**: 메모리 기반 (`Arc<Mutex<...>>`)
- **확장**: SQLite 또는 PostgreSQL (선택)

### 클라이언트
- 간단한 웹 UI (HTML/JS 또는 React)
- 또는 CLI 기반 인터페이스

## 🔒 동시성 및 Race Condition 분석 (핵심)

본 프로젝트에서는 다음과 같은 동시성 문제를 의도적으로 발생시키고 분석합니다.

### 1. 동일 카드 동시 수정
| 항목 | 내용 |
|------|------|
| **상황** | 사용자 A와 B가 같은 카드 내용을 동시에 수정 |
| **문제** | 최종 데이터가 비결정적으로 변경됨 |
| **해결** | Optimistic Locking (버전 기반) — 카드마다 `version` 필드 추가, 요청 시 version 비교 후 충돌 처리 |

### 2. 카드 이동과 삭제 동시 발생
| 항목 | 내용 |
|------|------|
| **상황** | 한 사용자는 카드 이동, 다른 사용자는 카드 삭제 |
| **문제** | 존재하지 않는 카드 상태 발생 가능 |
| **해결** | 서버에서 상태 검증 + atomic 처리 또는 lock 적용 |

### 3. 동일 컬럼에 카드 동시 추가
| 항목 | 내용 |
|------|------|
| **상황** | 여러 사용자가 동시에 카드 삽입 |
| **문제** | 카드 순서 충돌 발생 |
| **해결** | 서버 기준 순서 재정렬, insert 시 순서 재계산 |

### 4. 카드 순서 동시 변경
| 항목 | 내용 |
|------|------|
| **상황** | 여러 사용자가 동시에 순서 변경 |
| **문제** | 순서 불일치 |
| **해결** | 컬럼 단위 lock + reorder 작업 atomic 처리 |

## 🛡️ 동시성 제어 방법

- `Mutex` / `RwLock`을 통한 공유 데이터 보호
- `Arc`를 활용한 공유 상태 관리
- Version 기반 충돌 감지
- 필요 시 작업 큐(Queue) 기반 순차 처리

## 🧪 테스트 및 검증

| 테스트 유형 | 내용 |
|-------------|------|
| **동시성 테스트** | 여러 스레드에서 동시에 동일 자원 접근, Race Condition 재현 |
| **충돌 테스트** | 동일 카드 동시 수정 시 충돌 발생 여부 확인 |
| **일관성 테스트** | 카드 순서 정합성 검사, 삭제된 카드 접근 불가 확인 |
| **부하 테스트** | 다수 클라이언트 요청 처리, 데이터 무결성 유지 여부 확인 |

## 🚀 시작하기

```bash
# 레포지토리 클론
git clone https://github.com/codeNBogus/systemTech-F-NotionClone.git
cd systemTech-F-NotionClone

# 빌드 및 실행 (Rust 설치 필요)
cargo build
cargo run
```

## � 담당 역할 (이재익 - PR)

**시스템 아키텍처 설계**
- 전체 모듈 구조 정의 (models / store / handlers / errors 분리)
- 데이터 모델 설계: Board → Column → Card 계층 구조 및 관계 정의
- API 엔드포인트 설계 및 라우팅 구조 수립 (RESTful 원칙 기반)
- 공유 상태 관리 방식 결정 (`Arc<RwLock<...>>` 기반 메모리 저장소)

## �👥 팀 구성

| 이름 | 역할 |
|------|------|
| 이동열 |  |
| 이재익 | PR: 시스템 아키텍처 설계 |
| 이준서 |  |
| 이해성 |  |
## 📄 라이선스

MIT License

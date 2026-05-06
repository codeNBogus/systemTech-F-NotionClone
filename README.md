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
- 보드 생성 / 조회 / 삭제 (cascade)
- 컬럼 생성 및 관리
- 카드 생성 / 수정 / 삭제
- 카드 컬럼 간 이동
- 카드 순서 변경

### 협업 기능
- 여러 사용자의 동시 접속
- 동일 자원 동시 수정 처리
- 변경 사항 실시간 반영
- 사용자 닉네임 기반 작업자 구분

### 카드 수정 이력 로그 (추가 기능)
- 카드 생성·수정·상태변경·이동·순서변경 시 자동으로 이력 기록
- 각 이력 항목에 버전 번호, 작업 종류, 변경 시간(KST), 변경 내용 포함
- `GET /api/cards/:card_id/logs` API로 전체 이력 조회 가능
- 웹 UI에서 카드 hover 시 🕐 버튼을 클릭해 수정 이력 모달로 확인 가능
- 동시성 문제 발생 시 어느 시점에 어떤 변경이 일어났는지 추적하는 데 활용

### WebSocket 브로드캐스트 (추가 기능)
- 보드·컬럼·카드 변경 시 연결된 모든 클라이언트에 실시간 이벤트 전송
- 이벤트 종류 (총 10가지): `board_created`, `board_deleted`, `column_created`, `column_deleted`, `card_created`, `card_updated`, `card_deleted`, `card_moved`, `card_status_changed`, `card_reordered`
- `emit()`을 write lock 안에서 호출하여 뮤테이션 순서 = 브로드캐스트 순서 보장
- `/ws` 엔드포인트로 WebSocket 연결 (`ws://localhost:3000/ws`)
- 클라이언트는 자동 재연결 로직(3초)을 갖추어 끊김 없는 동기화 제공
- 이벤트 수신 시 현재 화면의 보드 데이터를 즉시 갱신하여, 다른 브라우저에서 수정한 내용이 새로고침 없이 반영
- 카드 생성·수정·상태변경·삭제는 클라이언트 상태에 즉시 반영하고, 이동·정렬·컬럼 변경은 최신 보드 데이터를 재조회해 화면 일관성 유지

### 사용자 닉네임 및 Audit Log (추가 기능)
- 화면 상단의 `nickname` 입력값을 작업자 식별값으로 사용
- 보드 생성 시 `owner_nickname`을 보드 정보에 저장
- 보드·컬럼·카드 생성/수정/삭제/이동 작업마다 `actor_nickname`을 함께 전송
- `GET /api/boards/:board_id/audit-logs` API로 보드별 감사 로그 조회 가능
- 웹 UI의 `Audit Log` 버튼으로 현재 보드의 작업 이력 확인 가능
- 감사 로그에는 작업자 닉네임, 작업 종류, 대상 종류, 시간(KST), 상세 내용 기록
- 상태 복구용 `data/wal.jsonl`과 감사 로그용 `data/audit.jsonl`을 분리하여 저장소 상태와 보안 로그 역할을 명확히 분리

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

### WAL (Write-Ahead Log) 영속성 (추가 기능)

기존 시스템은 메모리에만 데이터를 저장해 서버를 재시작하면 모든 보드/컬럼/카드가 사라지는 문제가 있었습니다. 이를 해결하기 위해 **Write-Ahead Log** 기반 영속성을 구현했습니다.

#### 동작 원리

```
[API 요청] → emit(event) → WAL append + fsync → WS broadcast
                                  ↓
                          data/wal.jsonl

[서버 시작] data/wal.jsonl → replay() → apply_events() → 메모리 복원
```

#### 핵심 특징

| 특성 | 설명 |
|------|------|
| **Append-only 로그** | `data/wal.jsonl`에 모든 상태 변경 이벤트를 JSON-Line으로 append |
| **fsync 보장** | 매 write마다 `file.sync_all()` 호출 → 실제 디스크에 기록 |
| **Replay 복원** | 서버 시작 시 WAL 전체를 읽어 메모리 재구성 |
| **Torn-write 처리** | 마지막 라인이 손상돼도 panic 없이 정상 라인까지만 복원 |
| **순서 보장** | WAL append → broadcast 순서로 emit, write lock 안에서 atomic 처리 |

> 감사 로그는 WAL에 섞지 않고 `data/audit.jsonl`에 별도로 저장합니다. `wal.jsonl`은 보드/컬럼/카드 상태 복구 전용이고, `audit.jsonl`은 사용자 작업 추적 전용입니다.

#### 검증 방법

```bash
# 1. 서버 실행, 보드/카드 생성
cargo run

# 2. Ctrl+C로 종료 후 재시작
cargo run
# → "📂 WAL replay: N events loaded" 출력
# → 브라우저 새로고침 시 이전 데이터 그대로 복원
```

## 🏗️ 시스템 구조

### 백엔드
- **언어**: Rust
- **웹 프레임워크**: axum
- **비동기 처리**: tokio

### 데이터 관리
- **메모리 저장소**: `Arc<RwLock<HashMap<...>>>` 기반 in-memory store
- **영속성**: Write-Ahead Log (`data/wal.jsonl`) — append + fsync로 디스크 보존
- **복원**: 서버 시작 시 WAL replay로 메모리 재구성
- **감사 로그**: `data/audit.jsonl` — 사용자 닉네임과 작업 이력을 상태 저장소와 분리해 기록

### 클라이언트
- HTML/CSS/JavaScript (Vanilla JS)
- WebSocket 자동 재연결 클라이언트
- WebSocket 이벤트 수신 시 현재 보드 UI 즉시 갱신
- 닉네임 입력 및 Audit Log 조회 UI 제공
- 캐시 방지 meta 태그 적용

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

### 5. WAL 기록 vs 메모리 업데이트 순서
| 항목 | 내용 |
|------|------|
| **상황** | 메모리는 업데이트됐지만 WAL 기록 전 서버 크래시 |
| **문제** | 재시작 시 메모리 상태와 WAL이 불일치 (데이터 유실) |
| **해결** | `emit()`을 write lock 안에서 호출 — WAL append → fsync → broadcast 순서로 atomic 처리하여 WAL과 메모리가 항상 일치 |

## 🛡️ 동시성 제어 방법

- `Mutex` / `RwLock`을 통한 공유 데이터 보호
- `Arc`를 활용한 공유 상태 관리
- Version 기반 충돌 감지
- WAL append + fsync로 durability 보장
- 필요 시 작업 큐(Queue) 기반 순차 처리

## 🧪 테스트 및 검증

| 테스트 유형 | 내용 |
|-------------|------|
| **동시성 테스트** | 여러 스레드에서 동시에 동일 자원 접근, Race Condition 재현 |
| **충돌 테스트** | 동일 카드 동시 수정 시 충돌 발생 여부 확인 |
| **일관성 테스트** | 카드 순서 정합성 검사, 삭제된 카드 접근 불가 확인 |
| **부하 테스트** | 다수 클라이언트 요청 처리, 데이터 무결성 유지 여부 확인 |
| **WAL 영속성 테스트** | 서버 재시작 후 데이터 복원 확인 |

## 🚀 시작하기

```bash
# 레포지토리 클론
git clone https://github.com/codeNBogus/systemTech-F-NotionClone.git
cd systemTech-F-NotionClone

# 빌드 및 실행 (Rust 설치 필요)
cargo build
cargo run
```

서버 시작 시 `data/wal.jsonl`이 있으면 자동으로 replay되어 이전 상태를 복원합니다.

## 👤 담당 역할 (이재익 - PR)

**시스템 아키텍처 설계**
- 전체 모듈 구조 정의 (models / store / handlers / errors 분리)
- 데이터 모델 설계: Board → Column → Card 계층 구조 및 관계 정의
- API 엔드포인트 설계 및 라우팅 구조 수립 (RESTful 원칙 기반)
- 공유 상태 관리 방식 결정 (`Arc<RwLock<...>>` 기반 메모리 저장소)

## 👤 담당 역할 (이동열)

**사용자 식별, 실시간 UI 반영, 감사 로그 분리 저장 구현**
- 사용자 닉네임 입력 기능 구현 — 클라이언트에서 `nickname` 값을 관리하고 보드/컬럼/카드 변경 요청마다 `actor_nickname`으로 서버에 전달
- Notion식 실시간 반영 개선 — WebSocket 이벤트 수신 시 새로고침이나 보드 재선택 없이 현재 화면의 카드 생성·수정·상태변경·삭제를 즉시 반영
- 실시간 일관성 보완 — 카드 이동·정렬·컬럼 변경처럼 주변 position 정합성이 필요한 작업은 최신 보드 상태를 재조회해 화면 불일치 방지
- 보드별 Audit Log 구현 — `AuditLog` 모델과 `GET /api/boards/:board_id/audit-logs` API를 추가하여 작업자, 작업 종류, 시간, 상세 내용을 조회 가능하게 구성
- 상태 저장소와 감사 로그 분리 — 보드 상태 복구는 `data/wal.jsonl`, 사용자 작업 추적은 `data/audit.jsonl`에 저장하도록 역할 분리

## 👤 담당 역할 (이해성)

**데이터 영속성 및 실시간 협업 인프라 구축**
- Write-Ahead Log(WAL) 기반 영속성 계층 설계·구현 (append + fsync로 durability 보장, 서버 시작 시 replay로 메모리 자동 복원, torn-write 처리)
- 이벤트 소싱 구조 확장 (`WsEvent` 직렬화/역직렬화, 10가지 이벤트별 상태 복원 로직 `apply_events` 구현, write lock 내부에서 WAL → broadcast 원자적 순서 보장)
- 보드 삭제 기능 (cascade) 구현 — 보드 → 컬럼 → 카드까지 일관성 있게 제거되도록 백엔드·API·프론트엔드 구현
- WebSocket 실시간 동기화 클라이언트 구축 — 자동 재연결, 이벤트 수신 시 UI 자동 갱신, 캐시 방지로 다중 사용자 환경에서 변경사항이 즉시 반영되도록 마무리

## 👥 팀 구성

| 이름 | 역할 |
|------|------|
| 이동열 | 사용자 닉네임 식별 + Notion식 실시간 UI 반영 + Audit Log 분리 저장 |
| 이재익 | PR: 시스템 아키텍처 설계 |
| 이준서 |  |
| 이해성 | WAL 영속성 + 보드 삭제 + 실시간 동기화 클라이언트 |

## 📄 라이선스

MIT License

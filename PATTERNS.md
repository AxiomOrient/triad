# Code Patterns and Anti-Patterns

이 문서는 triad 코드베이스에서 발견된 코드 품질 문제와 해결 패턴을 정리한다.
구현 중 반복적으로 나타난 문제들을 기록해 재발을 막는 것이 목적이다.

---

## 왜 이 작업이 필요했나

빠른 구현 과정에서 아래 문제들이 누적되었다.

1. 동일한 디렉터리 스캔 코드가 4곳에 복사됨
2. 루프 안에서 파일 I/O를 반복 — N+1 패턴
3. 테스트 유틸 함수가 두 버전 공존 — 약한 버전과 강한 버전
4. 수동 max 추적 루프가 3곳에서 반복
5. 소소한 코드 냄새 (let _ =, 이중 변수 바인딩, vec![] 불필요 사용)

---

## 발견된 패턴과 해결책

### 1. 반복 I/O 헬퍼 누락

**문제**: `next_run_id`, `read_run_records`, `pending_patch_id_for_claim`, `next_patch_id`가
각각 `fs::read_dir()` + 에러 핸들링 30줄을 동일하게 반복.

**해결**: 공통 헬퍼 추출.

```rust
// Before: 각 함수마다 30줄씩 반복
for entry in fs::read_dir(dir).map_err(|err| { ... })? {
    let entry = entry.map_err(|err| { ... })?;
    let file_type = entry.file_type().map_err(|err| { ... })?;
    if !file_type.is_file() || ext != Some("json") { continue; }
    // ...
}

// After: 헬퍼로 위임
fn json_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>, TriadError> { ... }

for path in json_files_in_dir(dir)? {
    // 핵심 로직만
}
```

---

### 2. 루프 안 N+1 파일 읽기

**문제**: `list_claims`, `next_claim`, `get_claim`이 각 클레임마다 `detect_drift`를 호출하고,
`detect_drift` 내부에서 매번 evidence 파일 전체를 읽음.
클레임이 N개면 evidence 파일을 N번 읽는다.

**해결**: 루프 밖에서 한 번만 읽고 내부 함수에 전달.

```rust
// Before: N+1
for claim in claims {
    let drift = self.detect_drift(&claim.id)?; // 내부에서 evidence 파일 읽음
}

// After: 1번만 읽기
let all_evidence = read_evidence(evidence_path)?;  // 한 번만
for claim in claims {
    let drift = compute_drift(repo_root, &claim.id, &all_evidence, ...)?;
}
```

**원칙**: 루프 바깥으로 꺼낼 수 있는 I/O는 모두 꺼낸다.

---

### 3. 수동 max 추적 루프

**문제**: "시퀀스 번호가 가장 큰 레코드 찾기" 패턴이 3곳에서 수동으로 반복.

```rust
// Before: 수동 추적
let mut latest = None;
for record in records {
    let replace = latest.as_ref()
        .map(|cur| record.id.sequence_number() > cur.id.sequence_number())
        .unwrap_or(true);
    if replace { latest = Some(record); }
}
```

**해결**: `Iterator::max_by_key` 사용.

```rust
// After: 표준 이터레이터
let latest = records.into_iter()
    .filter(|r| &r.claim_id == claim_id)
    .max_by_key(|r| r.id.sequence_number());
```

---

### 4. 테스트 유틸 중복

**문제**: `assert_output_matches_schema_data_contract`(단순 필드 체크)와
`assert_output_matches_schema`(재귀 타입/패턴 검증) 두 버전이 공존.

**해결**: 더 강한 버전으로 통일하고 약한 버전 삭제.

**원칙**: 테스트 유틸 함수는 프로덕션 코드와 동일하게 단일 진실 원천을 유지한다.

---

### 5. 의도 불명확 코드

```rust
// Before: 리턴값 버리는 의도가 불명확
let _ = canonical_claim_revision_bytes(&claim);

// After: 명시적 호출 + 주석
// Validate claim is serializable before returning.
canonical_claim_revision_bytes(&claim);
```

```rust
// Before: 이중 바인딩
let evidence = [...].collect::<Vec<_>>();
let mut evidence = evidence;
evidence.sort_by(...);

// After: 직접 mut 선언
let mut evidence = [...].collect::<Vec<_>>();
evidence.sort_by(...);
```

---

## 핵심 원칙 요약

| 원칙 | 설명 |
|------|------|
| 헬퍼 먼저 | 동일 패턴이 3곳 이상이면 헬퍼로 추출한다 |
| I/O는 루프 밖 | 루프 안에서 파일/DB를 읽으면 항상 N+1을 의심한다 |
| 표준 이터레이터 | 수동 max/min 추적보다 `max_by_key`, `min_by_key`를 쓴다 |
| 단일 진실 원천 | 테스트 유틸도 두 버전이 생기면 약한 버전을 즉시 삭제한다 |
| 의도 명시 | `let _ =` 대신 명시적 호출과 주석으로 의도를 드러낸다 |
| clippy 0 유지 | 모든 커밋은 `cargo clippy` 경고 없이 통과해야 한다 |

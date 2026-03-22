# Claim Format

Claim은 "이 저장소에서 지금 참이어야 하는 작은 약속 하나"다.
좋은 claim은 사람이 읽어도 의미가 분명하고, evidence를 붙여 참/거짓을 판정할 수 있을 정도로 작다.

## Directory Contract

- claims root는 `spec/claims/` 다.
- 파일명은 항상 `{CLAIM_ID}.md` 다.
- nested directory는 허용하지 않는다.
- file stem과 H1 claim id는 exact match여야 한다.

## Required Layout

```md
# REQ-auth-001 Login success

## Claim
System grants access with valid credentials.

## Examples
- valid -> 200
- invalid -> 401

## Invariants
- session is issued

## Notes
MFA out of scope.
```

섹션 순서는 아래만 허용한다.

1. `# <CLAIM_ID> <Title>`
2. `## Claim`
3. `## Examples`
4. `## Invariants`
5. optional `## Notes`

## Parsing Rules

- `Claim` 본문은 자유 text를 허용한다.
- `Examples` 와 `Invariants` 는 `- ` bullet만 허용한다.
- 각 bullet payload는 trim 후 non-empty여야 한다.
- 추가 H2/H3 섹션은 허용하지 않는다.

좋은 예:

- "올바른 로그인은 성공해야 한다"
- "없는 사용자를 조회하면 404를 반환해야 한다"

피해야 할 예:

- "인증 시스템이 전반적으로 잘 동작해야 한다"
- "사용자 경험이 자연스러워야 한다"

## Canonicalization

- trailing whitespace를 제거한다.
- multiline body 앞뒤 blank line을 제거한다.
- bullet marker는 항상 `- ` 로 다시 쓴다.
- 섹션 순서를 고정한다.
- file ending은 terminal newline 1개를 유지한다.

canonical text의 `sha256`이 `revision_digest`다.

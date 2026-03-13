# Claim Format

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

## Canonicalization

- trailing whitespace를 제거한다.
- multiline body 앞뒤 blank line을 제거한다.
- bullet marker는 항상 `- ` 로 다시 쓴다.
- 섹션 순서를 고정한다.
- file ending은 terminal newline 1개를 유지한다.

canonical text의 `sha256`이 `revision_digest`다.

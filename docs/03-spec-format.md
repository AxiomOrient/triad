# Spec Format

## Directory Contract

- claims root: `spec/claims/`
- file name: `{CLAIM_ID}.md`
- nested directory 금지
- file stem과 H1의 claim id는 정확히 일치해야 한다.

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

필수 섹션 순서:

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

## Canonicalization Rules

- trailing whitespace 제거
- multiline body 앞뒤 blank trim
- bullet marker는 항상 `- `
- 섹션 순서 고정
- file ending은 terminal newline 1개

canonical text의 `sha256`이 `revision_digest`다.

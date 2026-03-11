# Spec Format

## Scope

- machine-readable claim markdown 파일 형식과 parser 규칙을 정의한다.

## Out of Scope

- runtime 실행 정책, test layer semantics는 다루지 않는다.

## Directory Contract

- strict claims root: `spec/claims/`
- file name: `{CLAIM_ID}.md`
- claim file 1개는 claim 1개와 정확히 1:1 대응한다.
- nested directory는 허용하지 않는다. 단순성과 grep 가능성을 우선한다.
- runtime discovery는 top-level `*.md` 만 claim candidate로 읽고, nested directory entry가 보이면 오류로 처리한다.

## File Template

```md
# REQ-auth-001 Login success

## Claim
사용자는 유효한 이메일/비밀번호 조합으로 로그인할 수 있어야 한다.

## Examples
- valid credentials -> 200 + session cookie
- wrong password -> 401
- deleted user -> 404

## Invariants
- 비밀번호 원문은 로그에 남지 않는다.
- 실패 응답은 계정 존재 여부를 과도하게 노출하지 않는다.

## Notes
- MFA는 범위 밖
```

## Mandatory Structure

1. 첫 줄은 H1 하나여야 한다.
2. H1 형식: `# <CLAIM_ID> <Title>`
3. 아래 H2 섹션은 순서대로 존재해야 한다.
   - `## Claim`
   - `## Examples`
   - `## Invariants`
4. `## Notes` 는 선택이다.
5. `Examples` 와 `Invariants` 는 bullet list만 허용한다.
6. 추가 H2/H3 섹션은 금지한다.

## Parsing Rules

### H1
- `CLAIM_ID` regex: `^REQ-[a-z0-9-]+-\d{3}$`
- title은 빈 문자열이면 안 된다.
- runtime은 H1의 `CLAIM_ID` 를 먼저 읽고 file stem과 exact match를 강제한다.

### Claim section
- 자유 본문 허용
- 최소 1개 paragraph 필요

### Examples
- 항목 수는 1개 이상
- 각 항목은 non-empty text
- parser는 예시 문장을 그대로 string 배열로 저장

### Invariants
- 항목 수는 1개 이상
- 각 항목은 선언문이어야 하며 imperative checklist는 피한다

### Notes
- 자유 형식
- semantic parsing 대상이 아니다
- agent prompt에 보조 맥락으로만 첨부된다

## Invalid Examples

### Invalid: missing invariants
```md
# REQ-auth-001 Login success

## Claim
...

## Examples
- valid credentials -> 200
```

### Invalid: extra section
```md
# REQ-auth-001 Login success

## Claim
...

## Examples
- valid credentials -> 200

## Invariants
- no plaintext password logs

## Open Questions
- should deleted user return 404?
```

### Invalid: wrong file name
- file path: `spec/claims/login.md`
- H1 id: `REQ-auth-001`
- reason: file name must match id exactly

## Revision Model

spec file 내부에 revision number를 쓰지 않는다.  
revision은 accepted content로부터 계산된다.

canonical revision input은 UTF-8 bytes로 만든다.

canonical text 규칙:
- H1은 항상 `# <CLAIM_ID> <Title>` 한 줄로 다시 쓴다.
- 섹션 순서는 항상 `Claim -> Examples -> Invariants -> Notes` 이다.
- 섹션 사이에는 정확히 빈 줄 1개만 둔다.
- line ending은 항상 LF(`\n`) 로 통일한다.
- file 끝에는 terminal newline 1개를 둔다.
- `Claim`/`Notes` 본문은 각 줄의 trailing space와 trailing tab을 제거하고, 앞뒤 blank line은 제거한다.
- `Examples`/`Invariants` 항목은 `- ` prefix로 다시 쓰고, bullet payload의 앞뒤 공백은 제거한다.
- `Notes` 가 없으면 `## Notes` 섹션 자체를 생략한다.

revision 계산기는 이 canonical text를 hash input으로 사용한다.
실제 revision 값 매핑은 이후 단계에서 stable hash -> revision number 방식으로 연결한다.

구현에서는 `u32 revision` 필드를 도메인에 두되, 실제 계산기는 `triad-runtime` 에서 제공한다.

## Human Editing Rules

- claim file은 인간이 직접 수정할 수 있다.
- 단, `work` 단계에서 AI가 spec file을 직접 수정하는 것은 금지된다.
- behavior change를 반영하려면 patch draft -> `accept` 를 거쳐야 한다.

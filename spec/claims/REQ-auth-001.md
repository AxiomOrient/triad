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

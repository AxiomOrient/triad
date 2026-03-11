# REQ-auth-002 Session invalidation on logout

## Claim
로그아웃 이후 기존 세션 쿠키는 더 이상 인증된 요청에 사용될 수 없어야 한다.

## Examples
- logout -> 204
- old cookie after logout -> 401

## Invariants
- 로그아웃은 idempotent 하게 처리된다.
- 로그아웃 성공 후 동일 세션으로 보호 자원에 접근할 수 없다.

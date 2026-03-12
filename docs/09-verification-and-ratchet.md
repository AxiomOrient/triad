# Verification And Ratchet

## Verification

`triad`는 verify command 실행 결과를 `Evidence`로 정규화하고, current snapshot과 비교해 report를 계산한다.

## Freshness

evidence가 fresh 이려면 둘 다 만족해야 한다.

1. `evidence.claim_revision_digest == claim.revision_digest`
2. `evidence.artifact_digests == current_artifact_snapshot`

freshness 종류:

- `Fresh`
- `StaleClaimRevision`
- `StaleArtifacts`
- `StaleBoth`

## ClaimStatus

hard evidence만 status 계산에 사용한다.

```text
if any fresh hard fail exists:
    Contradicted
else if any fresh hard pass exists:
    Confirmed
else if any fresh hard unknown exists:
    Blocked
else if any stale hard evidence exists:
    Stale
else:
    Unsupported
```

## Ratchet

patch/accept ratchet은 현재 v1 범위 밖이다.

- patch draft 없음
- accept flow 없음
- spec rewrite automation 없음

phase 2가 필요하면 별도 `triad-ratchet` crate로 분리한다.

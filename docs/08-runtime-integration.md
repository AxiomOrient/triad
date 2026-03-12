# Runtime Integration

v1에서 `triad`는 external runtime backend를 직접 소유하지 않는다.

현재 integration seam은 두 층뿐이다.

1. `triad-core`
   - pure verification
2. `triad-fs`
   - claim markdown
   - evidence ndjson
   - snapshot collection
   - verify command capture

즉 현재 host integration은 “runtime adapter”가 아니라 “filesystem adapter”다.

다른 host는 아래 방식으로 붙을 수 있다.

- claim/source of truth를 자기 저장소에서 읽는다
- current artifact digests를 만든다
- evidence를 자기 저장 방식으로 읽는다
- `triad-core`에 `Claim`, snapshot, evidence를 넘겨 `ClaimReport`를 계산한다

이 구조의 목적은 verification kernel과 host/orchestrator 책임을 분리하는 것이다.

# Product Charter

## Scope

- `triad`가 무엇을 해결하는지, 어떤 원칙으로 설계되는지, 무엇을 하지 않을지를 정의한다.

## Out of Scope

- 세부 CLI 플래그, JSON schema 필드 정의, 파일 레이아웃의 line-by-line 설명은 다른 문서로 넘긴다.

## Problem Statement

스펙은 작성되는 순간 완성되지 않는다. 구현 과정에서 스펙의 빈틈이 드러나고, 코드가 바뀌면 테스트가 추가되어야 하며, 실제 검증 결과는 다시 스펙을 수정하게 만든다. 따라서 시스템의 핵심은 `spec + agent = code` 가 아니라 **`spec -> code -> tests -> spec` 의 반복 닫힘** 이다.

## Product Thesis

`triad`는 스펙/코드/테스트 삼각형을 직접 동기화하는 도구가 아니다.  
정확히는 **claim과 evidence를 중심으로 삼각형의 drift를 감지하고 ratchet 하는 local-first 엔진** 이다.

핵심 문장:

> 모델은 제안하고, 엔진은 검증하고, 인간은 승인한다.

## Primary Users

### Human Developer
- 다음에 무엇을 해야 하는지 한 번에 하나만 보고 싶다.
- spec을 유지하되, 구현 결과가 spec을 바꾸게 되는 현실을 받아들인다.
- AI가 만든 변경이 곧바로 정본이 되기를 원하지 않는다.

### AI Agent
- 좁은 컨텍스트에서 한 claim만 다루고 싶다.
- stdout JSON contract가 안정적이어야 한다.
- 명령 실패와 후속 행동을 exit code로 기계적으로 해석하고 싶다.

## Design Principles

1. **Simplicity**
   - workflow는 하나만 둔다: `next -> work -> verify -> accept`
   - 범용 model/provider SDK abstraction을 제품 범위에 넣지 않는다.
   - 다만 `work` 1회 실행을 위한 좁은 agent runtime adapter layer는 허용한다.
   - daemon과 hidden state를 두지 않는다.

2. **Focus On Essentials**
   - 정본은 `spec/`, `src/`, `tests/` 세 곳뿐이다.
   - `.triad/` 는 파생 상태와 감사 정보만 가진다.
   - decision log를 독립 정본으로 만들지 않는다.

3. **Human-Centered Design**
   - 인간에게는 다음 행동 하나만 보인다.
   - spec 수정은 direct write가 아니라 patch draft -> accept 로만 진행된다.
   - 되돌리기 어려운 자동화는 기본으로 두지 않는다.

4. **Determinism Over Cleverness**
   - `next` 선택 규칙은 고정이다.
   - drift 판단 규칙은 문서화된 하나의 방식만 사용한다.
   - priority weight, heuristic tuning, hidden scoring은 제품 범위에 두지 않는다.

5. **Local-First**
   - 기본 실행 단위는 local CLI다.
   - 서버 확장은 옵션이다.
   - 외부 인프라가 없더라도 핵심 workflow가 닫혀야 한다.

## Goals

- strict markdown spec을 atomic claim으로 해석한다.
- claim-scoped AI work를 수행한다.
- verify를 통해 append-only evidence를 남긴다.
- evidence를 근거로 spec patch draft를 제안한다.
- human CLI와 agent CLI를 분리하되 동일한 엔진을 사용한다.

## Non-Goals

- 범용 AI orchestration framework
- 여러 모델 provider를 추상화하는 SDK
- commit hook 기반의 hard block workflow
- 멀티에이전트 기본 실행
- 임의 자유 서식 문서 전체를 파싱하는 문서 시스템

## One Sure Way

제품에서 허용하는 표준 루프는 아래뿐이다.

1. `triad next`
2. `triad work`
3. `triad verify`
4. `triad accept`

이 루프를 우회하는 public 명령은 넣지 않는다.  
agent CLI는 low-level contract를 제공하지만, 철학은 동일하다. 엔진이 workflow를 소유하고 agent는 그 primitive를 호출할 뿐이다.

## Success Criteria

- 새 사용자가 10분 내에 strict claim file 하나를 추가하고 `next -> work -> verify -> accept` 루프를 완주할 수 있다.
- agent가 문서 없이도 `triad agent ...` JSON output만으로 claim을 조회하고 검증/patch 적용 흐름을 진행할 수 있다.
- spec direct write 없이도 behavior change를 patch draft로 제안할 수 있다.
- evidence 없이 spec이 바뀌지 않는다.

## Design References

- Kent Beck, Canon TDD — 작은 테스트 목록, 한 번에 하나의 검증 가능한 변화
- Andrej Karpathy, verifiability — 자동화는 specifiable/verifiable 축에서 결정됨
- plumb — staged diff + decision extraction 접근
- OpenAI Codex AGENTS.md / structured outputs
- one-shot CLI runtimes: `codex exec`, `claude -p`, `gemini -p`


## External References

- Kent Beck, Canon TDD — https://tidyfirst.substack.com/p/canon-tdd
- Andrej Karpathy, Verifiability — https://karpathy.bearblog.dev/verifiability/
- plumb repository — https://github.com/dbreunig/plumb
- Codex AGENTS.md guide — https://developers.openai.com/codex/guides/agents-md/

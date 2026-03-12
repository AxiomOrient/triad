# Product Charter

## Product Definition

`triad`는 주어진 `Claim`이 현재 artifact snapshot과 append-only evidence에 비추어 참인지 판정하는 verification kernel이다.

## Core Thesis

- core는 `Claim` 하나만 안다.
- verdict는 evidence freshness와 verdict 조합으로만 결정한다.
- grouping, workflow orchestration, patch ratchet은 core 밖 책임이다.

## Goals

- strict markdown claim을 읽는다.
- claim revision digest를 계산한다.
- snapshot + evidence를 기준으로 deterministic report를 계산한다.
- filesystem 기준 reference adapter를 제공한다.
- thin CLI로 `init / lint / verify / report`를 제공한다.

## Non-Goals

- `next / work / accept` workflow
- agent runtime
- provider abstraction
- patch draft state machine
- multi-agent orchestration
- command envelope schema platform

## Design Principles

1. 한 가지 질문만 푼다.
2. deterministic-first
3. append-only evidence
4. freshness는 claim digest + artifact digests로만 판정
5. headless 우선
6. accidental complexity보다 explicit data contract를 우선한다.

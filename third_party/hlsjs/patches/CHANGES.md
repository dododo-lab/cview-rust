# CView hls.js 패치 변경 내역

업스트림 태그 원본(`../fork.json` 의 `upstreamTag` tarball)에 아래 패치를 번호 순서대로 적용한다. 모든 새 동작은 설정으로
켜고, 기본값은 업스트림과 같다. 고친 파일에는 `CView 수정(NNNN)` 주석이 있다(Apache-2.0 §4(b)).

| 패치 | 설정 | 내용 |
|---|---|---|
| 0001-live-sync-reference | `liveSyncReference: 'targetduration' \| 'segment'` | 지연 목표·`liveSyncPosition` 상한·정지 후 증가 상한·갱신 지연 판정·최대 지연의 단위를 `TARGETDURATION` 대신 실제 평균 세그먼트 길이(`min(TARGETDURATION, 평균)`, 하한 0.5초)로 계산할 수 있게 한다 |
| 0002-live-reload-reference | `liveReloadReference: 'targetduration' \| 'segment'` | 라이브 재생목록 재요청 간격의 기준을 실제 평균 세그먼트 길이로 할 수 있게 한다. 치지직에서 재생목록이 안 바뀐 뒤의 재요청 대기가 5초 → 약 1초 |
| 0003-blocking-reload-policy | `blockingReloadOutsideLowLatency: boolean`, `blockingReloadFallback: boolean` | 저지연 모드가 아닐 때 블로킹 재요청 지시자(`_HLS_msn/_HLS_part`)를 붙이지 않을 수 있게 하고, 지시자 요청이 HTTP 400 이면 지시자 없이 즉시 다시 묻는다. 거절이 3번 쌓이면 그 인스턴스는 블로킹 재요청을 끈다 |
| 0004-live-sync-stall-decay | `liveSyncStallDecayMs: number` | 멈춤마다 +1초씩 늘어나는 지연 목표를, 마지막 멈춤 뒤 이 시간마다 하나씩 되돌린다. 설정하면 같은 목표값 재대입은 멈춤 이력을 지우지 않는다 |
| 0005-load-unbuffered-tail-parts | `loadUnbufferedTailParts: boolean` | 저지연(파트) 로드에서 fragment-tracker 의 버퍼 판정 여유(0.2초) 때문에 앞 파트만 받은 세그먼트가 OK 로 잡히면(꼬리 파트가 0.03~0.15초이거나 버퍼 끝이 파트 끝보다 몇 ms 모자람), `isLoopLoading`·`loadFragment` 가 받을 것이 없다고 보고 그 파트가 재생목록에서 빠질 때까지(≈12초) 아무것도 받지 못한다. 받지 않은 파트가 남았으면 받는다(한 번 받은 파트는 다시 받지 않는다) |

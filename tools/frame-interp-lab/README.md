# 프레임 보간 연구 도구

라이브 영상을 실시간으로 보간해 더 부드럽게 보여 줄 수 있는지 확인하려고 만든 측정 도구다.
앱 빌드에는 들어가지 않는다. 결론과 수치는 [연구 보고서](../../docs/research/frame-interpolation.md)와
[개발 계획·1단계 실측](../../docs/research/vfi-dev-plan.md)에 있다.

| 파일 | 내용 |
| --- | --- |
| `bench.m` | VideoToolbox `VTFrameProcessor` 벤치마크. 저지연 보간(`lowlat`), 프레임률 변환(`frc`, `frcq`), 비교용 그대로 두기·단순 섞기 |
| `webgpu-bench.html` | WKWebView 안 WebGPU 양방향 블록 매칭 보간(WGSL) |
| `run-webgpu.swift` | 위 페이지를 앱과 같은 WKWebView로 띄워 결과(JSON)를 출력 |
| `run.sh` | 전체 빌드·측정(2026-11-13 보고서) |
| `run-plan.sh` | 개발 계획 1단계 실측: 보호 장치·서킷브레이커·RIFE GPU 시간(창을 띄우지 않음) |
| `rife-mpsgraph.m` | RIFE 4.25 / 4.25.lite 를 MPSGraph 로 돌려 시간을 잰다(`--weights` 없으면 무작위 가중치). `--check --weights` 는 CPU 기준 구현과 출력 차이 |
| `mps-ops-micro.m` | 위 복제본의 1080p 연산별 시간(합성곱·전치 합성곱·concat·slice·grid_sample) |
| `rife-graph.h` | RIFE 그래프 구성(원래 구성·빠른 구성, 무작위 또는 학습 가중치)과 PyTorch 연산 의미를 따른 CPU 기준 구현 |
| `rife-export.py` | Practical-RIFE `flownet.pkl` → 원시 float32(`weights.bin`, `index.json`). torch 불필요, 허용 목록 언피클러 |
| `mpp-conv-micro.m` | Metal 4 MetalPerformancePrimitives `convolution2d` 를 셰이더에서 돌려 MPSGraph 와 비교(OS 런타임 컴파일) |

## 실행

macOS 26 이상, Xcode 명령줄 도구가 필요하다.

```sh
tools/frame-interp-lab/run.sh                 # 합성 장면만
tools/frame-interp-lab/run.sh /tmp/bipbop.mp4  # 실제 영상도 함께
```

`bench` 옵션:

- `--source synth|<파일>`: 합성 장면(기본) 또는 영상 파일. 파일은 원래 프레임률의 절반을 입력으로 쓰고 빠진 프레임을 정답으로 비교한다.
- `--algo lowlat|frc|frcq|rife|none`. `rife` 는 `--rife-weights <rife-export.py 출력 폴더>`(저장소 밖), `--rife-variant 4.25|lite`,
  `--rife-scale 1|0.5`, `--rife-fast 3x3|phase|off`, `--rife-dtype f32|f16`(FP16 은 정합 시험에서 오차가 커서 비권장), `--rife-bgr`
- `--size WxH`(합성), `--max-width N`(파일 축소), `--proc-size WxH`(줄여서 보간한 뒤 다시 키움)
- `--in-fps N`, `--interp N`(입력 사이에 넣을 장 수)
- `--loop 초`: 입력 속도에 맞춰 반복하며 마감 초과 비율과 CPU를 잰다. ANE·GPU 사용률은 이때 바깥에서 본다.
- `--inflight N`: 비동기 동시 요청 수(처리량 측정, 화질 계산 생략)
- `--dump 폴더`: 21번째 입력 쌍의 앞 프레임·섞기·보간·정답 PNG(`--guards` 면 보호 장치 적용본도)
- `--cuts N`: 합성 장면에서 입력 N장마다 장면 전환(배경·물체만 바뀌고 방송 화면 글자는 그대로)
- `--guards`: 보간 결과에 보호 장치를 씌워 영역별 PSNR 비교. `cut`(64×36 휘도 SAD + 히스토그램),
  `static`(120ms 이상 정지한 화소는 원본), `conf`(움직임으로 설명되지 않는 32×32 타일은 앞 프레임). 문턱은 `--g-*`
  (`--g-cut-sad`, `--g-cut-hist`, `--g-static-ms`, `--g-static-k`, `--g-static-tau`, `--g-static-erode`, `--g-conf-abs`, `--g-conf-rel`, `--g-conf-layers`)
- `--rt 초 [--breaker off|on|v1] [--budget-ms B]`: 실시간 모사. 절대 시간표로 입력이 도착하고 비동기 1건만 처리한다
  (측정 스레드는 시간 제약 정책). 마감(도착 + B, 기본 B = 1000/(출력fps−입력fps))을 넘긴 보간은 버린다.
  `on`(v2): 0.5초 창의 평균 > 0.85·B 또는 마감 초과 ≥ 10%, 또는 1건 > 2·B(재시도 직후 2건 제외)면 보간을 멈췄다가
  1·2·4·8초 뒤 다시 시도한다. `v1`: 첫 시험 규칙(8건 창, 마감 초과 2건).

합성 장면에는 가로로 흐르는 배경(1200 px/s), 회전 막대, 빠른 공(2400 px/s), 프레임마다 바뀌는 타이머 글자, 올라가는 채팅 상자, 고정 자막이 들어 있다. PSNR은 휘도 기준이며 영역별로도 낸다.

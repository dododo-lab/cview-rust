# 프레임 보간 연구 도구

라이브 영상을 실시간으로 보간해 더 부드럽게 보여 줄 수 있는지 확인하려고 만든 측정 도구다.
앱 빌드에는 들어가지 않는다. 결론과 수치는 [연구 보고서](../../docs/research/frame-interpolation.md)에 있다.

| 파일 | 내용 |
| --- | --- |
| `bench.m` | VideoToolbox `VTFrameProcessor` 벤치마크. 저지연 보간(`lowlat`), 프레임률 변환(`frc`, `frcq`), 비교용 그대로 두기·단순 섞기 |
| `webgpu-bench.html` | WKWebView 안 WebGPU 양방향 블록 매칭 보간(WGSL) |
| `run-webgpu.swift` | 위 페이지를 앱과 같은 WKWebView로 띄워 결과(JSON)를 출력 |
| `run.sh` | 전체 빌드·측정 |

## 실행

macOS 26 이상, Xcode 명령줄 도구가 필요하다.

```sh
tools/frame-interp-lab/run.sh                 # 합성 장면만
tools/frame-interp-lab/run.sh /tmp/bipbop.mp4  # 실제 영상도 함께
```

`bench` 옵션:

- `--source synth|<파일>`: 합성 장면(기본) 또는 영상 파일. 파일은 원래 프레임률의 절반을 입력으로 쓰고 빠진 프레임을 정답으로 비교한다.
- `--algo lowlat|frc|frcq|none`
- `--size WxH`(합성), `--max-width N`(파일 축소), `--proc-size WxH`(줄여서 보간한 뒤 다시 키움)
- `--in-fps N`, `--interp N`(입력 사이에 넣을 장 수)
- `--loop 초`: 입력 속도에 맞춰 반복하며 마감 초과 비율과 CPU를 잰다. ANE·GPU 사용률은 이때 바깥에서 본다.
- `--inflight N`: 비동기 동시 요청 수(처리량 측정, 화질 계산 생략)
- `--dump 폴더`: 21번째 입력 쌍의 앞 프레임·섞기·보간·정답 PNG

합성 장면에는 가로로 흐르는 배경(1200 px/s), 회전 막대, 빠른 공(2400 px/s), 프레임마다 바뀌는 타이머 글자, 올라가는 채팅 상자, 고정 자막이 들어 있다. PSNR은 휘도 기준이며 영역별로도 낸다.

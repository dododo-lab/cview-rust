# CView hls.js 포크

`app-ui/player/hls.min.js` 는 업스트림 hls.js 태그 원본에 `patches/` 의 패치를 얹어 직접 빌드한 결과다.
(같은 포크를 Swift 판 CView v2 의 `ThirdParty/hls.js-*` 에서 옮겨 왔다. 1.7.3-cview.2 번들을 이 저장소의 스크립트로
바이트 단위까지 재현한 뒤 업스트림만 올렸다.)

| 경로 | 내용 |
|---|---|
| `fork.json` | 업스트림 태그·원본 tarball SHA-256·포크 리비전·빌드 대상·기능 플래그 |
| `patches/` | 우리 패치. `NNNN-이름.patch` 를 번호 순서대로 적용한다. 변경 내역은 `patches/CHANGES.md` |
| `contract/` | 빌드 번들 계약 테스트(`node --test`). 치지직형 재생목록을 실제 파서·로더·컨트롤러에 통과시킨다 |
| `integrity.json` | 커밋된 `hls.min.js` 의 버전·SHA-256·패치 목록(빌드 스크립트가 쓴다) |
| `../../scripts/build-hlsjs-fork.sh` | tarball 받기(SHA-256 확인) → 패치 → 버전 주입 → `npm ci` → rollup → 앱 리소스·매니페스트 갱신 → 계약 테스트 |

업스트림 소스는 저장소에 두지 않는다. 작업 폴더는 `CVIEW_HLSJS_WORK`, 없으면 cargo target-dir 아래 `hlsjs-fork` 다.

## 명령

```bash
bash scripts/build-hlsjs-fork.sh
```

```bash
bash scripts/build-hlsjs-fork.sh --check
```

```bash
node --test third_party/hlsjs/contract/*.test.cjs
```

`--check` 는 다시 빌드한 결과가 커밋된 `hls.min.js`·`integrity.json` 과 바이트 단위로 같은지 본다. `--debug` 는 압축하지 않은
빌드와 소스맵을 작업 폴더의 `debug/` 에 만든다.

## 경량 빌드(기능 플래그)

`fork.json` 의 `features` 로 쓰지 않는 기능을 빌드에서 뺀다: 자막, 대체 오디오, DRM(EME), CMCD, 콘텐츠 스티어링, 변수 치환,
인터스티셜, I-frame. 앱이 이 기능을 쓰지 않고 치지직 재생목록에 해당 태그가 없다(`contract/0000-chzzk-playlist-shapes.test.cjs`).

## 버전

- 패치가 없고 `forkRevision` 이 0 이면 업스트림 버전 그대로다(공식 릴리스와 바이트 동일).
- 그 밖에는 `<업스트림>-cview.<forkRevision>` 이다. `forkRevision` 은 패치 묶음의 리비전이라 패치를 더하거나 바꿀 때만 올린다.
  업스트림만 올리면 앞자리 버전이 바뀐다(1.7.3-cview.2 → 1.7.4-cview.2).

## 업스트림 갱신

1. `fork.json` 의 `upstreamTag`·`upstreamVersion`·`upstreamTarballSHA256` 을 고친다
   (`https://github.com/video-dev/hls.js/archive/refs/tags/<태그>.tar.gz` 의 SHA-256).
2. `bash scripts/build-hlsjs-fork.sh` — 패치가 안 붙으면 그 패치를 새 원본에 맞춰 다시 만든다.
3. 패치가 고친 문제를 업스트림이 고쳤으면 패치를 지운다. 계약 테스트와 앱 실재생을 확인한다.

## 패치 만드는 법

1. 빌드를 한 번 돌려 작업 폴더의 `tree/` 를 만든다(업스트림 원본이 기준 커밋인 독립 git 저장소다).
2. 고친 뒤 `git -C <작업 폴더>/tree diff > third_party/hlsjs/patches/NNNN-이름.patch`. 패치 하나에 목적 하나.
3. 고친 파일 맨 위에 `// CView 수정: …` 주석(Apache-2.0 §4(b)). `patches/CHANGES.md` 에 한 줄. 새 동작은 설정 플래그로,
   기본값은 업스트림과 같게.
4. `forkRevision` 을 올리고 다시 빌드한다.

// CView 프레임 보간 5단계 — 네이티브 재생 실험 도구 (앱 코드 밖)
//
// HLS/LL-HLS/MP4 → AVPlayer + AVPlayerItemVideoOutput(디코드 프레임 앞당겨 받기)
//   → 장면 전환 감지 → VideoToolbox 저지연 보간(VTLowLatencyFrameInterpolation, ANE)
//   → 정지 화소 보호(NV12) → 원본·중간 프레임을 PTS 순서로 출력
// 마감 서킷브레이커 v2, 세대 번호(탐색·화질 전환·프레임률 변화·불연속·재연결)로
// 이전 결과가 새 화면에 섞이지 않게 한다. 오디오는 AVPlayer가 그대로 내고,
// 영상 출력은 AVPlayerItem 시계(timebase)에 맞춘다.
//
// 출력 방식(--sink):
//   null      창 없음. 출력 프레임을 표시 시각까지 붙잡았다가 놓는다(기본).
//   renderer  창 없음. AVSampleBufferVideoRenderer + AVSampleBufferRenderSynchronizer.
//   window    창을 띄워 실제로 보여 준다(사용자가 직접 실행). i 키로 보간 켜고 끄기, q 키로 끝.
//
// 빌드: tools/vfi-native-player/build.sh   사용법: vfi-player --help

#import <Foundation/Foundation.h>
#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <VideoToolbox/VideoToolbox.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <CoreText/CoreText.h>
#import <QuartzCore/QuartzCore.h>
#include <mach/mach.h>
#include <mach/mach_time.h>
#include <mach/thread_policy.h>
#include <mach/thread_act.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <sys/resource.h>
#include <sys/sysctl.h>
#include <os/lock.h>

// ───────────────────────────── 시간·통계 도우미 ─────────────────────────────

static double nowMs(void) { return CACurrentMediaTime() * 1000.0; }   // mach_absolute_time 과 같은 시계

static uint64_t msToAbs(double ms) {
    static mach_timebase_info_data_t tb;
    if (!tb.denom) mach_timebase_info(&tb);
    return (uint64_t)(ms * 1e6 * tb.denom / tb.numer);
}

static double cpuMs(void) {
    struct rusage ru; getrusage(RUSAGE_SELF, &ru);
    return (ru.ru_utime.tv_sec + ru.ru_stime.tv_sec) * 1e3 + (ru.ru_utime.tv_usec + ru.ru_stime.tv_usec) / 1e3;
}

static double footprintMB(void) {
    task_vm_info_data_t info; mach_msg_type_number_t n = TASK_VM_INFO_COUNT;
    if (task_info(mach_task_self(), TASK_VM_INFO, (task_info_t)&info, &n) != KERN_SUCCESS) return NAN;
    return info.phys_footprint / 1048576.0;
}

typedef struct { double *v; long n, cap; } DArr;
static void dpush(DArr *a, double x) {
    if (a->n == a->cap) { a->cap = a->cap ? a->cap * 2 : 1024; a->v = realloc(a->v, a->cap * sizeof(double)); }
    a->v[a->n++] = x;
}
static int cmpd(const void *a, const void *b) { double x = *(const double *)a, y = *(const double *)b; return x < y ? -1 : x > y; }
// 선형 보간 분위수. 원본 배열은 건드리지 않는다.
static double dpct(const DArr *a, long from, double p) {
    long n = a->n - from; if (n <= 0) return NAN;
    double *t = malloc(n * sizeof(double)); memcpy(t, a->v + from, n * sizeof(double));
    qsort(t, n, sizeof(double), cmpd);
    double pos = p * (n - 1); long i = (long)floor(pos); double f = pos - i;
    double r = i + 1 < n ? t[i] * (1 - f) + t[i + 1] * f : t[i];
    free(t); return r;
}
static double dmin(const DArr *a, long from) { double m = INFINITY; for (long i = from; i < a->n; i++) m = MIN(m, a->v[i]); return a->n > from ? m : NAN; }
static double dmean(const DArr *a, long from) { double s = 0; for (long i = from; i < a->n; i++) s += a->v[i]; return a->n > from ? s / (a->n - from) : NAN; }

static double sec(CMTime t) { return CMTIME_IS_NUMERIC(t) ? CMTimeGetSeconds(t) : NAN; }
static CMTime tsec(double s) { return CMTimeMakeWithSeconds(s, 1000000); }

// ───────────────────────────── 옵션 ─────────────────────────────

typedef enum { SinkNull = 0, SinkRenderer, SinkWindow } SinkKind;
typedef enum { LatWeb = 0, LatAVPlayer, LatOff } LatMode;

typedef struct {
    NSString *url, *file, *chzzk, *jsonOut, *csvOut, *generate, *switchPlan;
    double seconds, leadMs, lookaheadMs, liveOffset, seekEvery, rateToggle, cutSad, cutHist, staticMs, injectPct, injectMs, forwardBuffer;
    int maxHeight, maxAhead, staticTau, poolCap, genW, genH, genFps, verbose, vtInput;   // vtInput: 0 auto, 1 direct, 2 copy
    BOOL interp, useCut, useStatic, useBreaker, breakerV2, audio, probeSizes, help;
    SinkKind sink; LatMode latency;
} Options;

static Options gOpt;

static void usage(void) {
    printf(
"vfi-player — CView 프레임 보간 5단계 네이티브 재생 실험 도구\n\n"
"소스(하나):\n"
"  --url <m3u8|mp4 URL>        HLS·LL-HLS·MP4 주소\n"
"  --file <경로>               로컬 동영상 파일\n"
"  --chzzk <채널 id|popular>   치지직 공개 방송(로그인 없음, 성인·유료·암호화 방송 제외)\n"
"  --generate <새 경로.mp4> [--gen-size 1280x720] [--gen-fps 60] --seconds N\n"
"                              시험 영상 만들기(이동 무늬·빠른 공·고정 자막·타이머·8초마다 장면 전환)\n"
"  --probe-sizes               VT 저지연 보간이 실제로 도는 크기 확인(세션 시작 + 처리 1회)\n\n"
"재생·측정:\n"
"  --seconds N                 측정 시간(기본 60)\n"
"  --sink null|renderer|window 출력 방식(기본 null, window 는 창을 띄움)\n"
"  --interp on|off             보간(기본 on, off 는 기준 측정)\n"
"  --guards cut,static|none    보호 장치(기본 cut,static)\n"
"  --breaker on|v2|off         마감 서킷브레이커(기본 on: 당겨 오기용 v3, v2: 연구 계획 1단계 규칙)\n"
"  --max-height N              화질 상한(기본 720, 0 = 제한 없음)\n"
"  --latency avplayer|web|off  라이브 지연 제어(기본 avplayer: AVPlayer 의 LL-HLS 정책 그대로. web: 앞 버퍼 4초 넘으면\n"
"                              1.03배, 2초 이하면 1.0배 — hls.js 기준이라 AVPlayer 에서는 계속 따라잡다 끊겼다)\n"
"  --live-offset S             라이브 지연 목표(configuredTimeOffsetFromLive, 기본 3.5초. 치지직 PART-HOLD-BACK 3.05초보다\n"
"                              가까우면 AVPlayer 가 -12640 \"Cannot get that close to live\"를 낸다)\n"
"  --lead-ms N                 출력 큐에 넣어야 하는 여유(기본 5ms)\n"
"  --lookahead-ms N            디코드 프레임을 앞당겨 받는 최대 거리(기본 150ms)\n"
"  --max-ahead N               붙잡아 두는 미처리 프레임 수 상한(기본 6)\n"
"  --seek-every S              S초마다 탐색(VOD·파일: 뒤로 10초, 앞으로 15초 번갈아)\n"
"  --forward-buffer S          앞 버퍼 목표(초, VOD). 화질 전환이 빨리 보이게 할 때(기본: AVPlayer 가 정함)\n"
"  --vt-input auto|direct|copy VT 입력: 디코더 버퍼 그대로(direct), VT 가 요구하는 속성의 버퍼로 복사(copy),\n"
"                              auto(기본): 추가 픽셀(extended pixels)이 있거나 VT 가 거부하면 복사\n"
"  --switch \"40:432,80:720\"   지정 시각에 화질 상한 바꾸기(초:세로 화소)\n"
"  --rate-toggle S             S초마다 재생 속도 1.0↔1.03 (VOD·파일)\n"
"  --audio                     소리 내기(기본: 창 모드만 켬)\n"
"  --json <경로> --csv <경로>   요약 JSON·초 단위 CSV\n"
"  --cut-sad X --cut-hist X --static-tau N --static-ms N   보호 장치 문턱\n"
"  --inject-delay P:MS         시험용: VT 완료의 P%%를 0~MS ms 늦춘다(포기·순서 처리 검증)\n");
}

static BOOL parseOptions(int argc, const char **argv) {
    Options o = {0};
    o.seconds = 60; o.leadMs = 5; o.lookaheadMs = 150; o.liveOffset = 3.5; o.maxHeight = 720; o.maxAhead = 6;
    o.cutSad = 0.10; o.cutHist = 0.30; o.staticTau = 3; o.staticMs = 120; o.poolCap = 16;
    o.genW = 1280; o.genH = 720; o.genFps = 60;
    o.interp = YES; o.useCut = YES; o.useStatic = YES; o.useBreaker = YES; o.sink = SinkNull; o.latency = LatAVPlayer;
    BOOL audioSet = NO;
    for (__block int i = 1; i < argc; i++) {
        NSString *a = @(argv[i]);
        NSString *(^val)(void) = ^NSString *{ if (i + 1 >= argc) { fprintf(stderr, "%s 값이 없습니다\n", argv[i]); exit(64); } return @(argv[++i]); };
        if ([a isEqual:@"--help"] || [a isEqual:@"-h"]) o.help = YES;
        else if ([a isEqual:@"--url"]) o.url = val();
        else if ([a isEqual:@"--file"]) o.file = val();
        else if ([a isEqual:@"--chzzk"]) o.chzzk = val();
        else if ([a isEqual:@"--generate"]) o.generate = val();
        else if ([a isEqual:@"--gen-size"]) { NSArray *p = [val() componentsSeparatedByString:@"x"]; o.genW = [p[0] intValue]; o.genH = p.count > 1 ? [p[1] intValue] : 0; }
        else if ([a isEqual:@"--gen-fps"]) o.genFps = [val() intValue];
        else if ([a isEqual:@"--probe-sizes"]) o.probeSizes = YES;
        else if ([a isEqual:@"--seconds"]) o.seconds = [val() doubleValue];
        else if ([a isEqual:@"--sink"]) { NSString *v = val(); o.sink = [v isEqual:@"renderer"] ? SinkRenderer : [v isEqual:@"window"] ? SinkWindow : SinkNull; }
        else if ([a isEqual:@"--interp"]) o.interp = ![val() isEqual:@"off"];
        else if ([a isEqual:@"--guards"]) { NSString *v = val(); o.useCut = [v containsString:@"cut"]; o.useStatic = [v containsString:@"static"]; }
        else if ([a isEqual:@"--breaker"]) { NSString *v = val(); o.useBreaker = ![v isEqual:@"off"]; o.breakerV2 = [v isEqual:@"v2"]; }
        else if ([a isEqual:@"--max-height"]) o.maxHeight = [val() intValue];
        else if ([a isEqual:@"--latency"]) { NSString *v = val(); o.latency = [v isEqual:@"avplayer"] ? LatAVPlayer : [v isEqual:@"off"] ? LatOff : LatWeb; }
        else if ([a isEqual:@"--live-offset"]) o.liveOffset = [val() doubleValue];
        else if ([a isEqual:@"--lead-ms"]) o.leadMs = [val() doubleValue];
        else if ([a isEqual:@"--lookahead-ms"]) o.lookaheadMs = [val() doubleValue];
        else if ([a isEqual:@"--max-ahead"]) o.maxAhead = MAX(2, [val() intValue]);
        else if ([a isEqual:@"--seek-every"]) o.seekEvery = [val() doubleValue];
        else if ([a isEqual:@"--switch"]) o.switchPlan = val();
        else if ([a isEqual:@"--rate-toggle"]) o.rateToggle = [val() doubleValue];
        else if ([a isEqual:@"--audio"]) { o.audio = YES; audioSet = YES; }
        else if ([a isEqual:@"--json"]) o.jsonOut = val();
        else if ([a isEqual:@"--csv"]) o.csvOut = val();
        else if ([a isEqual:@"--cut-sad"]) o.cutSad = [val() doubleValue];
        else if ([a isEqual:@"--cut-hist"]) o.cutHist = [val() doubleValue];
        else if ([a isEqual:@"--static-tau"]) o.staticTau = [val() intValue];
        else if ([a isEqual:@"--static-ms"]) o.staticMs = [val() doubleValue];
        else if ([a isEqual:@"--forward-buffer"]) o.forwardBuffer = [val() doubleValue];
        else if ([a isEqual:@"--vt-input"]) { NSString *v = val(); o.vtInput = [v isEqual:@"direct"] ? 1 : [v isEqual:@"copy"] ? 2 : 0; }
        else if ([a isEqual:@"--pool-cap"]) o.poolCap = MAX(4, [val() intValue]);
        else if ([a isEqual:@"--inject-delay"]) { NSArray *p = [val() componentsSeparatedByString:@":"]; o.injectPct = [p[0] doubleValue]; o.injectMs = p.count > 1 ? [p[1] doubleValue] : 100; }
        else if ([a isEqual:@"-v"]) o.verbose++;
        else { fprintf(stderr, "알 수 없는 옵션: %s (--help)\n", argv[i]); return NO; }
    }
    if (!audioSet) o.audio = o.sink == SinkWindow;
    gOpt = o;
    return YES;
}

// ───────────────────────────── 프레임·짝 ─────────────────────────────

@interface VFFrame : NSObject {
@public
    CVPixelBufferRef pb;
    CMTime pts;
    double acqMs, lookMs;   // 받은 시각, 받을 때 표시까지 남은 시간(아이템 시간)
    int w, h;
    uint64_t gen;
}
@end
@implementation VFFrame
- (void)dealloc { if (pb) CVPixelBufferRelease(pb); }
@end

@class VFSession;
@interface VFPair : NSObject {
@public
    VFFrame *prev, *cur;
    CVPixelBufferRef dst, srcA, srcB;   // srcA·srcB: 복사 방식일 때 VT 에 넣은 사본
    CMTime mid;
    uint64_t gen;
    double submitMs;
    BOOL abandoned, warm, stalledAtSubmit, counted, resolved;   // counted: 비율 집계 대상, resolved: 결과를 이미 셈
    BOOL probe;               // 브레이커 반쯤 열림 시험 요청(결과는 내보내지 않는다)
    long guardPushes;
    __strong VFSession *session;
}
@end
@implementation VFPair
- (void)dealloc { if (dst) CVPixelBufferRelease(dst); if (srcA) CVPixelBufferRelease(srcA); if (srcB) CVPixelBufferRelease(srcB); }
@end

// ───────────────────────────── 보호 장치(NV12 Y 평면) ─────────────────────────────
// tools/frame-interp-lab/bench.m 의 cut·static 을 420v 입력에 맞게 옮겼다.
// 장면 전환: 64×36 축소 휘도 평균 절대차 그리고 32칸 히스토그램 L1 거리가 둘 다 문턱 이상.
// 정지 보호: |Y(n) − Y(n−1)| ≤ τ 가 K장 이어진 화소는 보간 대신 두 원본 평균(색차는 2×2 모두 보호일 때).
// 문턱은 전체 범위 휘도에서 정했으므로 썸네일·히스토그램은 비디오 범위(16~235)를 전체 범위로 펴서 계산한다.

typedef struct {
    int W, H, tau, K;
    double cutSad, cutHist;
    uint8_t *cnt, *prot;
    CVPixelBufferRef last;    // 마지막으로 넣은 프레임(보유). Y 평면을 복사하지 않고 직접 읽는다.
    float thPrev[64 * 36], thCur[64 * 36];
    double hPrev[32], hCur[32];
    BOOL first;
    long pushes;              // guardPush 횟수(보간 결과에 맞는 정지 판정인지 확인)
    uint8_t lut[256];
} Guard;

enum { kStripes = 4 };

static Guard *guardNew(int W, int H) {
    Guard *g = calloc(1, sizeof(Guard));
    g->W = W; g->H = H;
    g->cnt = calloc(W * H, 1); g->prot = calloc(W * H, 1);
    for (int i = 0; i < 256; i++) g->lut[i] = (uint8_t)(MIN(255, MAX(0, lround((i - 16) * 255.0 / 219.0))) >> 3);   // 히스토그램 칸
    g->first = YES; g->K = 7; g->tau = 3; g->cutSad = 0.10; g->cutHist = 0.30;
    return g;
}
static void guardFree(Guard *g) { if (!g) return; if (g->last) CVPixelBufferRelease(g->last); free(g->cnt); free(g->prot); free(g); }
static void guardReset(Guard *g) {
    g->first = YES; memset(g->cnt, 0, g->W * g->H); memset(g->prot, 0, g->W * g->H);
    if (g->last) { CVPixelBufferRelease(g->last); g->last = NULL; }
}

// 한 줄의 정지 횟수 갱신(분기 없이 써서 NEON 으로 벡터화된다)
static inline void staticRow(const uint8_t *restrict c, const uint8_t *restrict p, uint8_t *restrict cn, uint8_t *restrict pr, int W, int tau, int K) {
    for (int x = 0; x < W; x++) {
        int d = c[x] > p[x] ? c[x] - p[x] : p[x] - c[x];
        int inc = cn[x] + (cn[x] < 255);
        int v = d <= tau ? inc : 0;
        cn[x] = (uint8_t)v; pr[x] = (uint8_t)(v >= K);
    }
}

static void guardPush(Guard *g, CVPixelBufferRef pb) {
    int W = g->W, H = g->H;
    g->pushes++;
    memcpy(g->thPrev, g->thCur, sizeof g->thCur); memcpy(g->hPrev, g->hCur, sizeof g->hCur);
    CVPixelBufferRef lastPb = g->first ? NULL : g->last;
    CVPixelBufferLockBaseAddress(pb, kCVPixelBufferLock_ReadOnly);
    if (lastPb) CVPixelBufferLockBaseAddress(lastPb, kCVPixelBufferLock_ReadOnly);
    const uint8_t *yc = CVPixelBufferGetBaseAddressOfPlane(pb, 0); size_t rc = CVPixelBufferGetBytesPerRowOfPlane(pb, 0);
    const uint8_t *yp = lastPb ? CVPixelBufferGetBaseAddressOfPlane(lastPb, 0) : yc; size_t rp = lastPb ? CVPixelBufferGetBytesPerRowOfPlane(lastPb, 0) : rc;
    // 썸네일: 64×36 칸의 비디오 범위 휘도 평균(가로세로 한 칸 걸러 표본)을 전체 범위로 편다(선형이라 평균 뒤 변환해도 같다).
    float *th = g->thCur;
    for (int ty = 0; ty < 36; ty++) {
        int y0 = ty * H / 36, y1 = (ty + 1) * H / 36;
        for (int tx = 0; tx < 64; tx++) {
            int x0 = tx * W / 64, x1 = (tx + 1) * W / 64; uint32_t sum = 0, n = 0;
            for (int y = y0; y < y1; y += 2) { const uint8_t *row = yc + y * rc; for (int x = x0; x < x1; x += 2) sum += row[x]; n += (uint32_t)((x1 - x0 + 1) / 2); }
            float m = (float)sum / MAX(n, 1); th[ty * 64 + tx] = fminf(255.f, fmaxf(0.f, (m - 16.f) * (255.f / 219.f)));
        }
    }
    // 32칸 히스토그램(가로세로 네 칸 걸러 표본)
    uint32_t hist[32] = {0}; uint32_t tot = 0; const uint8_t *bin = g->lut;
    for (int y = 0; y < H; y += 4) { const uint8_t *row = yc + y * rc; for (int x = 0; x < W; x += 4) { hist[bin[row[x]]]++; tot++; } }
    for (int i = 0; i < 32; i++) g->hCur[i] = (double)hist[i] / MAX(tot, 1);
    if (g->first) { memcpy(g->thPrev, g->thCur, sizeof g->thCur); memcpy(g->hPrev, g->hCur, sizeof g->hCur); }
    uint8_t *cnt = g->cnt, *prot = g->prot; int tau = g->tau, K = g->K;
    dispatch_apply(kStripes, DISPATCH_APPLY_AUTO, ^(size_t st) {
        int ya = (int)st * H / kStripes, yb = ((int)st + 1) * H / kStripes;
        for (int y = ya; y < yb; y++) staticRow(yc + y * rc, yp + y * rp, cnt + (size_t)y * W, prot + (size_t)y * W, W, tau, K);
    });
    if (lastPb) CVPixelBufferUnlockBaseAddress(lastPb, kCVPixelBufferLock_ReadOnly);
    CVPixelBufferUnlockBaseAddress(pb, kCVPixelBufferLock_ReadOnly);
    if (g->last) CVPixelBufferRelease(g->last);
    g->last = CVPixelBufferRetain(pb);
    g->first = NO;
}

static BOOL guardIsCut(Guard *g, double *sadOut, double *histOut) {
    double sad = 0, hd = 0;
    for (int i = 0; i < 64 * 36; i++) sad += fabsf(g->thCur[i] - g->thPrev[i]);
    sad /= 64 * 36 * 255.0;
    for (int i = 0; i < 32; i++) hd += fabs(g->hCur[i] - g->hPrev[i]);
    if (sadOut) *sadOut = sad;
    if (histOut) *histOut = hd;
    return sad >= g->cutSad && hd >= g->cutHist;
}

// 보간 결과(dst)의 정지 화소를 두 원본 평균으로 바꾼다(prev·cur 는 guardPush 로 마지막에 넣은 짝).
static inline void blendRowY(uint8_t *restrict o, const uint8_t *restrict a, const uint8_t *restrict b, const uint8_t *restrict pr, int W) {
    for (int x = 0; x < W; x++) { uint8_t m = (uint8_t)-(pr[x] != 0); uint8_t avg = (uint8_t)((a[x] + b[x] + 1) >> 1); o[x] = (uint8_t)((o[x] & ~m) | (avg & m)); }
}
static inline void blendRowUV(uint8_t *restrict o, const uint8_t *restrict a, const uint8_t *restrict b, const uint8_t *restrict p0, const uint8_t *restrict p1, int W2) {
    for (int cx = 0; cx < W2; cx++) {
        uint8_t m = (uint8_t)-((p0[2 * cx] & p0[2 * cx + 1] & p1[2 * cx] & p1[2 * cx + 1]) != 0);
        uint8_t u = (uint8_t)((a[2 * cx] + b[2 * cx] + 1) >> 1), v = (uint8_t)((a[2 * cx + 1] + b[2 * cx + 1] + 1) >> 1);
        o[2 * cx] = (uint8_t)((o[2 * cx] & ~m) | (u & m)); o[2 * cx + 1] = (uint8_t)((o[2 * cx + 1] & ~m) | (v & m));
    }
}
static void guardApplyNV12(Guard *g, CVPixelBufferRef dst, CVPixelBufferRef prev, CVPixelBufferRef cur) {
    int W = g->W, H = g->H;
    CVPixelBufferLockBaseAddress(dst, 0);
    CVPixelBufferLockBaseAddress(prev, kCVPixelBufferLock_ReadOnly);
    CVPixelBufferLockBaseAddress(cur, kCVPixelBufferLock_ReadOnly);
    uint8_t *dy = CVPixelBufferGetBaseAddressOfPlane(dst, 0), *duv = CVPixelBufferGetBaseAddressOfPlane(dst, 1);
    size_t rdy = CVPixelBufferGetBytesPerRowOfPlane(dst, 0), rduv = CVPixelBufferGetBytesPerRowOfPlane(dst, 1);
    const uint8_t *py = CVPixelBufferGetBaseAddressOfPlane(prev, 0), *cyp = CVPixelBufferGetBaseAddressOfPlane(cur, 0);
    size_t rpy = CVPixelBufferGetBytesPerRowOfPlane(prev, 0), rcy = CVPixelBufferGetBytesPerRowOfPlane(cur, 0);
    const uint8_t *puv = CVPixelBufferGetBaseAddressOfPlane(prev, 1), *cuv = CVPixelBufferGetBaseAddressOfPlane(cur, 1);
    size_t rpuv = CVPixelBufferGetBytesPerRowOfPlane(prev, 1), rcuv = CVPixelBufferGetBytesPerRowOfPlane(cur, 1);
    const uint8_t *prot = g->prot;
    int rowsUV = H / 2;
    dispatch_apply(kStripes, DISPATCH_APPLY_AUTO, ^(size_t st) {
        int ca = (int)st * rowsUV / kStripes, cb = ((int)st + 1) * rowsUV / kStripes;
        for (int c = ca; c < cb; c++) {
            for (int r = 0; r < 2; r++) { int y = c * 2 + r; blendRowY(dy + y * rdy, py + y * rpy, cyp + y * rcy, prot + (size_t)y * W, W); }
            blendRowUV(duv + c * rduv, puv + c * rpuv, cuv + c * rcuv, prot + (size_t)c * 2 * W, prot + (size_t)(c * 2 + 1) * W, W / 2);
        }
    });
    CVPixelBufferUnlockBaseAddress(cur, kCVPixelBufferLock_ReadOnly);
    CVPixelBufferUnlockBaseAddress(prev, kCVPixelBufferLock_ReadOnly);
    CVPixelBufferUnlockBaseAddress(dst, 0);
}

// ───────────────────────────── 마감 서킷브레이커 ─────────────────────────────
// bench.m 의 v2(0.5초 창, 1·2·4·8초 열림, 10초 정상이면 1초로)를 당겨 오기 구조에 맞게 고쳤다(v3).
// bench 는 '도착 + B' 마감이라 처리 시간 흔들림이 곧 늦음이었다. 여기서는 프레임을 앞당겨 받아 마감까지
// 여유(720p60 중앙값 약 100ms)가 있으므로 흔들림은 늦음 표시가 직접 잡는다. 그래서
//   - 늦음(마감 초과·오류) ≥ 창의 10%        → 열림 (v2 와 같음)
//   - 창 평균 처리 시간 > B(입력 간격)         → 열림 (v2 는 0.85·B: 늦음 없이도 자주 열렸다)
//   - 한 건 규칙은 없다. v2 의 '한 건 > 2·B' 는 마감 안에 끝난 72ms 한 건에도 열려 1초씩 보간을 쉬었다.
//     ANE 가 가끔 200~300ms 멈춰도 그동안은 어차피 1건만 처리 중이라 끌 이득이 없다.
//   - 재시도 직후 2건은 창에 넣지 않는다(쉰 뒤 첫 요청은 0.16~1.1초 걸린다).
// v2 규칙은 --breaker v2 로 고를 수 있다. 열린 기간은 완료 시각부터 센다
// (bench 는 제출한 프레임 번호부터 세어, 1초 넘게 걸린 요청 뒤에는 열자마자 닫히는 문제가 있었다).

enum { kRing = 64 };
typedef struct {
    double ring[kRing]; uint8_t lateRing[kRing];
    int n, idx, win, warm, v2;
    double openUntilMs, cooldown, okSinceMs;
    long trips;
    // v3 반쯤 열림: 열린 동안 시험 요청으로 ANE 를 깨워 두고, 시험 결과가 건강할 때만 닫는다.
    double probeRing[16]; int probeN, probeIdx; double nextProbeMs; long probes, extensions;
} Breaker;

// 창만 비운다. 열린 상태·지연 단계(cooldown)·정상 시작 시각은 유지한다(세대가 바뀌어도 ANE 사정은 그대로다).
static void breakerClearWindow(Breaker *b) {
    b->n = 0; b->idx = 0; memset(b->ring, 0, sizeof b->ring); memset(b->lateRing, 0, sizeof b->lateRing);
}
static void breakerSetFps(Breaker *b, double fps) {
    if (b->cooldown <= 0) b->cooldown = 1.0;
    b->win = MIN(kRing, MAX(8, (int)lround(0.5 * fps)));
    breakerClearWindow(b);
}
static BOOL breakerFeed(Breaker *b, double ms, BOOL late, double B) {
    if (!b->v2 && b->warm > 0) { b->warm--; return NO; }   // 재시도 직후 10건은 창에 넣지 않는다
    b->ring[b->idx] = ms; b->lateRing[b->idx] = late; b->idx = (b->idx + 1) % b->win; if (b->n < b->win) b->n++;
    double mean = 0; int lates = 0; for (int i = 0; i < b->n; i++) { mean += b->ring[i]; lates += b->lateRing[i]; } mean /= MAX(b->n, 1);
    BOOL warming = b->warm > 0; if (warming) b->warm--;
    if (b->v2) {
        if (!warming && ms > 2 * B) return YES;
        return b->n == b->win && (mean > 0.85 * B || lates * 10 >= b->win);
    }
    (void)warming;
    return b->n == b->win && (mean > B || lates * 10 >= b->win);
}
static void breakerTrip(Breaker *b, double nowMsV) {
    b->probeN = 0; b->probeIdx = 0; b->nextProbeMs = nowMsV;
    b->openUntilMs = nowMsV + b->cooldown * 1000.0;
    b->cooldown = MIN(b->cooldown * 2, b->v2 ? 8.0 : 30.0); b->n = 0; b->idx = 0; b->trips++; b->okSinceMs = 0;   // v3: 1·2·4·8·16·30초(다시 시도할 때마다 켜졌다 꺼지는 전환을 줄인다)
    memset(b->lateRing, 0, sizeof b->lateRing); b->warm = b->v2 ? 2 : 10;
}

static void breakerProbeDone(Breaker *b, double ms, double nowMsV) {
    b->probeRing[b->probeIdx] = ms; b->probeIdx = (b->probeIdx + 1) % 16; if (b->probeN < 16) b->probeN++;
    b->nextProbeMs = nowMsV + MAX(100.0, 4.0 * ms);   // ANE 사용률 20% 이하
}
// 열려 있으면 YES. v3 반쯤 열림:
// - 열린 동안 시험 요청(결과는 버림, ANE 사용률 20% 이하)으로 ANE 를 깨워 둔다. 오래 쉬면 다시 켠 직후 13~23ms 로 느려져
//   대기열이 밀리고 곧 다시 열리는 악순환이 생겼다(치지직 5분에 34번).
// - 시험 요청은 드문드문이라 ANE 가 클럭을 낮춘 상태의 시간이다(720p 약 15~18ms, 연속 처리 땐 9ms). 그래서 시험 결과로는
//   '아예 못 따라감'(최근 시험 중앙값 > 2·B, 1080p 는 약 100ms)만 판정해 열린 기간을 1초씩 늘리고,
//   그 밖에는 기간이 끝나면 닫고 실제 보간으로 시도한다. 닫은 직후 10건은 창에 넣지 않는다(ANE 가 다시 빨라질 시간).
static BOOL breakerIsOpen(Breaker *b, double nowMsV, double B) {
    if (b->openUntilMs <= 0) return NO;
    if (nowMsV < b->openUntilMs) return YES;
    if (!b->v2) {
        int n = MIN(b->probeN, 9);
        if (n >= 3) {
            double t[9]; for (int i = 0; i < n; i++) t[i] = b->probeRing[(b->probeIdx - 1 - i + 16) % 16];
            qsort(t, n, sizeof(double), cmpd);
            if (t[n / 2] > 2 * B) { b->openUntilMs = nowMsV + 1000; b->extensions++; return YES; }
        }
        b->warm = 10;
    }
    b->openUntilMs = 0;
    return NO;
}

// ───────────────────────────── VT 세션 ─────────────────────────────

@interface VFSession : NSObject {
@public
    int W, H;
    VTFrameProcessor *proc;
    CVPixelBufferPoolRef pool;
    NSDictionary *auxAttrs;
    _Atomic int state;          // 0 준비 중, 1 준비됨, 2 실패
    double startMs, warmMs;
    NSString *failure;
    double vtOffset; uint64_t offsetGen; double lastVt;
    // 복사 방식: 디코더 버퍼를 VT 가 거부하면(-19740 등) VT 가 요구하는 속성의 버퍼로 옮겨 넣는다.
    BOOL copyMode; int directErrors;
    CVPixelBufferPoolRef srcPool; VTPixelTransferSessionRef xfer;
}
@end
@implementation VFSession
- (void)dealloc {
    if (pool) CVPixelBufferPoolRelease(pool);
    if (srcPool) CVPixelBufferPoolRelease(srcPool);
    if (xfer) { VTPixelTransferSessionInvalidate(xfer); CFRelease(xfer); }
}
@end

static void fillGray(CVPixelBufferRef pb, uint8_t yv) {
    CVPixelBufferLockBaseAddress(pb, 0);
    for (size_t p = 0; p < CVPixelBufferGetPlaneCount(pb); p++) {
        uint8_t *b = CVPixelBufferGetBaseAddressOfPlane(pb, p); size_t rb = CVPixelBufferGetBytesPerRowOfPlane(pb, p), h = CVPixelBufferGetHeightOfPlane(pb, p);
        memset(b, p == 0 ? yv : 128, rb * h);
    }
    CVPixelBufferUnlockBaseAddress(pb, 0);
}

static CVPixelBufferRef makeFromAttrs(NSDictionary *attrs, int w, int h) {
    NSMutableDictionary *a = [attrs mutableCopy] ?: [NSMutableDictionary new];
    a[(id)kCVPixelBufferWidthKey] = @(w); a[(id)kCVPixelBufferHeightKey] = @(h);
    if (!a[(id)kCVPixelBufferIOSurfacePropertiesKey]) a[(id)kCVPixelBufferIOSurfacePropertiesKey] = @{};
    id fmt = a[(id)kCVPixelBufferPixelFormatTypeKey]; OSType f = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
    if ([fmt isKindOfClass:[NSNumber class]]) f = [fmt unsignedIntValue];
    else if ([fmt isKindOfClass:[NSArray class]] && [fmt count]) f = [[fmt firstObject] unsignedIntValue];
    [a removeObjectForKey:(id)kCVPixelBufferPixelFormatTypeKey];
    CVPixelBufferRef pb = NULL;
    if (CVPixelBufferCreate(kCFAllocatorDefault, w, h, f, (__bridge CFDictionaryRef)a, &pb) != kCVReturnSuccess) return NULL;
    return pb;
}

// 세션을 시작하고 처리 3회로 실제 동작을 확인한다(구성·세션 시작이 성공해도 첫 처리에서 실패하는 크기가 있다).
static void sessionStart(VFSession *s) {
    if (!VTLowLatencyFrameInterpolationConfiguration.isSupported) { s->failure = @"isSupported=NO"; s->state = 2; return; }
    if (@available(macOS 27.0, *)) {
        NSInteger edge = [VTLowLatencyFrameInterpolationConfiguration maximumDimensionForSpatialScaleFactor:1];
        NSInteger area = [VTLowLatencyFrameInterpolationConfiguration maximumPixelCountForSpatialScaleFactor:1];
        if (edge > 0 && (s->W > edge || s->H > edge || (NSInteger)s->W * s->H > area)) {
            s->failure = [NSString stringWithFormat:@"크기 상한 초과(최대 변 %ld, 최대 화소 %ld)", (long)edge, (long)area]; s->state = 2; return;
        }
    }
    VTLowLatencyFrameInterpolationConfiguration *cfg = [[VTLowLatencyFrameInterpolationConfiguration alloc] initWithFrameWidth:s->W frameHeight:s->H numberOfInterpolatedFrames:1];
    if (!cfg) { s->failure = @"구성 실패"; s->state = 2; return; }
    VTFrameProcessor *p = [VTFrameProcessor new]; NSError *err = nil;
    double t0 = nowMs();
    if (![p startSessionWithConfiguration:cfg error:&err]) { s->failure = [NSString stringWithFormat:@"세션 시작 실패: %@", err ? err.localizedDescription : @"NSError 없음(샌드박스 안에서 실행했는지 확인)"]; s->state = 2; return; }
    s->startMs = nowMs() - t0;
    NSMutableDictionary *pa = [cfg.destinationPixelBufferAttributes mutableCopy];
    if (!pa[(id)kCVPixelBufferIOSurfacePropertiesKey]) pa[(id)kCVPixelBufferIOSurfacePropertiesKey] = @{};
    pa[(id)kCVPixelBufferWidthKey] = @(s->W); pa[(id)kCVPixelBufferHeightKey] = @(s->H);
    CVPixelBufferPoolRef pool = NULL;
    if (CVPixelBufferPoolCreate(NULL, NULL, (__bridge CFDictionaryRef)pa, &pool) != kCVReturnSuccess) { [p endSession]; s->failure = @"출력 풀 생성 실패"; s->state = 2; return; }
    s->pool = pool; s->auxAttrs = @{ (id)kCVPixelBufferPoolAllocationThresholdKey: @(gOpt.poolCap) };
    NSMutableDictionary *sa = [cfg.sourcePixelBufferAttributes mutableCopy];
    if (!sa[(id)kCVPixelBufferIOSurfacePropertiesKey]) sa[(id)kCVPixelBufferIOSurfacePropertiesKey] = @{};
    sa[(id)kCVPixelBufferWidthKey] = @(s->W); sa[(id)kCVPixelBufferHeightKey] = @(s->H);
    CVPixelBufferPoolRef sp = NULL;
    if (CVPixelBufferPoolCreate(NULL, NULL, (__bridge CFDictionaryRef)sa, &sp) == kCVReturnSuccess) s->srcPool = sp;
    VTPixelTransferSessionRef x = NULL;
    if (VTPixelTransferSessionCreate(NULL, &x) == noErr) s->xfer = x;
    CVPixelBufferRef a = makeFromAttrs(cfg.sourcePixelBufferAttributes, s->W, s->H), b = makeFromAttrs(cfg.sourcePixelBufferAttributes, s->W, s->H);
    CVPixelBufferRef d = NULL; CVPixelBufferPoolCreatePixelBuffer(NULL, pool, &d);
    if (!a || !b || !d) { if (a) CVPixelBufferRelease(a); if (b) CVPixelBufferRelease(b); if (d) CVPixelBufferRelease(d); [p endSession]; s->failure = @"준비 버퍼 생성 실패"; s->state = 2; return; }
    fillGray(a, 100); fillGray(b, 140);
    double t1 = nowMs(); BOOL ok = YES;
    for (int i = 1; i <= 3 && ok; i++) {
        VTFrameProcessorFrame *fa = [[VTFrameProcessorFrame alloc] initWithBuffer:(i % 2 ? a : b) presentationTimeStamp:tsec(i - 1)];
        VTFrameProcessorFrame *fb = [[VTFrameProcessorFrame alloc] initWithBuffer:(i % 2 ? b : a) presentationTimeStamp:tsec(i)];
        VTFrameProcessorFrame *fd = [[VTFrameProcessorFrame alloc] initWithBuffer:d presentationTimeStamp:tsec(i - 0.5)];
        VTLowLatencyFrameInterpolationParameters *prm = [[VTLowLatencyFrameInterpolationParameters alloc] initWithSourceFrame:fb previousFrame:fa interpolationPhase:@[@0.5] destinationFrames:@[fd]];
        NSError *e = nil;
        if (!fa || !fb || !fd || !prm || ![p processWithParameters:prm error:&e]) { ok = NO; s->failure = [NSString stringWithFormat:@"준비 처리 실패: %@", e ? e.localizedDescription : @"매개변수 생성 실패"]; }
    }
    s->warmMs = nowMs() - t1;
    CVPixelBufferRelease(a); CVPixelBufferRelease(b); CVPixelBufferRelease(d);
    if (!ok) { [p endSession]; s->state = 2; return; }
    s->lastVt = 3.0; s->offsetGen = UINT64_MAX;
    s->copyMode = gOpt.vtInput == 2;
    s->proc = p; s->state = 1;
}

// ───────────────────────────── 출력(싱크) ─────────────────────────────

typedef struct { CVPixelBufferRef pb; double releaseMs; } HeldFrame;

@interface VFSink : NSObject {
@public
    SinkKind kind;
    AVSampleBufferRenderSynchronizer *sync;
    id<AVQueuedSampleBufferRendering> renderer;   // AVSampleBufferVideoRenderer
    AVSampleBufferDisplayLayer *layer;
    CMVideoFormatDescriptionRef fmt;
    HeldFrame *held; int nHeld, capHeld;
    long enqueued, enqueueFailed, resyncs;
    double lastSyncCheckMs;
}
@end
@implementation VFSink
@end

static VFSink *gSink;

// ───────────────────────────── 전역 상태 ─────────────────────────────

static dispatch_queue_t gPipe;                 // 보간 파이프라인 상태는 모두 이 직렬 큐에서만 바꾼다
static os_unfair_lock gLock = OS_UNFAIR_LOCK_INIT;
static AVPlayerItemVideoOutput *gOutput;       // gLock
static CMTimebaseRef gItemTB;                  // gLock (보유)
static _Atomic int gFifoCount;
static _Atomic int gPullerReset;
static _Atomic BOOL gStop;
static _Atomic BOOL gInterpOn = YES;           // 창 모드에서 i 키로 바꾼다

typedef struct {
    long frames, pairs, eligible, onTime, late, lateStart, abandoned, errors, cuts, gaps, bypass, sessionWait, unsupported,
         poolExhausted, passOff, warmPairs, stalledPairs, staleDropped, orderViolation, origLate, emittedOrig, emittedMid,
         bumps, toggles, lateCompleted, compositeLate, fpsChanges, sizeChanges, staticSkipped, vtRestarts,
         countedErrors, staleCounted, runtimeOff, softBumps, coalescedBumps;
} Counters;

typedef struct {
    // 보간 파이프라인(gPipe 전용)
    uint64_t gen;
    NSMutableArray<VFFrame *> *fifo;
    VFFrame *prev;            // 마지막으로 내보낸 원본(다음 짝의 앞 프레임)
    VFFrame *lastRecv;        // 마지막으로 받은 프레임(간격 계산)
    VFPair *inflight;
    VFSession *session;
    NSMutableDictionary<NSString *, VFSession *> *sessions;   // "WxH" → 세션(실패 판정 기억)
    Guard *guard;
    Breaker br;
    double T, Tsetup;          // 입력 간격(ms): 최근 9개 간격의 최솟값, 설정 시점 값
    double dts[9]; int nDt, dtIdx;
    int fpsMismatchRun;
    double warmUntilMs;
    int lastShown;             // 토글 계산: -1 미정, 0 원본만, 1 보간 표시
    CMTime lastEmitPTS;
    Counters c;
    DArr procMs, procAtMs, compMs, guardMs, lookMs, slackMs, submitLeadMs, probeMs;   // procAtMs: 완료 시각
    NSMutableArray *events;    // 세대 변경·세션 시작 등
    NSMutableDictionary *sessionLog;
    double cutSadMaxNormal, cutHistMaxNormal;
    int consecutiveErrors;
    double countedMs;          // 집계 대상 짝이 차지한 재생 시간(짝마다 입력 간격 T)
    double lastHardBumpMs; uint64_t lastHardBumpGen;
    BOOL frameHasExtended;     // 디코더 버퍼에 추가 픽셀(패딩 줄)이 있다
} Pipe;

static Pipe P;

static NSString *gSourceDesc = @"";
static BOOL gIsLive = NO;

static void logEvent(NSString *kind, NSDictionary *info) {
    NSMutableDictionary *e = [info mutableCopy] ?: [NSMutableDictionary new];
    e[@"kind"] = kind; e[@"t_ms"] = @(lround(nowMs()));
    [P.events addObject:e];
    if (gOpt.verbose) printf("  · %s %s\n", kind.UTF8String, [[e description] stringByReplacingOccurrencesOfString:@"\n" withString:@" "].UTF8String);
}

// 아이템 시간 → 호스트 시각(ms). 정지·끊김(속도 0)이면 무한대.
static double hostMsOfItemTime(CMTime x) {
    os_unfair_lock_lock(&gLock);
    CMTimebaseRef tb = gItemTB ? (CMTimebaseRef)CFRetain(gItemTB) : NULL;
    os_unfair_lock_unlock(&gLock);
    if (!tb) return INFINITY;
    CMTime t; Float64 r = 0; CMTimebaseGetTimeAndRate(tb, &t, &r);
    double now = nowMs();
    CFRelease(tb);
    if (r <= 0.01 || !CMTIME_IS_NUMERIC(t)) return INFINITY;
    return now + (sec(x) - sec(t)) * 1000.0 / r;
}

// ───────────────────────────── 싱크 동작(gPipe) ─────────────────────────────

static void sinkSyncToItem(BOOL force) {
    VFSink *s = gSink; if (!s || !s->sync) return;
    double now = nowMs();
    if (!force && now - s->lastSyncCheckMs < 50) return;
    s->lastSyncCheckMs = now;
    os_unfair_lock_lock(&gLock);
    CMTimebaseRef tb = gItemTB ? (CMTimebaseRef)CFRetain(gItemTB) : NULL;
    os_unfair_lock_unlock(&gLock);
    if (!tb) return;
    CMTime it; Float64 ir = 0; CMTimebaseGetTimeAndRate(tb, &it, &ir);
    CMTime st; Float64 sr = 0; CMTimebaseGetTimeAndRate(s->sync.timebase, &st, &sr);
    CFRelease(tb);
    if (!CMTIME_IS_NUMERIC(it)) return;
    double drift = CMTIME_IS_NUMERIC(st) ? fabs(sec(it) - sec(st)) * 1000.0 : INFINITY;
    if (force || fabs(ir - sr) > 1e-4 || drift > 2.0) {
        CMTime host = CMClockGetTime(CMClockGetHostTimeClock());
        os_unfair_lock_lock(&gLock);
        tb = gItemTB ? (CMTimebaseRef)CFRetain(gItemTB) : NULL;
        os_unfair_lock_unlock(&gLock);
        if (!tb) return;
        CMTime t2 = CMSyncConvertTime(host, CMClockGetHostTimeClock(), tb); Float64 r2 = CMTimebaseGetRate(tb);
        CFRelease(tb);
        [s->sync setRate:(float)r2 time:t2 atHostTime:host];
        s->resyncs++;
    }
}

static void sinkReleaseDue(void) {
    VFSink *s = gSink; if (!s || s->kind != SinkNull) return;
    double now = nowMs(); int k = 0;
    for (int i = 0; i < s->nHeld; i++) {
        if (s->held[i].releaseMs <= now) CVPixelBufferRelease(s->held[i].pb);
        else s->held[k++] = s->held[i];
    }
    s->nHeld = k;
}

static void sinkFlush(void) {
    VFSink *s = gSink; if (!s) return;
    if (s->kind == SinkNull) { for (int i = 0; i < s->nHeld; i++) CVPixelBufferRelease(s->held[i].pb); s->nHeld = 0; }
    else if ([(id)s->renderer respondsToSelector:@selector(flushWithRemovalOfDisplayedImage:completionHandler:)]) [(AVSampleBufferVideoRenderer *)s->renderer flushWithRemovalOfDisplayedImage:NO completionHandler:nil];
    else [s->renderer flush];
    P.lastEmitPTS = kCMTimeInvalid;
}

static void sinkEmit(CVPixelBufferRef pb, CMTime pts, BOOL interpolated) {
    if (CMTIME_IS_NUMERIC(P.lastEmitPTS) && CMTimeCompare(pts, P.lastEmitPTS) <= 0) {
        if (P.c.orderViolation++ < 20) logEvent(@"order_violation", @{ @"pts": @(sec(pts)), @"last": @(sec(P.lastEmitPTS)), @"mid": @(interpolated) });
        return;
    }
    P.lastEmitPTS = pts;
    double displayMs = hostMsOfItemTime(pts);
    if (interpolated) P.c.emittedMid++; else { P.c.emittedOrig++; if (isfinite(displayMs) && nowMs() > displayMs) P.c.origLate++; }
    VFSink *s = gSink;
    if (s->kind == SinkNull) {
        if (s->nHeld == s->capHeld) { s->capHeld = s->capHeld ? s->capHeld * 2 : 64; s->held = realloc(s->held, s->capHeld * sizeof(HeldFrame)); }
        // 표시 시각 + 입력 한 간격 동안 붙잡는다(실제 출력 큐가 버퍼를 쥐는 시간을 흉내).
        double rel = isfinite(displayMs) ? displayMs + MAX(P.T, 8) : nowMs() + 500;
        s->held[s->nHeld++] = (HeldFrame){ CVPixelBufferRetain(pb), rel };
        s->enqueued++;
        return;
    }
    if (!s->fmt || !CMVideoFormatDescriptionMatchesImageBuffer(s->fmt, pb)) {
        if (s->fmt) CFRelease(s->fmt);
        s->fmt = NULL; CMVideoFormatDescriptionCreateForImageBuffer(NULL, pb, &s->fmt);
    }
    CMSampleTimingInfo ti = { kCMTimeInvalid, pts, kCMTimeInvalid };
    CMSampleBufferRef sb = NULL;
    if (!s->fmt || CMSampleBufferCreateReadyWithImageBuffer(NULL, pb, s->fmt, &ti, &sb) != noErr || !sb) { s->enqueueFailed++; return; }
    [s->renderer enqueueSampleBuffer:sb];
    CFRelease(sb);
    s->enqueued++;
}

// ───────────────────────────── 파이프라인(gPipe) ─────────────────────────────

static void pipeAdvance(void);
static void emitOrig(VFFrame *f);

static void setupForFps(double T) {
    P.Tsetup = T;
    double fps = 1000.0 / T;
    if (P.guard) P.guard->K = MAX(2, (int)lround(gOpt.staticMs * fps / 1000.0));
    breakerSetFps(&P.br, fps);
}

static void markShown(int shown) {
    if (P.lastShown >= 0 && shown != P.lastShown) P.c.toggles++;
    P.lastShown = shown;
}

// 처리 중인 짝을 결과 없이 정리한다(세대 변경). 집계 대상인데 아직 결과가 없으면 stale 로 센다.
static void resolveInflightAsStale(void) {
    VFPair *pr = P.inflight;
    if (!pr) return;
    if (pr->counted && !pr->resolved) { P.c.staleCounted++; pr->resolved = YES; }
    pr->abandoned = YES;   // 완료되면 세대가 달라 버린다
}

// hard: 탐색·출력 비움·시간 점프·거꾸로 가는 PTS·재연결 — 앞서 받은 프레임은 새 화면과 이어지지 않는다.
//       미처리 프레임을 버리고, 출력을 비우고, 받기 스레드를 처음부터 다시 받게 한다.
//       한 번의 탐색이 time_jumped 와 output_flushed 를 함께 보내므로 150ms 안의 두 번째 hard 변경은 합친다.
// soft: 크기·프레임률 변화 — 재생은 이어진다. 처리 중인 짝의 원본과 미처리 프레임을 원본으로 내보내고
//       출력·받기는 건드리지 않는다.
static void bumpGeneration(NSString *why, BOOL hard) {
    double now = nowMs();
    if (hard && P.lastHardBumpGen == P.gen && now - P.lastHardBumpMs < 150 && ![why isEqual:@"backward_pts"]) {   // 프레임이 직접 보여 준 역행은 합치지 않는다
        P.c.coalescedBumps++;
        logEvent(@"generation_coalesced", @{ @"gen": @(P.gen), @"why": why });
        return;
    }
    VFPair *pr = P.inflight;
    if (!hard && pr && !pr->abandoned && pr->gen == P.gen) emitOrig(pr->cur);   // 원본이 아직 안 나갔다
    resolveInflightAsStale();
    for (VFFrame *f in P.fifo) { if (!hard) emitOrig(f); atomic_fetch_sub(&gFifoCount, 1); }
    [P.fifo removeAllObjects];
    P.gen++;
    P.c.bumps++; if (!hard) P.c.softBumps++;
    P.prev = nil;
    if (hard) P.lastRecv = nil;
    if (P.guard) guardReset(P.guard);
    P.nDt = 0; P.dtIdx = 0; P.fpsMismatchRun = 0;
    breakerClearWindow(&P.br);
    P.warmUntilMs = now + 2000;
    P.lastShown = -1;
    if (hard) {
        sinkFlush();
        atomic_store(&gPullerReset, 1);
        P.lastHardBumpMs = now; P.lastHardBumpGen = P.gen;
    }
    sinkSyncToItem(YES);
    logEvent(@"generation", @{ @"gen": @(P.gen), @"why": why, @"hard": @(hard) });
}

static void requestSession(int W, int H) {
    NSString *key = [NSString stringWithFormat:@"%dx%d", W, H];
    VFSession *s = P.sessions[key];
    if (!s) {
        s = [VFSession new]; s->W = W; s->H = H; s->state = 0;
        P.sessions[key] = s;
        logEvent(@"session_request", @{ @"size": key });
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
            sessionStart(s);
            dispatch_async(gPipe, ^{
                NSMutableDictionary *info = [@{ @"size": key, @"ok": @(s->state == 1), @"start_ms": @(s->startMs), @"warm3_ms": @(s->warmMs) } mutableCopy];
                if (s->failure) info[@"failure"] = s->failure;
                P.sessionLog[key] = [info copy];
                logEvent(@"session_ready", info);
                printf("  · VT 세션 %s: %s\n", key.UTF8String, s->state == 1 ? [NSString stringWithFormat:@"준비됨(시작 %.0fms, 준비 처리 3회 %.0fms)", s->startMs, s->warmMs].UTF8String : s->failure.UTF8String);
                pipeAdvance();
            });
        });
    }
    P.session = s;
}

static void onFrame(VFFrame *f) {
    P.c.frames++;
    dpush(&P.lookMs, f->lookMs);
    // 크기 변화 → soft 세대 변경·세션 다시 준비
    if (!P.guard || P.guard->W != f->w || P.guard->H != f->h) {
        BOOL first = P.guard == NULL;
        {   // 새 크기 첫 프레임의 버퍼 형식(VT 가 디코더 버퍼를 거부할 때 원인을 찾으려고)
            size_t el = 0, er = 0, et = 0, eb = 0; CVPixelBufferGetExtendedPixels(f->pb, &el, &er, &et, &eb);
            OSType fmt = CFSwapInt32HostToBig(CVPixelBufferGetPixelFormatType(f->pb));
            id mtx = CFBridgingRelease(CVBufferCopyAttachment(f->pb, kCVImageBufferYCbCrMatrixKey, NULL));
            logEvent(@"frame_format", @{ @"size": [NSString stringWithFormat:@"%dx%d", f->w, f->h], @"fmt": [[NSString alloc] initWithBytes:&fmt length:4 encoding:NSASCIIStringEncoding] ?: @"?",
                                         @"bpr0": @(CVPixelBufferGetBytesPerRowOfPlane(f->pb, 0)), @"bpr1": @(CVPixelBufferGetBytesPerRowOfPlane(f->pb, 1)),
                                         @"h0": @(CVPixelBufferGetHeightOfPlane(f->pb, 0)), @"ext": [NSString stringWithFormat:@"%zu,%zu,%zu,%zu", el, er, et, eb],
                                         @"iosurface": @(CVPixelBufferGetIOSurface(f->pb) != NULL), @"matrix": [mtx description] ?: @"" });
        }
        if (!first) P.c.sizeChanges++;
        bumpGeneration([NSString stringWithFormat:@"size %dx%d", f->w, f->h], NO);
        { size_t el = 0, er = 0, et = 0, eb = 0; CVPixelBufferGetExtendedPixels(f->pb, &el, &er, &et, &eb); P.frameHasExtended = (el | er | et | eb) != 0; }
        guardFree(P.guard); P.guard = guardNew(f->w, f->h);
        P.guard->tau = gOpt.staticTau; P.guard->cutSad = gOpt.cutSad; P.guard->cutHist = gOpt.cutHist;
        if (P.Tsetup > 0) P.guard->K = MAX(2, (int)lround(gOpt.staticMs * (1000.0 / P.Tsetup) / 1000.0));
        if (gOpt.interp) requestSession(f->w, f->h);
    }
    if (P.lastRecv) {
        int cmp = CMTimeCompare(f->pts, P.lastRecv->pts);
        if (cmp <= 0) { bumpGeneration(@"backward_pts", YES); }
        else {
            double dt = (sec(f->pts) - sec(P.lastRecv->pts)) * 1000.0;
            if (dt < 200) {   // 탐색·불연속으로 생긴 큰 간격은 간격 추정에 넣지 않는다
                P.dts[P.dtIdx] = dt; P.dtIdx = (P.dtIdx + 1) % 9; if (P.nDt < 9) P.nDt++;
                // 입력 간격 = 최근 9개 간격의 최솟값. 프레임이 빠지면 간격이 길어지기만 하므로 중앙값은 30fps 로 잘못 읽는다.
                double med = P.dts[0]; for (int i = 1; i < P.nDt; i++) med = MIN(med, P.dts[i]);
                if (P.Tsetup <= 0 && P.nDt >= 3) { P.T = med; setupForFps(med); logEvent(@"fps", @{ @"T_ms": @(med) }); }
                else if (P.Tsetup > 0) {
                    P.T = med;
                    if (P.nDt == 9 && fabs(med - P.Tsetup) / P.Tsetup > 0.25) {
                        if (++P.fpsMismatchRun >= 3) {
                            P.c.fpsChanges++;
                            logEvent(@"fps", @{ @"T_ms": @(med), @"was_ms": @(P.Tsetup) });
                            setupForFps(med);
                            bumpGeneration([NSString stringWithFormat:@"fps → %.2f", 1000.0 / med], NO);
                            P.T = med;
                        }
                    } else P.fpsMismatchRun = 0;
                }
            }
        }
    }
    f->gen = P.gen;
    P.lastRecv = f;
    [P.fifo addObject:f];
    pipeAdvance();
}

static void emitOrig(VFFrame *f) {
    sinkEmit(f->pb, f->pts, NO);
    P.prev = f;
}

// 제출 판단에 쓰는 예상 처리 시간: 최근 3초 안에 끝난 요청(최대 60건)의 p75.
// - p10 을 쓰면 느린 요청 뒤 쌓인 짝을 마감 직전에 넣어 11ms 만에 끝나도 늦음이 되고, 그 늦음이 브레이커를 다시 열었다.
// - 개수로만 자르면 느린 표본이 몰린 뒤 값이 높게 굳는다: 그 값으로는 아무 짝도 넣지 못하고, 넣지 않으니 표본도
//   갱신되지 않는다(치지직 e5: '시작 못함' 10,716건). 그래서 시간으로 자르고, 최근 표본이 5건 미만이면 0.5·T 로 다시 시도한다.
static double recentMinProc(double now, double T) {
    double tmp[60]; int n = 0;
    for (long i = P.procMs.n - 1; i >= 0 && n < 60; i--) { if (P.procAtMs.v[i] < now - 3000) break; tmp[n++] = P.procMs.v[i]; }
    if (n < 5) return 0.5 * T;
    qsort(tmp, n, sizeof(double), cmpd);
    double pos = 0.75 * (n - 1); int i0 = (int)floor(pos); double f = pos - i0;
    return i0 + 1 < n ? tmp[i0] * (1 - f) + tmp[i0 + 1] * f : tmp[i0];
}

// 집계 대상 짝의 '보간하지 못함' 결과를 센다.
static void countSkip(long *counter, BOOL counted) { if (counted) { (*counter)++; markShown(0); } }

// VT 오류(포기된 짝 포함). 5번 이어지면 세션을 버리고 다시 시작한다.
static void handleVtError(VFPair *pr, NSError *e) {
    P.c.errors++; P.consecutiveErrors++;
    if (P.c.errors <= 50) logEvent(@"vt_error", @{ @"code": @(e.code), @"msg": e.localizedDescription ?: @"", @"abandoned": @(pr->abandoned), @"copy": @(pr->srcA != NULL) });
    VFSession *ss = pr->session;
    if (ss && gOpt.vtInput != 1 && !ss->copyMode && !pr->srcA && ++ss->directErrors >= 3 && ss->srcPool && ss->xfer) {   // 디코더 버퍼를 거부하면 복사 방식으로
        ss->copyMode = YES; P.consecutiveErrors = 0;
        logEvent(@"vt_copy_mode", @{ @"size": [NSString stringWithFormat:@"%dx%d", ss->W, ss->H], @"code": @(e.code) });
        printf("  · VT 가 %dx%d 디코더 버퍼를 거부(%ld)해 복사 방식으로 바꿉니다\n", ss->W, ss->H, (long)e.code);
        return;
    }
    if (P.consecutiveErrors >= 5 && pr->session && P.session == pr->session) {
        NSString *key = [NSString stringWithFormat:@"%dx%d", pr->session->W, pr->session->H];
        [pr->session->proc endSession]; pr->session->proc = nil; pr->session->state = 2;
        [P.sessions removeObjectForKey:key]; P.consecutiveErrors = 0; P.c.vtRestarts++;
        logEvent(@"vt_restart", @{ @"size": key, @"n": @(P.c.vtRestarts) });
        if (P.c.vtRestarts <= 2) requestSession(pr->session->W, pr->session->H);
        else { VFSession *dead = [VFSession new]; dead->W = pr->session->W; dead->H = pr->session->H; dead->state = 2; dead->failure = @"VT 오류가 이어져 이 크기는 보간하지 않음"; P.sessions[key] = dead; P.session = dead; }
    }
}

static void breakerAfter(VFPair *pr, double ms, BOOL late, double fin) {
    if (!gOpt.useBreaker || !pr->counted) return;   // 준비 구간(세대 시작 2초)·끊김 중 결과는 브레이커 판단에서 뺀다
    double T = P.T > 0 ? P.T : 16.67;
    // v3: 요청 자체가 느렸던 늦음만 센다. 앞 요청이 느려 대기열에서 늦게 출발한 짝(처리는 빨랐음)은 ANE 가 못 따라간다는 증거가 아니다.
    BOOL lateForBreaker = P.br.v2 ? late : (late && ms > 0.75 * T);
    if (breakerFeed(&P.br, ms, lateForBreaker, T)) { breakerTrip(&P.br, nowMs()); logEvent(@"breaker_trip", @{ @"ms": @(ms), @"late": @(late), @"abandoned": @(pr->abandoned) }); }
    else if (P.br.n == P.br.win) { if (!P.br.okSinceMs) P.br.okSinceMs = fin; else if (fin - P.br.okSinceMs > 10000) P.br.cooldown = 1.0; }
}

static void onComplete(VFPair *pr, NSError *e, double done) {
    double ms = done - pr->submitMs;
    if (P.inflight == pr) P.inflight = nil;
    if (e) handleVtError(pr, e); else P.consecutiveErrors = 0;
    if (pr->probe) { if (!e) breakerProbeDone(&P.br, ms, nowMs()); dpush(&P.probeMs, ms); pipeAdvance(); return; }   // 시험 요청은 procMs 에 넣지 않는다
    if (pr->gen != P.gen) { P.c.staleDropped++; pipeAdvance(); return; }
    if (!e) { dpush(&P.procMs, ms); dpush(&P.procAtMs, done); }   // 처리 시간 분포·제출 판단: 성공한 실제 요청만
    if (pr->abandoned) {   // 마감을 넘겨 원본을 이미 내보냈다
        P.c.lateCompleted++;
        breakerAfter(pr, ms, YES, nowMs());
        pipeAdvance(); return;
    }
    BOOL ok = !e;
    double tc = nowMs();
    if (ok && gOpt.useStatic && P.guard && P.guard->W == pr->cur->w && P.guard->H == pr->cur->h) {
        if (P.guard->pushes == pr->guardPushes) guardApplyNV12(P.guard, pr->dst, pr->prev->pb, pr->cur->pb);
        else P.c.staticSkipped++;
    }
    if (ok) CVBufferPropagateAttachments(pr->cur->pb, pr->dst);
    double fin = nowMs();
    if (ok) dpush(&P.compMs, fin - tc);
    double dl = hostMsOfItemTime(pr->mid) - gOpt.leadMs;
    BOOL inTime = ok && fin <= dl;
    if (ok && !inTime) P.c.compositeLate++;
    if (ok && isfinite(dl)) dpush(&P.slackMs, dl - fin);
    if (pr->counted && !pr->resolved) {
        pr->resolved = YES;
        if (inTime) { P.c.onTime++; markShown(1); }
        else if (!ok) { P.c.countedErrors++; markShown(0); }
        else { P.c.late++; markShown(0); }
    }
    if (inTime) sinkEmit(pr->dst, pr->mid, YES);
    emitOrig(pr->cur);
    breakerAfter(pr, ms, !inTime, fin);
    pipeAdvance();
}

// fifo 맨 앞 프레임 하나를 처리한다. 진행했으면 YES.
// 짝은 한 곳에서 같은 순서로 분류한다: 간격 이상 → 장면 전환 → 실행 중 보간 끔 → 준비 구간·끊김(집계 제외) → 결과.
// 집계 대상 짝의 결과(on_time + late + late_start + bypass + session_wait + unsupported + pool_exhausted
// + counted_errors + stale_counted + interp_off)는 eligible 과 같아야 한다(요약의 accounting_ok).
static BOOL pipeStep(void) {
    if (P.fifo.count == 0) return NO;
    VFFrame *cur = P.fifo.firstObject;
    BOOL needGuard = gOpt.useCut || gOpt.useStatic;
    if (!P.prev || P.prev->gen != cur->gen) {   // 세대의 첫 프레임
        [P.fifo removeObjectAtIndex:0]; atomic_fetch_sub(&gFifoCount, 1);
        if (P.guard && needGuard) guardPush(P.guard, cur->pb);
        emitOrig(cur); return YES;
    }
    // 처리 중인 짝의 원본이 아직 안 나갔으면 기다린다(먼저 내보내면 순서가 뒤집힌다).
    // 그 짝이 마감을 넘겨 포기되면(pipeTick) 원본이 나가고 P.prev 가 바뀐다.
    if (P.inflight && !P.inflight->abandoned && P.inflight->gen == P.gen) return NO;
    double T = P.T > 0 ? P.T : 16.67;
    double dt = (sec(cur->pts) - sec(P.prev->pts)) * 1000.0;
    CMTime mid = CMTimeAdd(P.prev->pts, CMTimeMultiplyByRatio(CMTimeSubtract(cur->pts, P.prev->pts), 1, 2));
    double deadline = hostMsOfItemTime(mid) - gOpt.leadMs;
    BOOL stalled = !isfinite(deadline);
    double T0 = P.T > 0 ? P.T : 16.67;
    double now = nowMs(), minProc = recentMinProc(now, T0);
    BOOL vtBusy = P.inflight != nil;   // 포기됐거나 이전 세대 요청을 VT 가 아직 처리 중
    BOOL canStartInTime = stalled || now + minProc <= deadline;
    if (vtBusy && canStartInTime) return NO;   // VT 가 비면 이 짝을 처리할 수 있다
    [P.fifo removeObjectAtIndex:0]; atomic_fetch_sub(&gFifoCount, 1);
    P.c.pairs++;
    if (dt <= 0 || dt > 1.5 * T) { P.c.gaps++; if (P.guard && needGuard) guardPush(P.guard, cur->pb); emitOrig(cur); return YES; }
    // 보호 장치 상태는 짝 순서대로 갱신한다(정지 화소 횟수는 앞뒤 프레임 차이로 센다)
    double tg = nowMs();
    double sadV = 0, histV = 0; BOOL isCut = NO;
    if (needGuard && P.guard) { guardPush(P.guard, cur->pb); isCut = gOpt.useCut && guardIsCut(P.guard, &sadV, &histV); dpush(&P.guardMs, nowMs() - tg); }
    if (isCut) { P.c.cuts++; logEvent(@"cut", @{ @"sad": @(sadV), @"hist": @(histV) }); emitOrig(cur); return YES; }
    if (needGuard) { P.cutSadMaxNormal = MAX(P.cutSadMaxNormal, sadV); P.cutHistMaxNormal = MAX(P.cutHistMaxNormal, histV); }
    if (gOpt.interp && !atomic_load(&gInterpOn)) { P.c.runtimeOff++; P.lastShown = -1; emitOrig(cur); return YES; }   // 창 모드 i 키: 집계에서 뺀다
    BOOL warm = now < P.warmUntilMs;
    BOOL counted = !warm && !stalled;
    if (warm) P.c.warmPairs++; else if (stalled) P.c.stalledPairs++; else { P.c.eligible++; P.countedMs += T; }
    if (!gOpt.interp) { countSkip(&P.c.passOff, counted); emitOrig(cur); return YES; }
    if (vtBusy) { countSkip(&P.c.lateStart, counted); emitOrig(cur); return YES; }
    VFSession *s = P.session;
    int st = s ? s->state : 2;
    if (st == 0) { countSkip(&P.c.sessionWait, counted); emitOrig(cur); return YES; }
    if (st == 2) { countSkip(&P.c.unsupported, counted); emitOrig(cur); return YES; }
    BOOL probe = NO;
    if (gOpt.useBreaker && breakerIsOpen(&P.br, now, T)) {
        if (P.br.v2 || now < P.br.nextProbeMs || !canStartInTime) { countSkip(&P.c.bypass, counted); emitOrig(cur); return YES; }
        probe = YES;   // 원본은 아래에서 바로 내보내고, 결과는 버린다
    }
    if (!probe && !canStartInTime) { countSkip(&P.c.lateStart, counted); emitOrig(cur); return YES; }
    CVPixelBufferRef dst = NULL;
    CVReturn cr = CVPixelBufferPoolCreatePixelBufferWithAuxAttributes(NULL, s->pool, (__bridge CFDictionaryRef)s->auxAttrs, &dst);
    if (cr != kCVReturnSuccess || !dst) { countSkip(probe ? &P.c.bypass : &P.c.poolExhausted, counted); emitOrig(cur); return YES; }
    // VT 시간선: 세대마다 앞 세대보다 1초 뒤에서 시작하도록 옮겨 단조 증가를 지킨다(실제 간격은 유지)
    if (s->offsetGen != P.gen) { s->vtOffset = s->lastVt + 1.0 - sec(P.prev->pts); s->offsetGen = P.gen; }
    double vp = sec(P.prev->pts) + s->vtOffset, vc = sec(cur->pts) + s->vtOffset, vm = sec(mid) + s->vtOffset;
    s->lastVt = vc;
    CVPixelBufferRef inA = P.prev->pb, inB = cur->pb, cpA = NULL, cpB = NULL;
    if (gOpt.vtInput == 0 && P.frameHasExtended && !s->copyMode && s->srcPool && s->xfer) { s->copyMode = YES; logEvent(@"vt_copy_mode", @{ @"size": [NSString stringWithFormat:@"%dx%d", s->W, s->H], @"why": @"extended_pixels" }); }
    if (s->copyMode) {
        CVPixelBufferPoolCreatePixelBuffer(NULL, s->srcPool, &cpA); CVPixelBufferPoolCreatePixelBuffer(NULL, s->srcPool, &cpB);
        if (!cpA || !cpB || VTPixelTransferSessionTransferImage(s->xfer, P.prev->pb, cpA) != noErr || VTPixelTransferSessionTransferImage(s->xfer, cur->pb, cpB) != noErr) {
            if (cpA) CVPixelBufferRelease(cpA); if (cpB) CVPixelBufferRelease(cpB); CVPixelBufferRelease(dst);
            countSkip(probe ? &P.c.bypass : &P.c.poolExhausted, counted); emitOrig(cur); return YES;
        }
        inA = cpA; inB = cpB;
    }
    VTFrameProcessorFrame *fa = [[VTFrameProcessorFrame alloc] initWithBuffer:inA presentationTimeStamp:tsec(vp)];
    VTFrameProcessorFrame *fb = [[VTFrameProcessorFrame alloc] initWithBuffer:inB presentationTimeStamp:tsec(vc)];
    VTFrameProcessorFrame *fd = [[VTFrameProcessorFrame alloc] initWithBuffer:dst presentationTimeStamp:tsec(vm)];
    VTLowLatencyFrameInterpolationParameters *prm = (fa && fb && fd) ? [[VTLowLatencyFrameInterpolationParameters alloc] initWithSourceFrame:fb previousFrame:fa interpolationPhase:@[@0.5] destinationFrames:@[fd]] : nil;
    if (!prm) { CVPixelBufferRelease(dst); if (cpA) CVPixelBufferRelease(cpA); if (cpB) CVPixelBufferRelease(cpB); P.c.errors++; countSkip(&P.c.countedErrors, counted); emitOrig(cur); logEvent(@"vt_param_fail", @{}); return YES; }
    VFPair *pr = [VFPair new];
    pr->prev = P.prev; pr->cur = cur; pr->dst = dst; pr->srcA = cpA; pr->srcB = cpB; pr->mid = mid; pr->gen = P.gen; pr->submitMs = nowMs();
    pr->warm = warm; pr->stalledAtSubmit = stalled; pr->counted = counted; pr->session = s; pr->guardPushes = P.guard ? P.guard->pushes : -1;
    if (probe) {   // 시험 요청: 집계는 bypass, 원본은 지금 내보낸다(처리 중 다음 프레임을 막지 않게 abandoned 로 둔다)
        pr->probe = YES; pr->abandoned = YES; pr->resolved = YES; pr->counted = NO;
        countSkip(&P.c.bypass, counted); P.br.probes++; P.br.nextProbeMs = now + 1000;   // 완료 전까지 다음 시험 안 함
        emitOrig(cur);
    } else if (isfinite(deadline)) dpush(&P.submitLeadMs, deadline - pr->submitMs);
    P.inflight = pr;
    static uint64_t injectSeed = 0x9E3779B97F4A7C15ull;
    double injectMs = 0;
    if (gOpt.injectPct > 0) {   // 결정적 의사 난수(xorshift)
        injectSeed ^= injectSeed << 13; injectSeed ^= injectSeed >> 7; injectSeed ^= injectSeed << 17;
        if ((injectSeed % 10000) < gOpt.injectPct * 100) injectMs = (double)((injectSeed >> 16) % 1000) / 1000.0 * gOpt.injectMs;
    }
    [s->proc processWithParameters:prm completionHandler:^(id<VTFrameProcessorParameters> pp, NSError *e) {
        if (injectMs > 0) usleep((useconds_t)(injectMs * 1000));
        double done = nowMs();
        dispatch_async(gPipe, ^{ onComplete(pr, e, done); });
    }];
    return YES;
}

static void pipeAdvance(void) {
    while (pipeStep()) {}
}

// 마감 감시: 처리 중인 짝이 마감을 넘기면 원본을 먼저 내보낸다(완료되면 버린다).
static void pipeTick(void) {
    VFPair *pr = P.inflight;
    if (pr && !pr->abandoned && pr->gen == P.gen) {
        double dl = hostMsOfItemTime(pr->mid) - gOpt.leadMs;
        if (isfinite(dl) && nowMs() > dl) {
            pr->abandoned = YES; P.c.abandoned++;
            if (pr->counted && !pr->resolved) { pr->resolved = YES; P.c.late++; markShown(0); }
            emitOrig(pr->cur);
        }
    }
    pipeAdvance();
    sinkReleaseDue();
    sinkSyncToItem(NO);
}

// ───────────────────────────── 프레임 받기 스레드(실시간 정책) ─────────────────────────────
// 다음 프레임만 정확히 받으려고 hasNewPixelBufferForItemTime 을 이분 탐색한다.
// copyPixelBufferForItemTime 은 질의 시각 이하의 가장 늦은 프레임을 주고 그 앞 프레임은 버리므로,
// 질의 시각을 크게 잡으면 프레임을 말없이 잃는다.

static void *pullerMain(void *arg) {
    (void)arg;
    pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
    thread_time_constraint_policy_data_t pol = { (uint32_t)msToAbs(2.0), (uint32_t)msToAbs(0.5), (uint32_t)msToAbs(1.5), 1 };
    kern_return_t kr = thread_policy_set(pthread_mach_thread_np(pthread_self()), THREAD_TIME_CONSTRAINT_POLICY, (thread_policy_t)&pol, THREAD_TIME_CONSTRAINT_POLICY_COUNT);
    printf("  · 프레임 받기 스레드 실시간 정책: %s\n", kr == KERN_SUCCESS ? "적용" : "실패");
    double lastPTS = NAN; AVPlayerItemVideoOutput *lastOut = nil;
    uint64_t next = mach_absolute_time(); uint64_t step = msToAbs(2.0);
    while (!atomic_load(&gStop)) {
        next += step; uint64_t nowA = mach_absolute_time();
        if (next < nowA) next = nowA + step / 4;
        mach_wait_until(next);
        @autoreleasepool {
            os_unfair_lock_lock(&gLock);
            AVPlayerItemVideoOutput *out = gOutput;
            os_unfair_lock_unlock(&gLock);
            if (out != lastOut) { lastOut = out; lastPTS = NAN; }
            if (atomic_exchange(&gPullerReset, 0)) lastPTS = NAN;
            if (out) {
                for (int k = 0; k < 4; k++) {
                    if (atomic_load(&gFifoCount) >= gOpt.maxAhead) break;
                    double hostNow = CACurrentMediaTime();
                    CMTime itemNow = [out itemTimeForHostTime:hostNow];
                    if (!CMTIME_IS_NUMERIC(itemNow)) break;
                    double inow = sec(itemNow), hi = inow + gOpt.lookaheadMs / 1000.0;
                    double lo = isnan(lastPTS) ? inow - 0.05 : lastPTS;
                    if (hi <= lo) break;
                    if (![out hasNewPixelBufferForItemTime:tsec(hi)]) break;
                    double a = lo, b = hi;
                    if (isnan(lastPTS) && [out hasNewPixelBufferForItemTime:tsec(lo)]) b = lo;
                    else while (b - a > 0.0004) { double m = 0.5 * (a + b); if ([out hasNewPixelBufferForItemTime:tsec(m)]) b = m; else a = m; }
                    CMTime disp = kCMTimeInvalid;
                    CVPixelBufferRef pb = [out copyPixelBufferForItemTime:tsec(b) itemTimeForDisplay:&disp];
                    if (!pb) break;
                    if (!CMTIME_IS_NUMERIC(disp)) { CVPixelBufferRelease(pb); break; }
                    lastPTS = sec(disp);
                    VFFrame *f = [VFFrame new];
                    f->pb = pb; f->pts = disp; f->acqMs = hostNow * 1000.0; f->lookMs = (sec(disp) - inow) * 1000.0;
                    f->w = (int)CVPixelBufferGetWidth(pb); f->h = (int)CVPixelBufferGetHeight(pb);
                    atomic_fetch_add(&gFifoCount, 1);
                    dispatch_async(gPipe, ^{ onFrame(f); });
                }
            }
            dispatch_async(gPipe, ^{ pipeTick(); });
        }
    }
    return NULL;
}

// ───────────────────────────── 치지직 공개 방송 주소 ─────────────────────────────

static NSString *const kUA = @"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

// 응답의 형식을 단계마다 확인한다(null·배열이 섞여 와도 예외 없이 '재생 불가'로 처리).
static NSDictionary *asDict(id v) { return [v isKindOfClass:[NSDictionary class]] ? v : nil; }
static NSArray *asArr(id v) { return [v isKindOfClass:[NSArray class]] ? v : nil; }
static NSString *asStr(id v) { return [v isKindOfClass:[NSString class]] ? v : nil; }
static BOOL asTrue(id v) { return [v isKindOfClass:[NSNumber class]] && [v boolValue]; }

static id fetchJSON(NSString *urlString) {
    NSURL *u = [NSURL URLWithString:urlString]; if (!u) return nil;
    NSMutableURLRequest *rq = [NSMutableURLRequest requestWithURL:u];
    [rq setValue:kUA forHTTPHeaderField:@"User-Agent"];
    [rq setValue:@"application/json" forHTTPHeaderField:@"Accept"];
    rq.timeoutInterval = 10;
    __block NSData *body = nil; dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    [[[NSURLSession sharedSession] dataTaskWithRequest:rq completionHandler:^(NSData *d, NSURLResponse *r, NSError *e) {
        if (!e && [r isKindOfClass:[NSHTTPURLResponse class]] && [(NSHTTPURLResponse *)r statusCode] == 200) body = d;
        dispatch_semaphore_signal(sem);
    }] resume];
    dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 12 * NSEC_PER_SEC));
    return body ? [NSJSONSerialization JSONObjectWithData:body options:0 error:nil] : nil;
}

static NSString *gChzzkChannel;   // 고른 채널 id. 메모리에만 두고 출력·기록하지 않는다.

// 성공하면 LLHLS 주소. 토큰이 들어 있으므로 출력·기록하지 않는다. ended: 방송이 끝났음.
static NSURL *chzzkPlaybackURL(NSString *channelId, NSString **whyOut, BOOL *ended) {
    if (ended) *ended = NO;
    NSDictionary *c = asDict(asDict(fetchJSON([NSString stringWithFormat:@"https://api.chzzk.naver.com/service/v3/channels/%@/live-detail", channelId]))[@"content"]);
    if (!c) { *whyOut = @"live-detail 응답 없음"; return nil; }
    if (![asStr(c[@"status"]) isEqual:@"OPEN"]) { *whyOut = @"방송 중 아님"; if (ended) *ended = YES; return nil; }
    if (asTrue(c[@"adult"])) { *whyOut = @"성인 방송 제외"; return nil; }
    id pj = c[@"livePlaybackJson"]; NSDictionary *pb = asDict(pj);
    if (asStr(pj)) pb = asDict([NSJSONSerialization JSONObjectWithData:[asStr(pj) dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil]);
    if (!pb) { *whyOut = @"재생 정보 없음"; return nil; }
    NSString *auth = asStr(asDict(pb[@"meta"])[@"playbackAuthType"]);
    if (auth && ![auth isEqual:@"NONE"]) { *whyOut = @"재생 인증 필요 방송 제외"; return nil; }
    for (id mo in asArr(pb[@"media"])) {
        NSDictionary *m = asDict(mo);
        if (![asStr(m[@"mediaId"]) isEqual:@"LLHLS"]) continue;
        id aes = m[@"aes"];
        if (aes && aes != [NSNull null]) { *whyOut = @"암호화 방송 제외"; return nil; }
        NSString *path = asStr(m[@"path"]); NSURL *u = path ? [NSURL URLWithString:path] : nil;
        if (u) return u;
    }
    *whyOut = @"LLHLS 주소 없음"; return nil;
}

static NSURL *resolveChzzk(NSString *arg, NSString **desc) {
    NSString *why = nil;
    if (![arg isEqual:@"popular"]) {
        NSURL *u = chzzkPlaybackURL(arg, &why, NULL);
        if (!u) fprintf(stderr, "치지직 방송을 열 수 없습니다: %s\n", why.UTF8String);
        gChzzkChannel = arg; *desc = @"치지직 지정 채널"; return u;
    }
    NSArray *data = asArr(asDict(asDict(fetchJSON(@"https://api.chzzk.naver.com/service/v1/lives?size=20&sortType=POPULAR"))[@"content"])[@"data"]);
    int rank = 0;
    for (id io in data) {
        rank++;
        NSDictionary *it = asDict(io);
        if (!it || asTrue(it[@"adult"])) continue;
        NSString *cid = asStr(asDict(it[@"channel"])[@"channelId"]);
        if (!cid) continue;
        NSURL *u = chzzkPlaybackURL(cid, &why, NULL);
        if (u) { gChzzkChannel = cid; *desc = [NSString stringWithFormat:@"치지직 인기 방송 %d위(성인 아님)", rank]; return u; }
    }
    fprintf(stderr, "치지직 인기 방송 중 열 수 있는 방송이 없습니다\n");
    return nil;
}

// ───────────────────────────── 시험 영상 만들기 ─────────────────────────────

static void drawText(CGContextRef cg, NSString *s, CGFloat x, CGFloat y, CGFloat size, CGColorRef color) {
    CTFontRef font = CTFontCreateWithName(CFSTR("Apple SD Gothic Neo"), size, NULL);
    NSDictionary *attrs = @{ (id)kCTFontAttributeName: (__bridge id)font, (id)kCTForegroundColorAttributeName: (__bridge id)color };
    CTLineRef line = CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)[[NSAttributedString alloc] initWithString:s attributes:attrs]);
    CGContextSetTextPosition(cg, x, y); CTLineDraw(line, cg);
    CFRelease(line); CFRelease(font);
}

static void renderTestFrame(CVPixelBufferRef bgra, double t, int W, int H) {
    CVPixelBufferLockBaseAddress(bgra, 0);
    CGColorSpaceRef cs = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef cg = CGBitmapContextCreate(CVPixelBufferGetBaseAddress(bgra), W, H, 8, CVPixelBufferGetBytesPerRow(bgra), cs, kCGImageAlphaNoneSkipFirst | kCGBitmapByteOrder32Little);
    int scene = (int)floor(t / 8.0);
    // 배경: 장면마다 색·무늬 크기가 다른 체크 무늬가 초당 360px 흐른다
    CGFloat hue = fmod(scene * 0.37, 1.0);
    NSColor *c1 = [NSColor colorWithHue:hue saturation:0.55 brightness:0.85 alpha:1], *c2 = [NSColor colorWithHue:fmod(hue + 0.5, 1.0) saturation:0.45 brightness:0.35 alpha:1];
    int cell = 40 + (scene % 3) * 24; double shift = fmod(t * 360.0, 2.0 * cell);
    for (int yi = 0; yi * cell < H; yi++) for (int xi = -2; xi * cell < W; xi++) {
        CGContextSetFillColorWithColor(cg, ((xi + yi) & 1) ? c2.CGColor : c1.CGColor);
        CGContextFillRect(cg, CGRectMake(xi * cell + shift, yi * cell, cell, cell));
    }
    // 빠른 공(초당 1800px 왕복)
    double span = W - 120, p = fmod(t * 1800.0, 2 * span); double bx = p < span ? p : 2 * span - p;
    CGContextSetFillColorWithColor(cg, NSColor.whiteColor.CGColor);
    CGContextFillEllipseInRect(cg, CGRectMake(60 + bx - 40, H * 0.55, 80, 80));
    // 고정 자막
    CGContextSetFillColorWithColor(cg, [NSColor colorWithWhite:0 alpha:0.6].CGColor);
    CGContextFillRect(cg, CGRectMake(0, 0, W, H * 0.09));
    drawText(cg, @"고정 자막 — CView 프레임 보간 시험 영상", W * 0.05, H * 0.03, H * 0.045, NSColor.whiteColor.CGColor);
    // 프레임마다 바뀌는 타이머
    drawText(cg, [NSString stringWithFormat:@"%02d:%06.3f", (int)(t / 60), fmod(t, 60.0)], W * 0.72, H * 0.88, H * 0.06, NSColor.yellowColor.CGColor);
    CGContextRelease(cg); CGColorSpaceRelease(cs);
    CVPixelBufferUnlockBaseAddress(bgra, 0);
}

static int generateFile(NSString *path) {
    if ([[NSFileManager defaultManager] fileExistsAtPath:path]) { fprintf(stderr, "이미 있는 파일은 덮어쓰지 않습니다: %s\n", path.UTF8String); return 2; }
    int W = gOpt.genW, H = gOpt.genH, fps = gOpt.genFps; long n = lround(gOpt.seconds * fps);
    if (W <= 0 || H <= 0 || (W & 1) || (H & 1) || fps <= 0 || fps > 240 || n <= 0) { fprintf(stderr, "크기·프레임률이 잘못됐습니다\n"); return 64; }
    // 끝까지 쓴 뒤에만 최종 이름으로 바꾼다(중간에 끊기면 반쯤 쓴 파일이 남아 다음 실행이 그것을 쓰지 않게).
    NSString *partial = [path stringByAppendingString:@".partial.mp4"];
    [[NSFileManager defaultManager] removeItemAtPath:partial error:nil];
    NSError *err = nil;
    AVAssetWriter *w = [AVAssetWriter assetWriterWithURL:[NSURL fileURLWithPath:partial] fileType:AVFileTypeMPEG4 error:&err];
    if (!w) { fprintf(stderr, "AVAssetWriter 실패: %s\n", err.description.UTF8String); return 2; }
    NSDictionary *vs = @{ AVVideoCodecKey: AVVideoCodecTypeH264, AVVideoWidthKey: @(W), AVVideoHeightKey: @(H),
                          AVVideoColorPropertiesKey: @{ AVVideoColorPrimariesKey: AVVideoColorPrimaries_ITU_R_709_2, AVVideoTransferFunctionKey: AVVideoTransferFunction_ITU_R_709_2, AVVideoYCbCrMatrixKey: AVVideoYCbCrMatrix_ITU_R_709_2 },
                          AVVideoCompressionPropertiesKey: @{ AVVideoAverageBitRateKey: @(W * H * fps / 10), AVVideoProfileLevelKey: AVVideoProfileLevelH264HighAutoLevel, AVVideoAllowFrameReorderingKey: @YES, AVVideoMaxKeyFrameIntervalKey: @(fps * 2) } };
    AVAssetWriterInput *in = [AVAssetWriterInput assetWriterInputWithMediaType:AVMediaTypeVideo outputSettings:vs];
    in.expectsMediaDataInRealTime = NO;
    AVAssetWriterInputPixelBufferAdaptor *ad = [AVAssetWriterInputPixelBufferAdaptor assetWriterInputPixelBufferAdaptorWithAssetWriterInput:in sourcePixelBufferAttributes:@{ (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange), (id)kCVPixelBufferWidthKey: @(W), (id)kCVPixelBufferHeightKey: @(H), (id)kCVPixelBufferIOSurfacePropertiesKey: @{} }];
    [w addInput:in];
    if (![w startWriting]) { fprintf(stderr, "쓰기 시작 실패: %s\n", w.error.description.UTF8String); return 2; }
    [w startSessionAtSourceTime:kCMTimeZero];
    CVPixelBufferRef bgra = NULL;
    CVPixelBufferCreate(NULL, W, H, kCVPixelFormatType_32BGRA, (__bridge CFDictionaryRef)@{ (id)kCVPixelBufferIOSurfacePropertiesKey: @{} }, &bgra);
    VTPixelTransferSessionRef xfer = NULL; VTPixelTransferSessionCreate(NULL, &xfer);
    VTSessionSetProperty(xfer, kVTPixelTransferPropertyKey_DestinationYCbCrMatrix, kCVImageBufferYCbCrMatrix_ITU_R_709_2);
    for (long i = 0; i < n; i++) {
        @autoreleasepool {
            while (!in.readyForMoreMediaData && !atomic_load(&gStop)) usleep(1000);
            if (atomic_load(&gStop)) { [w cancelWriting]; [[NSFileManager defaultManager] removeItemAtPath:partial error:nil]; fprintf(stderr, "중단: 시험 영상을 만들지 않았습니다\n"); return 130; }
            renderTestFrame(bgra, (double)i / fps, W, H);
            CVPixelBufferRef yuv = NULL; CVPixelBufferPoolCreatePixelBuffer(NULL, ad.pixelBufferPool, &yuv);
            if (!yuv) { fprintf(stderr, "버퍼 풀 실패\n"); [w cancelWriting]; [[NSFileManager defaultManager] removeItemAtPath:partial error:nil]; return 2; }
            VTPixelTransferSessionTransferImage(xfer, bgra, yuv);
            CVBufferSetAttachment(yuv, kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_709_2, kCVAttachmentMode_ShouldPropagate);
            CVBufferSetAttachment(yuv, kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_709_2, kCVAttachmentMode_ShouldPropagate);
            CVBufferSetAttachment(yuv, kCVImageBufferTransferFunctionKey, kCVImageBufferTransferFunction_ITU_R_709_2, kCVAttachmentMode_ShouldPropagate);
            if (![ad appendPixelBuffer:yuv withPresentationTime:CMTimeMake(i, fps)]) { fprintf(stderr, "프레임 쓰기 실패: %s\n", w.error.description.UTF8String); CVPixelBufferRelease(yuv); [w cancelWriting]; [[NSFileManager defaultManager] removeItemAtPath:partial error:nil]; return 2; }
            CVPixelBufferRelease(yuv);
            if (i % (fps * 10) == 0) { printf("  만드는 중 %ld/%ld\n", i, n); fflush(stdout); }
        }
    }
    [in markAsFinished];
    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    [w finishWritingWithCompletionHandler:^{ dispatch_semaphore_signal(sem); }];
    dispatch_semaphore_wait(sem, DISPATCH_TIME_FOREVER);
    VTPixelTransferSessionInvalidate(xfer); CFRelease(xfer); CVPixelBufferRelease(bgra);
    if (w.status != AVAssetWriterStatusCompleted) { fprintf(stderr, "마무리 실패: %s\n", w.error.description.UTF8String); [[NSFileManager defaultManager] removeItemAtPath:partial error:nil]; return 2; }
    if (![[NSFileManager defaultManager] moveItemAtPath:partial toPath:path error:&err]) { fprintf(stderr, "이름 바꾸기 실패: %s\n", err.description.UTF8String); return 2; }
    printf("시험 영상: %s (%dx%d %dfps %.0f초, 8초마다 장면 전환)\n", path.UTF8String, W, H, fps, gOpt.seconds);
    return 0;
}

static int probeSizes(void) {
    int sizes[][2] = { {640, 360}, {852, 480}, {854, 480}, {768, 432}, {960, 540}, {1280, 720}, {1440, 810}, {1920, 1080} };
    printf("VT 저지연 보간 크기 확인(세션 시작 + 처리 3회)\n");
    for (unsigned i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
        VFSession *s = [VFSession new]; s->W = sizes[i][0]; s->H = sizes[i][1];
        sessionStart(s);
        printf("  %4dx%-4d %s\n", s->W, s->H, s->state == 1 ? [NSString stringWithFormat:@"됨(세션 시작 %.0fms, 처리 3회 %.0fms)", s->startMs, s->warmMs].UTF8String : s->failure.UTF8String);
        if (s->proc) [s->proc endSession];
    }
    return 0;
}

// ───────────────────────────── 재생기(메인 스레드) ─────────────────────────────

@interface VFView : NSView
@end
@implementation VFView
- (BOOL)acceptsFirstResponder { return YES; }
- (void)keyDown:(NSEvent *)e {
    NSString *k = e.charactersIgnoringModifiers.lowercaseString;
    if ([k isEqual:@"i"]) { BOOL v = !atomic_load(&gInterpOn); atomic_store(&gInterpOn, v); self.window.title = v ? @"vfi-player — 보간 켬 (i: 끄기, q: 끝)" : @"vfi-player — 보간 끔 (i: 켜기, q: 끝)"; }
    else if ([k isEqual:@"q"]) atomic_store(&gStop, YES);
}
@end

static double instrumentMB(void) {   // 측정 기록(표본 배열·이벤트)이 쓰는 메모리. C8 메모리 증가에서 뺀다.
    __block double bytes = 0;
    dispatch_sync(gPipe, ^{
        DArr *arrs[] = { &P.procMs, &P.procAtMs, &P.compMs, &P.guardMs, &P.lookMs, &P.slackMs, &P.submitLeadMs, &P.probeMs };
        for (unsigned i = 0; i < sizeof arrs / sizeof arrs[0]; i++) bytes += arrs[i]->n * sizeof(double);
        bytes += P.events.count * 400.0;
    });
    return bytes / 1048576.0;
}

@interface VFPlayer : NSObject <AVPlayerItemOutputPullDelegate>
@property (strong) AVPlayer *player;
@property (strong) AVPlayerItem *item;
@property (strong) AVPlayerItemVideoOutput *output;
@property (strong) NSURL *url;
@property (strong) NSWindow *window;
@property (strong) NSFileHandle *csv;
@property BOOL csvFailed;
@property double startMs, lastStallMs, fwdAboveSince, fwdBelowSince, notPlayingSince, nextReconnectAllowedMs, healthySinceMs, loopEndMs;
@property long stalls, reconnects, reconnectBudgetUsed, timeJumps, flushes, errorLogEntries, lastErrorLogCount;
@property BOOL catchingUp, reconnecting, truncated;
@property (strong) NSString *truncatedWhy;
@property int seekParity;
@property double lastSeekAt, lastRateToggleAt, lastProgressMs, lastCpu, lastCpuWall;
@property long progLastFrames, progLastOnTime, progLastElig, progLastLate, progLastCuts, progLastBypass, progLastProc;
@property (strong) NSMutableArray *switchPlan;
@property (strong) NSMutableArray<NSArray<NSNumber *> *> *latencies;   // [재생 시작 뒤 초, 지연 초]
@property double memAt60, instrAt60;
@end

static NSArray *kCsvCols(void) {
    return @[@"t", @"w", @"h", @"fps", @"gen", @"frames", @"eligible", @"on_time", @"late", @"cuts", @"bypass", @"trips", @"proc_n", @"latency_s", @"rate", @"fwd_s", @"cpu_pct", @"mem_mb", @"thermal", @"look_min_ms", @"playing"];
}

@implementation VFPlayer

- (void)buildItem {
    AVURLAsset *asset = [AVURLAsset URLAssetWithURL:self.url options:nil];
    AVPlayerItem *item = [AVPlayerItem playerItemWithAsset:asset];
    NSDictionary *pa = @{ (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange), (id)kCVPixelBufferIOSurfacePropertiesKey: @{} };
    AVPlayerItemVideoOutput *out = [[AVPlayerItemVideoOutput alloc] initWithPixelBufferAttributes:pa];
    out.suppressesPlayerRendering = YES;
    [out setDelegate:self queue:dispatch_get_main_queue()];
    [item addOutput:out];
    if (gOpt.maxHeight > 0) item.preferredMaximumResolution = CGSizeMake(ceil(gOpt.maxHeight * 16.0 / 9.0), gOpt.maxHeight);
    if (gIsLive) [self applyLiveSettings:item];
    else if (gOpt.forwardBuffer > 0) item.preferredForwardBufferDuration = gOpt.forwardBuffer;
    NSNotificationCenter *nc = NSNotificationCenter.defaultCenter;
    if (self.item) [nc removeObserver:self name:nil object:self.item];
    [nc addObserver:self selector:@selector(timeJumped:) name:AVPlayerItemTimeJumpedNotification object:item];
    [nc addObserver:self selector:@selector(stalled:) name:AVPlayerItemPlaybackStalledNotification object:item];
    [nc addObserver:self selector:@selector(errorLogged:) name:AVPlayerItemNewErrorLogEntryNotification object:item];
    [nc addObserver:self selector:@selector(failedToEnd:) name:AVPlayerItemFailedToPlayToEndTimeNotification object:item];
    self.item = item; self.output = out; self.lastErrorLogCount = 0;
    if (!self.player) {
        self.player = [AVPlayer playerWithPlayerItem:item];
        self.player.muted = !gOpt.audio;
        self.player.preventsDisplaySleepDuringVideoPlayback = YES;
    } else [self.player replaceCurrentItemWithPlayerItem:item];
    if (gIsLive) self.player.automaticallyWaitsToMinimizeStalling = gOpt.latency == LatAVPlayer;   // avplayer: AVPlayer 기본 동작 그대로
}

- (void)applyLiveSettings:(AVPlayerItem *)item {
    item.configuredTimeOffsetFromLive = CMTimeMakeWithSeconds(gOpt.liveOffset, 600);
    item.automaticallyPreservesTimeOffsetFromLive = gOpt.latency == LatAVPlayer;
    item.preferredForwardBufferDuration = 0;
    // 기본값 NO 면 라이브가 멈춘(속도 0) 동안 받기도 멈춰, 버퍼가 다시 차지 않아 스스로 회복하지 못했다.
    item.canUseNetworkResourcesForLiveStreamingWhilePaused = YES;
}

- (void)attachOutputWhenReady {
    os_unfair_lock_lock(&gLock);
    gOutput = self.output;
    if (gItemTB) CFRelease(gItemTB);
    gItemTB = self.item.timebase ? (CMTimebaseRef)CFRetain(self.item.timebase) : NULL;
    os_unfair_lock_unlock(&gLock);
}

- (void)outputSequenceWasFlushed:(AVPlayerItemOutput *)output {
    self.flushes++;
    dispatch_async(gPipe, ^{ bumpGeneration(@"output_flushed", YES); });
}
- (void)timeJumped:(NSNotification *)n {
    dispatch_async(dispatch_get_main_queue(), ^{ self.timeJumps++; });
    dispatch_async(gPipe, ^{ bumpGeneration(@"time_jumped", YES); });
}
- (void)stalled:(NSNotification *)n {
    dispatch_async(dispatch_get_main_queue(), ^{ self.stalls++; self.lastStallMs = nowMs(); });
    dispatch_async(gPipe, ^{ logEvent(@"stall", @{}); });
}
- (void)failedToEnd:(NSNotification *)n {
    NSError *e = n.userInfo[AVPlayerItemFailedToPlayToEndTimeErrorKey];
    NSDictionary *info = @{ @"code": @(e.code), @"domain": e.domain ?: @"", @"msg": e.localizedDescription ?: @"" };
    dispatch_async(gPipe, ^{ logEvent(@"failed_to_play_to_end", info); });
}
// AVPlayer 오류 기록. 주소에는 토큰이 들어 있으므로 호스트만 남긴다.
- (void)errorLogged:(NSNotification *)n {
    dispatch_async(dispatch_get_main_queue(), ^{
        AVPlayerItem *it = n.object; if (it != self.item) return;
        void (^handle)(AVPlayerItemErrorLog *) = ^(AVPlayerItemErrorLog *log) {
            NSArray<AVPlayerItemErrorLogEvent *> *ev = log.events;
            for (NSUInteger i = self.lastErrorLogCount; i < ev.count; i++) {
                AVPlayerItemErrorLogEvent *x = ev[i];
                NSString *host = x.URI ? ([NSURL URLWithString:x.URI].host ?: @"?") : @"";
                NSDictionary *info = @{ @"status": @(x.errorStatusCode), @"domain": x.errorDomain ?: @"", @"comment": x.errorComment ?: @"", @"host": host };
                self.errorLogEntries++;
                if (self.errorLogEntries <= 30) printf("  · AVPlayer 오류 기록: %ld %s %s (%s)\n", (long)x.errorStatusCode, (x.errorDomain ?: @"").UTF8String, (x.errorComment ?: @"").UTF8String, host.UTF8String);
                dispatch_async(gPipe, ^{ logEvent(@"error_log", info); });
            }
            self.lastErrorLogCount = MAX(self.lastErrorLogCount, ev.count);
        };
        if (@available(macOS 27.0, *)) [it fetchErrorLogWithCompletionHandler:^(AVPlayerItemErrorLog *log) { dispatch_async(dispatch_get_main_queue(), ^{ if (it == self.item) handle(log); }); }];
        else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            handle(it.errorLog);
#pragma clang diagnostic pop
        }
    });
}

- (double)forwardBuffer {
    CMTime ct = self.item.currentTime;
    for (NSValue *v in self.item.loadedTimeRanges) {
        CMTimeRange r = v.CMTimeRangeValue;
        if (CMTimeRangeContainsTime(r, ct) || CMTimeCompare(CMTimeRangeGetEnd(r), ct) == 0) return sec(CMTimeRangeGetEnd(r)) - sec(ct);
    }
    return 0;
}

- (void)stopRun:(NSString *)why {
    self.truncated = YES; self.truncatedWhy = why;
    printf("  · 측정을 일찍 끝냅니다: %s\n", why.UTF8String);
    atomic_store(&gStop, YES);
}

// 재연결: 같은 채널만 다시 연다(치지직 인기 방송이면 처음 고른 채널). API 호출은 메인 스레드 밖에서 한다.
- (void)reconnect:(NSString *)why {
    double now = nowMs();
    if (self.reconnecting || now < self.nextReconnectAllowedMs) return;
    if (self.reconnectBudgetUsed >= 5) { [self stopRun:@"재연결 한도(5회 연속)"]; return; }
    self.reconnecting = YES; self.reconnects++; self.reconnectBudgetUsed++;
    self.nextReconnectAllowedMs = now + 1000.0 * (1 << MIN(self.reconnectBudgetUsed, 4));   // 2·4·8·16초
    printf("  · 재연결 %ld (%s)\n", self.reconnects, why.UTF8String);
    os_unfair_lock_lock(&gLock); gOutput = nil; if (gItemTB) CFRelease(gItemTB); gItemTB = NULL; os_unfair_lock_unlock(&gLock);
    dispatch_async(gPipe, ^{ bumpGeneration([@"reconnect " stringByAppendingString:why], YES); });
    void (^rebuild)(NSURL *) = ^(NSURL *u) {
        if (u) self.url = u;
        [self buildItem];
        [self.player play];
        self.reconnecting = NO; self.notPlayingSince = 0; self.lastStallMs = 0;
    };
    if (gOpt.chzzk && gChzzkChannel) {
        NSString *cid = gChzzkChannel;
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
            NSString *why2 = nil; BOOL ended = NO;
            NSURL *u = chzzkPlaybackURL(cid, &why2, &ended);
            dispatch_async(dispatch_get_main_queue(), ^{
                if (!u) {
                    self.reconnecting = NO;
                    dispatch_async(gPipe, ^{ logEvent(@"reconnect_resolve_failed", @{ @"why": why2 ?: @"" }); });
                    if (ended) [self stopRun:@"방송이 끝났습니다"];
                    else printf("  · 재연결 주소를 받지 못했습니다(%s). 잠시 뒤 다시 시도합니다\n", (why2 ?: @"").UTF8String);
                    return;
                }
                rebuild(u);
            });
        });
    } else rebuild(nil);
}

- (void)writeCsvRow:(NSDictionary *)row {
    if (!self.csv || self.csvFailed) return;
    NSMutableArray *v = [NSMutableArray new];
    for (NSString *k in kCsvCols()) { id x = row[k]; [v addObject:x == [NSNull null] || !x ? @"" : [NSString stringWithFormat:@"%@", x]]; }
    NSError *e = nil;
    if (![self.csv writeData:[[[v componentsJoinedByString:@","] stringByAppendingString:@"\n"] dataUsingEncoding:NSUTF8StringEncoding] error:&e]) {
        self.csvFailed = YES; printf("  · CSV 쓰기 실패: %s\n", e.localizedDescription.UTF8String);
    }
}

// 1초마다(메인 스레드)
- (void)secondTick {
    double now = nowMs(), t = (now - self.startMs) / 1000.0;
    AVPlayerItem *it = self.item;
    if (!self.reconnecting) {
        if (it.status == AVPlayerItemStatusReadyToPlay) {
            os_unfair_lock_lock(&gLock); BOOL attached = gOutput == self.output && gItemTB; os_unfair_lock_unlock(&gLock);
            if (!attached) [self attachOutputWhenReady];
        } else if (it.status == AVPlayerItemStatusFailed) {
            printf("  · 재생 실패: %s\n", it.error.localizedDescription.UTF8String);
            if (gIsLive) [self reconnect:@"item_failed"]; else { [self stopRun:@"재생 실패"]; return; }
        }
    }
    BOOL playing = self.player.timeControlStatus == AVPlayerTimeControlStatusPlaying;
    double fwd = [self forwardBuffer];
    double lat = NAN; NSDate *cd = it.currentDate; if (cd) lat = -[cd timeIntervalSinceNow];
    if (isfinite(lat) && t > 5) [self.latencies addObject:@[@(t), @(lat)]];
    // 라이브 지연 제어(웹 규칙): 앞 버퍼 > 4초가 3초 이어지면 1.03배, ≤ 2초가 3초 이어지면 1.0배, 끊김 뒤 30초는 따라잡지 않음
    if (gIsLive && gOpt.latency == LatWeb && playing) {
        if (fwd > 4) { if (!self.fwdAboveSince) self.fwdAboveSince = now; } else self.fwdAboveSince = 0;
        if (fwd <= 2) { if (!self.fwdBelowSince) self.fwdBelowSince = now; } else self.fwdBelowSince = 0;
        BOOL cool = self.lastStallMs && now - self.lastStallMs < 30000;
        if (!self.catchingUp && !cool && self.fwdAboveSince && now - self.fwdAboveSince > 3000) { self.catchingUp = YES; self.player.rate = 1.03f; dispatch_async(gPipe, ^{ logEvent(@"rate", @{ @"rate": @1.03 }); }); }
        else if (self.catchingUp && ((self.fwdBelowSince && now - self.fwdBelowSince > 3000) || cool)) { self.catchingUp = NO; self.player.rate = 1.0f; dispatch_async(gPipe, ^{ logEvent(@"rate", @{ @"rate": @1.0 }); }); }
    }
    // 라이브가 재생 중이 아니면: 버퍼가 0.5초 넘게 차면 다시 재생, 8초(avplayer 방식은 30초) 넘게 멈추면 재연결.
    // 끊김 알림 없이 버퍼가 비고 멈추는 경우가 있어(치지직 LL-HLS) 알림이 아니라 상태로 판단한다.
    if (gIsLive && !self.reconnecting && t > 3) {
        if (playing) { self.notPlayingSince = 0; if (!self.healthySinceMs) self.healthySinceMs = now; }
        else {
            self.healthySinceMs = 0;
            if (!self.notPlayingSince) { self.notPlayingSince = now; dispatch_async(gPipe, ^{ logEvent(@"not_playing", @{ @"fwd": @(fwd), @"reason": self.player.reasonForWaitingToPlay ?: @"" }); }); }
            if (gOpt.latency != LatAVPlayer && fwd > 0.5) [self.player playImmediatelyAtRate:self.catchingUp ? 1.03f : 1.0f];
            else if (now - self.notPlayingSince > (gOpt.latency == LatAVPlayer ? 30000 : 8000)) [self reconnect:@"not_playing"];
        }
        if (self.healthySinceMs && now - self.healthySinceMs > 300000) self.reconnectBudgetUsed = 0;   // 5분 정상이면 한도를 되돌린다
    }
    // 시나리오(VOD·파일): 탐색, 화질 상한 바꾸기, 속도 바꾸기
    if (!gIsLive && gOpt.seekEvery > 0 && t - self.lastSeekAt >= gOpt.seekEvery) {
        self.lastSeekAt = t;
        double cur = sec(it.currentTime), dur = sec(it.duration);
        double target = (self.seekParity++ % 2 == 0) ? MAX(0, cur - 10) : cur + 15;
        if (isfinite(dur) && target > dur - 5) target = MAX(0, dur * 0.2);
        [self.player seekToTime:tsec(target) toleranceBefore:kCMTimeZero toleranceAfter:kCMTimeZero];
        dispatch_async(gPipe, ^{ logEvent(@"seek", @{ @"to": @(target) }); });
    }
    for (NSDictionary *sw in [self.switchPlan copy]) {
        if (t >= [sw[@"t"] doubleValue]) {
            int h = [sw[@"h"] intValue];
            it.preferredMaximumResolution = h > 0 ? CGSizeMake(ceil(h * 16.0 / 9.0), h) : CGSizeZero;
            [self.switchPlan removeObject:sw];
            dispatch_async(gPipe, ^{ logEvent(@"switch", @{ @"max_height": @(h) }); });
            printf("  · 화질 상한 %dp\n", h);
        }
    }
    if (!gIsLive && gOpt.rateToggle > 0 && t - self.lastRateToggleAt >= gOpt.rateToggle) {
        self.lastRateToggleAt = t;
        float r = self.player.rate > 1.01f ? 1.0f : 1.03f; self.player.rate = r;
        dispatch_async(gPipe, ^{ logEvent(@"rate", @{ @"rate": @(r) }); });
    }
    double mem = footprintMB();
    if (t >= 60 && !self.memAt60) { self.memAt60 = mem; self.instrAt60 = instrumentMB(); }
    double cpu = cpuMs(), cpuPct = self.lastCpuWall ? (cpu - self.lastCpu) / (now - self.lastCpuWall) * 100.0 : NAN;
    self.lastCpu = cpu; self.lastCpuWall = now;
    __block NSDictionary *row = nil;
    dispatch_sync(gPipe, ^{
        long nProc = P.procMs.n;
        row = @{ @"t": @(lround(t)), @"w": @(P.guard ? P.guard->W : 0), @"h": @(P.guard ? P.guard->H : 0), @"fps": @(P.T > 0 ? 1000.0 / P.T : 0),
                 @"gen": @(P.gen), @"frames": @(P.c.frames), @"eligible": @(P.c.eligible), @"on_time": @(P.c.onTime), @"late": @(P.c.late),
                 @"cuts": @(P.c.cuts), @"bypass": @(P.c.bypass), @"trips": @(P.br.trips), @"proc_n": @(nProc),
                 @"latency_s": isfinite(lat) ? @(lat) : [NSNull null], @"rate": @(self.player.rate), @"fwd_s": @(fwd), @"cpu_pct": isfinite(cpuPct) ? @(cpuPct) : [NSNull null],
                 @"mem_mb": @(mem), @"thermal": @(NSProcessInfo.processInfo.thermalState), @"look_min_ms": @(dmin(&P.lookMs, MAX(0, P.lookMs.n - 120))), @"playing": @(playing) };
    });
    [self writeCsvRow:row];
    if (now - self.lastProgressMs >= 5000) { self.lastProgressMs = now; [self printProgress:row cpu:cpuPct]; }
}

- (void)printProgress:(NSDictionary *)row cpu:(double)cpuPct {
    __block double p50 = NAN, p95 = NAN; __block long dE = 0, dOn = 0, dLate = 0, dCuts = 0, dByp = 0; __block uint64_t gen = 0; __block int w = 0, h = 0; __block double fps = 0, look = NAN; __block long trips = 0;
    dispatch_sync(gPipe, ^{
        long from = self.progLastProc; p50 = dpct(&P.procMs, from, 0.5); p95 = dpct(&P.procMs, from, 0.95); self.progLastProc = P.procMs.n;
        dE = P.c.eligible - self.progLastElig; dOn = P.c.onTime - self.progLastOnTime; dLate = P.c.late - self.progLastLate; dCuts = P.c.cuts - self.progLastCuts; dByp = P.c.bypass - self.progLastBypass;
        self.progLastElig = P.c.eligible; self.progLastOnTime = P.c.onTime; self.progLastLate = P.c.late; self.progLastCuts = P.c.cuts; self.progLastBypass = P.c.bypass;
        gen = P.gen; w = P.guard ? P.guard->W : 0; h = P.guard ? P.guard->H : 0; fps = P.T > 0 ? 1000.0 / P.T : 0; look = dmin(&P.lookMs, MAX(0, P.lookMs.n - 300)); trips = P.br.trips;
    });
    id lat = row[@"latency_s"];
    printf("[%4lds] %dx%d %.0ffps 세대%llu | 보간 제때 %5.1f%% 늦음 %ld 장면전환 %ld 브레이커 우회 %ld(열림 %ld) | 처리 p50 %.1f p95 %.1fms | 앞당김 최소 %.0fms | 지연 %s 속도 %.2f 버퍼 %.1fs%s | CPU %.0f%% 메모리 %.0fMB\n",
           [row[@"t"] longValue], w, h, fps, (unsigned long long)gen, dE ? 100.0 * dOn / dE : 0.0, dLate, dCuts, dByp, trips, p50, p95, look,
           lat == [NSNull null] ? "-" : [NSString stringWithFormat:@"%.2fs", [lat doubleValue]].UTF8String, [row[@"rate"] doubleValue], [row[@"fwd_s"] doubleValue],
           [row[@"playing"] boolValue] ? "" : " (멈춤)", cpuPct, [row[@"mem_mb"] doubleValue]);
    fflush(stdout);
}
@end

// ───────────────────────────── 요약 ─────────────────────────────

static NSString *sysctlString(const char *name) {
    size_t n = 0; if (sysctlbyname(name, NULL, &n, NULL, 0) != 0 || !n) return @"";
    char *b = malloc(n); sysctlbyname(name, b, &n, NULL, 0); NSString *s = @(b); free(b); return s;
}

static NSDictionary *distribution(const DArr *a) {
    if (!a->n) return @{ @"n": @0 };
    return @{ @"n": @(a->n), @"p50": @(dpct(a, 0, 0.5)), @"p95": @(dpct(a, 0, 0.95)), @"p99": @(dpct(a, 0, 0.99)), @"max": @(dpct(a, 0, 1.0)), @"min": @(dmin(a, 0)), @"mean": @(dmean(a, 0)) };
}

static id jsonSafe(id v) {
    if ([v isKindOfClass:[NSDictionary class]]) { NSMutableDictionary *d = [NSMutableDictionary new]; for (id k in v) d[k] = jsonSafe(v[k]); return d; }
    if ([v isKindOfClass:[NSArray class]]) { NSMutableArray *a = [NSMutableArray new]; for (id x in v) [a addObject:jsonSafe(x)]; return a; }
    if ([v isKindOfClass:[NSNumber class]] && !isfinite([v doubleValue])) return [NSNull null];
    return v ?: [NSNull null];
}

static double medianOf(NSArray<NSNumber *> *v) {
    if (!v.count) return NAN;
    NSArray *s = [v sortedArrayUsingSelector:@selector(compare:)]; return [s[s.count / 2] doubleValue];
}

// [초, 값] 표본 중 [from, to) 구간의 중앙값
static double medianInWindow(NSArray<NSArray<NSNumber *> *> *samples, double from, double to, long *nOut) {
    NSMutableArray *v = [NSMutableArray new];
    for (NSArray *x in samples) { double t = [x[0] doubleValue]; if (t >= from && t < to) [v addObject:x[1]]; }
    if (nOut) *nOut = v.count;
    return medianOf(v);
}

// ───────────────────────────── main ─────────────────────────────

static void onSignal(int sig) { (void)sig; atomic_store(&gStop, YES); }

int main(int argc, const char **argv) {
    int exitCode = 0;
    @autoreleasepool {
        setvbuf(stdout, NULL, _IOLBF, 0);
        if (!parseOptions(argc, argv)) return 64;
        if (gOpt.help) { usage(); return 0; }
        signal(SIGINT, onSignal); signal(SIGTERM, onSignal);
        if (gOpt.generate) return generateFile(gOpt.generate);
        if (gOpt.probeSizes) return probeSizes();
        NSURL *url = nil;
        if (gOpt.file) { url = [NSURL fileURLWithPath:gOpt.file]; gSourceDesc = [@"파일 " stringByAppendingString:gOpt.file.lastPathComponent]; }
        else if (gOpt.url) { url = [NSURL URLWithString:gOpt.url]; gSourceDesc = [NSString stringWithFormat:@"URL %@%@", url.host ?: @"", url.path.lastPathComponent.length ? [@"/…/" stringByAppendingString:url.path.lastPathComponent] : @""]; }
        else if (gOpt.chzzk) { NSString *d = nil; url = resolveChzzk(gOpt.chzzk, &d); gSourceDesc = d ?: @"치지직"; gIsLive = YES; }
        if (!url) { usage(); return 64; }

        gPipe = dispatch_queue_create("vfi.pipe", dispatch_queue_attr_make_with_qos_class(DISPATCH_QUEUE_SERIAL, QOS_CLASS_USER_INTERACTIVE, 0));
        P.fifo = [NSMutableArray new]; P.sessions = [NSMutableDictionary new]; P.events = [NSMutableArray new]; P.sessionLog = [NSMutableDictionary new];
        P.lastEmitPTS = kCMTimeInvalid; P.lastShown = -1; P.br.v2 = gOpt.breakerV2; P.br.cooldown = 1.0; P.br.win = 30;
        P.lastHardBumpGen = UINT64_MAX;

        VFPlayer *pl = [VFPlayer new];
        if (gOpt.csvOut) {   // 초 단위 기록은 메모리에 쌓지 않고 바로 쓴다
            if (![[NSFileManager defaultManager] createFileAtPath:gOpt.csvOut contents:[[[kCsvCols() componentsJoinedByString:@","] stringByAppendingString:@"\n"] dataUsingEncoding:NSUTF8StringEncoding] attributes:nil] ||
                !(pl.csv = [NSFileHandle fileHandleForWritingAtPath:gOpt.csvOut])) { fprintf(stderr, "CSV 파일을 만들 수 없습니다: %s\n", gOpt.csvOut.UTF8String); printf("CSV 파일을 만들 수 없습니다\n"); return 4; }
            [pl.csv seekToEndOfFile];
        }

        id activity = [NSProcessInfo.processInfo beginActivityWithOptions:NSActivityUserInitiated | NSActivityLatencyCritical | NSActivityIdleSystemSleepDisabled reason:@"vfi-player 측정"];

        // 출력 준비
        gSink = [VFSink new]; gSink->kind = gOpt.sink;
        if (gOpt.sink == SinkWindow) {
            [NSApplication sharedApplication];
            [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
            [NSApp finishLaunching];
            NSWindow *win = [[NSWindow alloc] initWithContentRect:NSMakeRect(120, 120, 1280, 720) styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable | NSWindowStyleMaskMiniaturizable backing:NSBackingStoreBuffered defer:NO];
            win.releasedWhenClosed = NO;   // ARC 가 보유한다(YES 면 닫을 때 한 번 더 해제돼 끝날 때 죽는다)
            win.title = @"vfi-player — 보간 켬 (i: 끄기, q: 끝)";
            VFView *v = [[VFView alloc] initWithFrame:win.contentView.bounds]; v.wantsLayer = YES;
            AVSampleBufferDisplayLayer *layer = [AVSampleBufferDisplayLayer new];
            layer.videoGravity = AVLayerVideoGravityResizeAspect; layer.backgroundColor = NSColor.blackColor.CGColor;
            layer.frame = v.bounds; layer.autoresizingMask = kCALayerWidthSizable | kCALayerHeightSizable;
            [v.layer addSublayer:layer];
            win.contentView = v; [win makeKeyAndOrderFront:nil]; [win makeFirstResponder:v];
            [NSApp activate];
            [[NSNotificationCenter defaultCenter] addObserverForName:NSWindowWillCloseNotification object:win queue:nil usingBlock:^(NSNotification *n) { atomic_store(&gStop, YES); }];
            gSink->layer = layer; gSink->renderer = layer.sampleBufferRenderer;
            pl.window = win;
        } else if (gOpt.sink == SinkRenderer) {
            // 단독 AVSampleBufferVideoRenderer 는 표시 대상이 없으면 enqueue 에서 예외를 던진다.
            // 창에 붙이지 않은 표시 레이어의 렌더러를 쓴다(화면에는 아무것도 나오지 않는다).
            AVSampleBufferDisplayLayer *layer = [AVSampleBufferDisplayLayer new];
            layer.frame = CGRectMake(0, 0, 1280, 720);
            gSink->layer = layer; gSink->renderer = layer.sampleBufferRenderer;
        }
        if (gSink->renderer) {
            gSink->sync = [AVSampleBufferRenderSynchronizer new];
            gSink->sync.delaysRateChangeUntilHasSufficientMediaData = NO;
            [gSink->sync addRenderer:gSink->renderer];
        }

        pl.url = url; pl.latencies = [NSMutableArray new];
        pl.switchPlan = [NSMutableArray new];
        for (NSString *part in [gOpt.switchPlan componentsSeparatedByString:@","]) {
            NSArray *kv = [part componentsSeparatedByString:@":"]; if (kv.count == 2) [pl.switchPlan addObject:@{ @"t": @([kv[0] doubleValue]), @"h": @([kv[1] intValue]) }];
        }
        [pl buildItem];
        printf("vfi-player: %s | 출력 %s | 보간 %s | 보호 %s%s | 브레이커 %s | 화질 상한 %s | 소리 %s\n", gSourceDesc.UTF8String,
               gOpt.sink == SinkNull ? "null" : gOpt.sink == SinkRenderer ? "renderer" : "window", gOpt.interp ? "켬" : "끔",
               gOpt.useCut ? "장면전환 " : "", gOpt.useStatic ? "정지화소" : "", gOpt.useBreaker ? (gOpt.breakerV2 ? "v2" : "v3") : "끔",
               gOpt.maxHeight ? [NSString stringWithFormat:@"%dp", gOpt.maxHeight].UTF8String : "없음", gOpt.audio ? "켬" : "끔");
        // 준비될 때까지 기다린다(최대 20초)
        double waitStart = nowMs();
        while (pl.item.status == AVPlayerItemStatusUnknown && nowMs() - waitStart < 20000 && !atomic_load(&gStop))
            [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
        if (pl.item.status != AVPlayerItemStatusReadyToPlay) { printf("재생 준비 실패: %s\n", pl.item.error.localizedDescription.UTF8String ?: "시간 초과"); return 3; }
        if (!gOpt.chzzk) {
            gIsLive = CMTIME_IS_INDEFINITE(pl.item.duration);
            if (gIsLive) {   // 라이브 설정을 다시 적용
                [pl applyLiveSettings:pl.item];
                pl.player.automaticallyWaitsToMinimizeStalling = gOpt.latency == LatAVPlayer;
            }
        }
        [pl attachOutputWhenReady];
        pthread_t th; pthread_create(&th, NULL, pullerMain, NULL);
        pl.startMs = nowMs();
        [pl.player play];
        dispatch_async(gPipe, ^{ sinkSyncToItem(YES); });
        printf("  · 재생 시작(%s, 시작 대기 %.0fms)\n", gIsLive ? "라이브" : "VOD/파일", nowMs() - waitStart);

        double nextTick = nowMs() + 1000;
        while (!atomic_load(&gStop) && (nowMs() - pl.startMs) / 1000.0 < gOpt.seconds) {
            @autoreleasepool {
                if (gOpt.sink == SinkWindow) {
                    NSEvent *ev;
                    while ((ev = [NSApp nextEventMatchingMask:NSEventMaskAny untilDate:[NSDate dateWithTimeIntervalSinceNow:0.01] inMode:NSDefaultRunLoopMode dequeue:YES])) [NSApp sendEvent:ev];
                } else [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
                double nowT = nowMs();
                if (nowT >= nextTick) { nextTick += 1000; if (nextTick < nowT) nextTick = nowT + 1000; [pl secondTick]; }   // 밀린 초를 몰아서 돌리지 않는다
                if (!gIsLive && pl.item.status == AVPlayerItemStatusReadyToPlay && CMTIME_IS_NUMERIC(pl.item.duration) && sec(pl.item.currentTime) >= sec(pl.item.duration) - 0.2) {
                    if (gOpt.seekEvery > 0 || gOpt.seconds > sec(pl.item.duration)) { [pl.player seekToTime:kCMTimeZero toleranceBefore:kCMTimeZero toleranceAfter:kCMTimeZero]; [pl.player play]; }
                }
            }
        }
        pl.loopEndMs = nowMs();
        BOOL interrupted = atomic_load(&gStop) && !pl.truncated && (pl.loopEndMs - pl.startMs) / 1000.0 < gOpt.seconds - 0.5;
        atomic_store(&gStop, YES);
        pthread_join(th, NULL);
        [pl.player pause];
        // 처리 중인 요청을 마무리(최대 2초)
        double drainStart = nowMs();
        __block BOOL busy = YES;
        while (busy && nowMs() - drainStart < 2000) { dispatch_sync(gPipe, ^{ busy = P.inflight != nil; }); if (busy) usleep(5000); }
        __block NSDictionary *rmetrics = nil;
        if (gSink->renderer && [(id)gSink->renderer respondsToSelector:@selector(loadVideoPerformanceMetricsWithCompletionHandler:)]) {
            __block _Atomic BOOL got = NO;
            [(AVSampleBufferVideoRenderer *)gSink->renderer loadVideoPerformanceMetricsWithCompletionHandler:^(AVVideoPerformanceMetrics *m) {
                NSDictionary *d = m ? @{ @"total_frames": @(m.totalNumberOfFrames), @"dropped": @(m.numberOfDroppedFrames), @"corrupted": @(m.numberOfCorruptedFrames), @"accumulated_delay_s": @(m.totalAccumulatedFrameDelay), @"optimized_compositing": @(m.numberOfFramesDisplayedUsingOptimizedCompositing) } : nil;
                dispatch_async(dispatch_get_main_queue(), ^{ rmetrics = d; atomic_store(&got, YES); });
            }];
            double ws = nowMs(); while (!atomic_load(&got) && nowMs() - ws < 1500) [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
        }
        double memEnd = footprintMB(), instrEnd = instrumentMB();

        // 요약: gPipe 상태는 그 큐 안에서 모두 복사·직렬화한다(밖에서 열거하다 바뀌는 일이 없게).
        double wall = (pl.loopEndMs - pl.startMs) / 1000.0;
        __block NSDictionary *sum = nil;
        NSArray *lats = [pl.latencies copy];
        long nFirst = 0, nLast = 0;
        double latFirst = medianInWindow(lats, 5, 65, &nFirst), latLast = medianInWindow(lats, wall - 60, wall + 1, &nLast);
        NSMutableArray *latVals = [NSMutableArray new]; for (NSArray *x in lats) [latVals addObject:x[1]];
        double latMedian = medianOf(latVals);
        long stalls = pl.stalls, reconnects = pl.reconnects, timeJumps = pl.timeJumps, flushes = pl.flushes, errLog = pl.errorLogEntries;
        double memAt60 = pl.memAt60, instrAt60 = pl.instrAt60;
        BOOL truncated = pl.truncated; NSString *truncWhy = pl.truncatedWhy ?: @"";
        dispatch_sync(gPipe, ^{
            Counters c = P.c;
            long pending = (P.inflight && P.inflight->counted && !P.inflight->resolved && P.inflight->gen == P.gen) ? 1 : 0;
            long outcomes = c.onTime + c.late + c.lateStart + c.bypass + c.sessionWait + c.unsupported + c.poolExhausted + c.countedErrors + c.staleCounted + c.passOff + pending;
            double onTimeRatio = c.eligible ? (double)c.onTime / c.eligible : NAN;
            long attempted = c.onTime + c.late + c.countedErrors;
            double lateRatio = attempted ? (double)c.late / attempted : NAN;
            double errRatio = attempted ? (double)c.countedErrors / attempted : NAN;
            double countedMin = P.countedMs / 60000.0;
            double togglesPerMin = countedMin > 0 ? c.toggles / countedMin : NAN;
            double memGrowth = memAt60 ? memEnd - memAt60 : NAN;
            double memGrowthAdj = memAt60 ? (memEnd - memAt60) - (instrEnd - instrAt60) : NAN;
            NSMutableDictionary *m = [NSMutableDictionary new];
            m[@"tool"] = @"vfi-player"; m[@"version"] = @2;
            m[@"source"] = gSourceDesc; m[@"live"] = @(gIsLive);
            m[@"options"] = @{ @"sink": gOpt.sink == SinkNull ? @"null" : gOpt.sink == SinkRenderer ? @"renderer" : @"window", @"interp": @(gOpt.interp), @"cut": @(gOpt.useCut), @"static": @(gOpt.useStatic), @"breaker": gOpt.useBreaker ? (gOpt.breakerV2 ? @"v2" : @"v3") : @"off",
                               @"max_height": @(gOpt.maxHeight), @"latency": gOpt.latency == LatWeb ? @"web" : gOpt.latency == LatAVPlayer ? @"avplayer" : @"off", @"live_offset_s": @(gOpt.liveOffset),
                               @"lead_ms": @(gOpt.leadMs), @"lookahead_ms": @(gOpt.lookaheadMs), @"max_ahead": @(gOpt.maxAhead), @"seek_every_s": @(gOpt.seekEvery), @"switch": gOpt.switchPlan ?: @"", @"rate_toggle_s": @(gOpt.rateToggle),
                               @"forward_buffer_s": @(gOpt.forwardBuffer), @"inject_delay": [NSString stringWithFormat:@"%g:%g", gOpt.injectPct, gOpt.injectMs],
                               @"cut_sad": @(gOpt.cutSad), @"cut_hist": @(gOpt.cutHist), @"static_tau": @(gOpt.staticTau), @"static_ms": @(gOpt.staticMs), @"seconds": @(gOpt.seconds) };
            m[@"environment"] = @{ @"model": sysctlString("hw.model"), @"chip": sysctlString("machdep.cpu.brand_string"), @"os": NSProcessInfo.processInfo.operatingSystemVersionString, @"thermal_end": @(NSProcessInfo.processInfo.thermalState) };
            m[@"wall_s"] = @(wall); m[@"counted_s"] = @(P.countedMs / 1000.0);
            m[@"truncated"] = @(truncated || interrupted); m[@"truncated_why"] = truncated ? truncWhy : interrupted ? @"사용자 중단" : @"";
            m[@"counts"] = @{ @"frames": @(c.frames), @"pairs": @(c.pairs), @"eligible": @(c.eligible), @"on_time": @(c.onTime), @"late": @(c.late), @"late_start": @(c.lateStart), @"abandoned": @(c.abandoned),
                              @"late_completed": @(c.lateCompleted), @"composite_late": @(c.compositeLate), @"errors": @(c.errors), @"counted_errors": @(c.countedErrors), @"cuts": @(c.cuts), @"gaps": @(c.gaps), @"bypass": @(c.bypass), @"breaker_trips": @(P.br.trips),
                              @"session_wait": @(c.sessionWait), @"unsupported": @(c.unsupported), @"pool_exhausted": @(c.poolExhausted), @"interp_off": @(c.passOff), @"runtime_off": @(c.runtimeOff), @"warm_pairs": @(c.warmPairs), @"stalled_pairs": @(c.stalledPairs),
                              @"stale_dropped": @(c.staleDropped), @"stale_counted": @(c.staleCounted), @"pending_at_end": @(pending), @"static_skipped": @(c.staticSkipped), @"vt_restarts": @(c.vtRestarts), @"order_violation": @(c.orderViolation), @"orig_late": @(c.origLate),
                              @"emitted_orig": @(c.emittedOrig), @"emitted_mid": @(c.emittedMid),
                              @"generations": @(P.gen), @"bumps": @(c.bumps), @"soft_bumps": @(c.softBumps), @"coalesced_bumps": @(c.coalescedBumps), @"fps_changes": @(c.fpsChanges), @"size_changes": @(c.sizeChanges), @"toggles": @(c.toggles),
                              @"stalls": @(stalls), @"reconnects": @(reconnects), @"time_jumps": @(timeJumps), @"output_flushes": @(flushes), @"avplayer_error_log": @(errLog),
                              @"sink_enqueued": @(gSink->enqueued), @"sink_enqueue_failed": @(gSink->enqueueFailed), @"sink_resyncs": @(gSink->resyncs) };
            m[@"accounting_ok"] = @(outcomes == c.eligible);
            m[@"breaker"] = @{ @"trips": @(P.br.trips), @"probes": @(P.br.probes), @"extensions": @(P.br.extensions), @"probe_ms": distribution(&P.probeMs) };
            m[@"ms"] = @{ @"proc": distribution(&P.procMs), @"composite": distribution(&P.compMs), @"guard": distribution(&P.guardMs), @"lookahead": distribution(&P.lookMs), @"slack_at_ready": distribution(&P.slackMs), @"submit_lead": distribution(&P.submitLeadMs) };
            m[@"cut_max_normal"] = @{ @"sad": @(P.cutSadMaxNormal), @"hist": @(P.cutHistMaxNormal) };
            m[@"sessions"] = [P.sessionLog copy];
            m[@"latency_s"] = @{ @"median": @(latMedian), @"first_min_median": @(latFirst), @"last_min_median": @(latLast), @"n": @(lats.count), @"reset_by_reconnect": @(reconnects > 0) };
            m[@"memory_mb"] = @{ @"at_60s": memAt60 ? @(memAt60) : [NSNull null], @"end": @(memEnd), @"growth_after_60s": @(memGrowth), @"instrument_growth": @(instrEnd - instrAt60), @"growth_after_60s_excluding_instrument": @(memGrowthAdj) };
            if (rmetrics) m[@"renderer_metrics"] = rmetrics;
            m[@"ratios"] = @{ @"on_time": @(onTimeRatio), @"late_of_attempted": @(lateRatio), @"error_of_attempted": @(errRatio), @"toggles_per_counted_min": @(togglesPerMin) };
            // 기준 판정(docs/research/vfi-dev-plan.md 2장). A/V 동기·웹 대비 지연(C7)은 이 모드로 잴 수 없다.
            NSMutableDictionary *cr = [NSMutableDictionary new];
            if (gOpt.interp && P.countedMs >= 30000) {   // C2: 집계 대상 30초 이상
                cr[@"C2_on_time_ge_95pct"] = @(onTimeRatio >= 0.95);
                cr[@"C2_late_le_1pct"] = @(isfinite(lateRatio) && lateRatio <= 0.01);
                cr[@"C2_toggles_le_60_per_min"] = @(togglesPerMin <= 60);
            }
            if (isfinite(memGrowthAdj) && wall >= 600) cr[@"C8_memory_growth_le_50mb"] = @(memGrowthAdj <= 50);
            if (wall >= 600 && reconnects == 0 && nFirst >= 30 && nLast >= 30) cr[@"C8_latency_not_growing"] = @(latLast - latFirst <= 0.5);
            cr[@"safety_no_stale_or_reordered_output"] = @(c.orderViolation == 0);
            cr[@"accounting_ok"] = @(outcomes == c.eligible);
            m[@"criteria"] = cr;
            m[@"events_total"] = @(P.events.count);
            m[@"events"] = [P.events subarrayWithRange:NSMakeRange(0, MIN(P.events.count, (NSUInteger)600))];
            sum = jsonSafe(m);
        });
        NSDictionary *cnt = sum[@"counts"];
        printf("%s", [NSString stringWithFormat:@"\n요약: %.0f초(집계 %.0f초), 프레임 %@, 보간 대상 짝 %@, 제때 %@(%.1f%%), 늦음 %@, 시작 못함 %@, 브레이커 우회 %@(열림 %@), 오류 %@, 장면전환 %@, 세대 %@, 순서 위반 %@, 버린 이전 결과 %@\n",
               wall, [sum[@"counted_s"] doubleValue], cnt[@"frames"], cnt[@"eligible"], cnt[@"on_time"],
               [sum[@"ratios"][@"on_time"] isKindOfClass:[NSNumber class]] ? [sum[@"ratios"][@"on_time"] doubleValue] * 100 : 0.0,
               cnt[@"late"], cnt[@"late_start"], cnt[@"bypass"], cnt[@"breaker_trips"], cnt[@"counted_errors"], cnt[@"cuts"], cnt[@"generations"], cnt[@"order_violation"], cnt[@"stale_dropped"]].UTF8String);
        printf("처리 시간 p50 %.2f p95 %.2f p99 %.2fms | 앞당김 p50 %.0f 최소 %.0fms | 지연 중앙값 %s | 메모리 증가 %s(측정 기록 제외)%s\n",
               [sum[@"ms"][@"proc"][@"p50"] doubleValue], [sum[@"ms"][@"proc"][@"p95"] doubleValue], [sum[@"ms"][@"proc"][@"p99"] doubleValue],
               [sum[@"ms"][@"lookahead"][@"p50"] doubleValue], [sum[@"ms"][@"lookahead"][@"min"] doubleValue],
               [sum[@"latency_s"][@"median"] isKindOfClass:[NSNumber class]] ? [NSString stringWithFormat:@"%.2fs", [sum[@"latency_s"][@"median"] doubleValue]].UTF8String : "-",
               [sum[@"memory_mb"][@"growth_after_60s_excluding_instrument"] isKindOfClass:[NSNumber class]] ? [NSString stringWithFormat:@"%.1fMB", [sum[@"memory_mb"][@"growth_after_60s_excluding_instrument"] doubleValue]].UTF8String : "-",
               [sum[@"truncated"] boolValue] ? [NSString stringWithFormat:@" | 일찍 끝남: %@", sum[@"truncated_why"]].UTF8String : "");
        for (NSString *k in [[sum[@"criteria"] allKeys] sortedArrayUsingSelector:@selector(compare:)]) printf("  %s %s\n", [sum[@"criteria"][k] boolValue] ? "✓" : "✗", k.UTF8String);
        if (gOpt.jsonOut) {
            NSError *je = nil;
            NSData *d = [NSJSONSerialization dataWithJSONObject:sum options:NSJSONWritingPrettyPrinted | NSJSONWritingSortedKeys error:&je];
            if (!d || ![d writeToFile:gOpt.jsonOut options:NSDataWritingAtomic error:&je]) { printf("JSON 쓰기 실패: %s\n", je.localizedDescription.UTF8String); fprintf(stderr, "JSON 쓰기 실패: %s\n", je.localizedDescription.UTF8String); exitCode = 4; }
        }
        if (pl.csv) { [pl.csv synchronizeAndReturnError:nil]; [pl.csv closeAndReturnError:nil]; if (pl.csvFailed) exitCode = 4; }
        dispatch_sync(gPipe, ^{ for (VFSession *s in P.sessions.allValues) if (s->proc) [s->proc endSession]; });
        [NSProcessInfo.processInfo endActivity:activity];
        if (!exitCode && [sum[@"truncated"] boolValue]) exitCode = 5;
    }
    return exitCode;
}

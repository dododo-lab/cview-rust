// CView 프레임 보간 연구용 벤치마크 (앱 코드와 무관한 독립 도구).
//
// VideoToolbox VTFrameProcessor 의 두 보간기를 같은 입력으로 돌려
//   - 프레임당 처리 시간(첫 세션 시작·모델 적재 시간 포함)
//   - 실제 중간 프레임(정답) 대비 PSNR(전체·채팅·자막·빠른 물체·타이머 영역)
//   - 일정한 속도로 넣을 때(실시간 모사) 마감 초과 비율과 CPU 시간
// 을 잰다. 비교 기준으로 '그대로 두기(hold)'와 '단순 섞기(blend)'도 함께 잰다.
//
//   lowlat : VTLowLatencyFrameInterpolation (macOS 26+, 이전 프레임+현재 프레임 → 중간)
//   frc    : VTFrameRateConversion (macOS 15.4+, 품질 우선순위 normal)
//   frcq   : VTFrameRateConversion (품질 우선순위 quality)
//
// 빌드·실행은 같은 폴더의 run.sh 를 본다.

#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#import <VideoToolbox/VideoToolbox.h>
#import <CoreVideo/CoreVideo.h>
#import <CoreText/CoreText.h>
#import <ImageIO/ImageIO.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#include <mach/mach_time.h>
#include <sys/resource.h>
#include <math.h>
#include <pthread.h>
#include <mach/thread_policy.h>
#include <mach/thread_act.h>

static double nowMs(void) {
    static mach_timebase_info_data_t tb;
    if (tb.denom == 0) mach_timebase_info(&tb);
    return (double)mach_absolute_time() * tb.numer / tb.denom / 1e6;
}

static double cpuMs(void) {
    struct rusage ru; getrusage(RUSAGE_SELF, &ru);
    return (ru.ru_utime.tv_sec + ru.ru_stime.tv_sec) * 1e3 + (ru.ru_utime.tv_usec + ru.ru_stime.tv_usec) / 1e3;
}

static CVPixelBufferRef makeBGRA(int w, int h) {
    NSDictionary *attrs = @{ (id)kCVPixelBufferIOSurfacePropertiesKey: @{},
                             (id)kCVPixelBufferMetalCompatibilityKey: @YES };
    CVPixelBufferRef pb = NULL;
    CVPixelBufferCreate(kCFAllocatorDefault, w, h, kCVPixelFormatType_32BGRA, (__bridge CFDictionaryRef)attrs, &pb);
    return pb;
}

static CVPixelBufferRef makeFromAttrs(NSDictionary *attrs, int w, int h, OSType fallbackFmt) {
    NSMutableDictionary *a = [attrs mutableCopy] ?: [NSMutableDictionary new];
    a[(id)kCVPixelBufferWidthKey] = @(w);
    a[(id)kCVPixelBufferHeightKey] = @(h);
    if (!a[(id)kCVPixelBufferIOSurfacePropertiesKey]) a[(id)kCVPixelBufferIOSurfacePropertiesKey] = @{};
    id fmt = a[(id)kCVPixelBufferPixelFormatTypeKey];
    OSType f = fallbackFmt;
    if ([fmt isKindOfClass:[NSNumber class]]) f = [fmt unsignedIntValue];
    else if ([fmt isKindOfClass:[NSArray class]] && [fmt count]) f = [[fmt firstObject] unsignedIntValue];
    [a removeObjectForKey:(id)kCVPixelBufferPixelFormatTypeKey];
    CVPixelBufferRef pb = NULL;
    CVReturn r = CVPixelBufferCreate(kCFAllocatorDefault, w, h, f, (__bridge CFDictionaryRef)a, &pb);
    if (r != kCVReturnSuccess) { fprintf(stderr, "CVPixelBufferCreate 실패 %d (fmt %.4s)\n", r, (char *)&f); exit(2); }
    return pb;
}

// ───────────────────────── 합성 장면 ─────────────────────────
// 라이브 방송에서 보간이 망가지기 쉬운 요소를 한 화면에 모은다.
typedef struct { CGRect chat, subtitle, ball, timer; } Regions;

static const char *kChat[] = {
    "치즈냥: ㅋㅋㅋㅋㅋ 방금 그거 뭐야", "하늘별: 와 컨트롤 미쳤다", "dododo: 1080p60 보간 테스트 중",
    "고양이발: 다음 판 가자!!", "Lumi: 채팅 오버레이 글자 번짐 확인", "초코칩: 🎉🎉 축하합니다",
    "밤하늘: 지연 2초 유지되나요?", "pixel: 빠른 화면 전환에서 깨짐?", "산책: 자막 고정 영역 테스트",
    "RIFE: 신경망 보간 vs 하드웨어", "구름: ㅎㅎ", "bitrate: 8Mbps 1080p60",
};

static Regions sceneRegions(int W, int H) {
    Regions r;
    r.chat = CGRectMake(W - W * 0.22, H * 0.08, W * 0.20, H * 0.62);
    r.subtitle = CGRectMake(W * 0.18, H * 0.84, W * 0.52, H * 0.08);
    r.ball = CGRectMake(0, H * 0.40, W * 0.76, H * 0.12);
    r.timer = CGRectMake(W * 0.02, H * 0.02, W * 0.22, H * 0.07);
    return r;
}

static void drawText(CGContextRef ctx, NSString *s, CGFloat x, CGFloat y, CGFloat size, CGColorRef color) {
    CTFontRef font = CTFontCreateWithName(CFSTR("Apple SD Gothic Neo"), size, NULL);
    NSDictionary *attrs = @{ (id)kCTFontAttributeName: (__bridge id)font, (id)kCTForegroundColorAttributeName: (__bridge id)color };
    NSAttributedString *as = [[NSAttributedString alloc] initWithString:s attributes:attrs];
    CTLineRef line = CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)as);
    CGContextSetTextPosition(ctx, x, y);
    CTLineDraw(line, ctx);
    CFRelease(line); CFRelease(font);
}

// 장면 번호. --cuts N 이면 입력 N장마다 바뀐다(배경·물체만 바뀌고, 방송 화면에 박힌 타이머·채팅·자막은 그대로).
static int gVariant = 0;

// y 는 위에서부터(화면 좌표). CG 는 아래에서부터라 뒤집는다.
static void renderScene(CVPixelBufferRef pb, double t) {
    const int v = gVariant;
    CVPixelBufferLockBaseAddress(pb, 0);
    int W = (int)CVPixelBufferGetWidth(pb), H = (int)CVPixelBufferGetHeight(pb);
    CGColorSpaceRef cs = CGColorSpaceCreateDeviceRGB();
    CGContextRef ctx = CGBitmapContextCreate(CVPixelBufferGetBaseAddress(pb), W, H, 8, CVPixelBufferGetBytesPerRow(pb), cs,
                                             kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    CGContextTranslateCTM(ctx, 0, H); CGContextScaleCTM(ctx, 1, -1);  // 위→아래 좌표
    double s = W / 1920.0;

    // 1) 가로로 흘러가는 배경(카메라 팬): 1200 px/s
    double pan = fmod((v % 2 ? -1 : 1) * t * 1200.0 * s + v * 777.0 * s + 64.0 * 40 * s * 1000, 64.0 * 40 * s);
    for (int i = -1; i < 40; i++) {
        double x = i * 64.0 * s - pan + 64.0 * 40 * s * (i < 0);
        for (int k = 0; k < 2; k++) {
            double xx = x + k * 64.0 * 40 * s;
            unsigned hsh = (unsigned)((i + 41) % 40 + 40 * v) * 2654435761u;
            CGFloat c1 = ((hsh >> 8) & 255) / 255.0, c2 = ((hsh >> 16) & 255) / 255.0;
            if (v % 2) CGContextSetRGBFillColor(ctx, 0.45 + 0.5 * c2, 0.25 + 0.5 * c1, 0.1 + 0.3 * c2, 1);
            else CGContextSetRGBFillColor(ctx, 0.15 + 0.5 * c1, 0.2 + 0.4 * c2, 0.35 + 0.3 * (1 - c1), 1);
            CGContextFillRect(ctx, CGRectMake(xx, 0, 64.0 * s + 1, H));
            CGContextSetRGBFillColor(ctx, 1, 1, 1, 0.25);
            for (int y = 0; y < H; y += (int)(48 * s)) CGContextFillRect(ctx, CGRectMake(xx + 8 * s, y, 4 * s, 24 * s));
        }
    }
    // 2) 회전 막대(물체 운동): 180°/s
    CGContextSaveGState(ctx);
    CGContextTranslateCTM(ctx, W * (0.30 + 0.22 * (v % 2)), H * (0.24 + 0.05 * (v % 3)));
    CGContextRotateCTM(ctx, t * M_PI + v * 1.1);
    if (v % 2) CGContextSetRGBFillColor(ctx, 0.2, 0.85, 0.95, 1); else CGContextSetRGBFillColor(ctx, 0.95, 0.75, 0.1, 1);
    CGContextFillRect(ctx, CGRectMake(-260 * s, -22 * s, 520 * s, 44 * s));
    CGContextRestoreGState(ctx);
    // 3) 빠른 공: 2400 px/s 왕복
    Regions R = sceneRegions(W, H);
    double span = R.ball.size.width - 120 * s;
    double ph = fmod((t + v * 0.37) * 2400.0 * s, 2 * span);
    double bx = ph < span ? ph : 2 * span - ph;
    CGContextSetRGBFillColor(ctx, 0.9, 0.1, 0.2, 1);
    CGContextFillEllipseInRect(ctx, CGRectMake(bx, R.ball.origin.y + 6 * s, 110 * s, 110 * s));
    // 4) 프레임마다 바뀌는 타이머(점수판·시계 같은 글자)
    CGContextSetRGBFillColor(ctx, 0, 0, 0, 0.75);
    CGContextFillRect(ctx, R.timer);
    CGColorRef white = CGColorCreateGenericRGB(1, 1, 1, 1);
    CGColorRef yellow = CGColorCreateGenericRGB(1, 0.9, 0.3, 1);
    CGContextSaveGState(ctx);
    CGContextTranslateCTM(ctx, 0, H); CGContextScaleCTM(ctx, 1, -1);  // 글자는 CG 좌표로
    int ms = (int)llround(t * 1000);
    drawText(ctx, [NSString stringWithFormat:@"LIVE %02d:%02d.%03d", ms / 60000, (ms / 1000) % 60, ms % 1000],
             R.timer.origin.x + 16 * s, H - R.timer.origin.y - R.timer.size.height * 0.72, 44 * s, white);
    CGContextRestoreGState(ctx);
    // 5) 채팅 오버레이: 반투명 상자, 줄이 60 px/s 로 올라감
    CGContextSetRGBFillColor(ctx, 0, 0, 0, 0.55);
    CGContextFillRect(ctx, R.chat);
    CGContextSaveGState(ctx);
    CGContextClipToRect(ctx, R.chat);
    CGContextTranslateCTM(ctx, 0, H); CGContextScaleCTM(ctx, 1, -1);
    double lineH = 40 * s, scroll = t * 60 * s;
    int first = (int)floor(scroll / lineH);
    for (int i = first; i < first + 20; i++) {
        double yTop = R.chat.origin.y + R.chat.size.height - (scroll - i * lineH) - lineH * 16;
        NSString *line = [NSString stringWithUTF8String:kChat[((i % 12) + 12) % 12]];
        drawText(ctx, line, R.chat.origin.x + 12 * s, H - yTop - lineH * 0.7, 24 * s, (i % 3) ? white : yellow);
    }
    CGContextRestoreGState(ctx);
    // 6) 고정 자막(움직이는 배경 위 정지 글자)
    CGContextSetRGBFillColor(ctx, 0, 0, 0, 0.6);
    CGContextFillRect(ctx, R.subtitle);
    CGContextSaveGState(ctx);
    CGContextTranslateCTM(ctx, 0, H); CGContextScaleCTM(ctx, 1, -1);
    drawText(ctx, @"자막: 빠르게 움직이는 화면 위 고정 글자 테스트", R.subtitle.origin.x + 24 * s,
             H - R.subtitle.origin.y - R.subtitle.size.height * 0.68, 40 * s, white);
    CGContextRestoreGState(ctx);

    CGColorRelease(white); CGColorRelease(yellow);
    CGContextRelease(ctx); CGColorSpaceRelease(cs);
    CVPixelBufferUnlockBaseAddress(pb, 0);
}

// ───────────────────────── 파일 입력 ─────────────────────────
@interface FileSource : NSObject
@property AVAssetReader *reader; @property AVAssetReaderTrackOutput *out; @property double fps; @property int w, h;
@end
@implementation FileSource
- (instancetype)initWithPath:(NSString *)p maxWidth:(int)mw {
    self = [super init];
    AVURLAsset *asset = [AVURLAsset URLAssetWithURL:[NSURL fileURLWithPath:p] options:nil];
    AVAssetTrack *tr = [[asset tracksWithMediaType:AVMediaTypeVideo] firstObject];
    if (!tr) { fprintf(stderr, "비디오 트랙 없음\n"); exit(2); }
    _fps = tr.nominalFrameRate;
    CGSize n = tr.naturalSize; _w = (int)n.width; _h = (int)n.height;
    NSMutableDictionary *os = [@{ (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA),
                                  (id)kCVPixelBufferIOSurfacePropertiesKey: @{} } mutableCopy];
    if (mw && _w > mw) { _h = (int)lround(_h * (double)mw / _w) & ~1; _w = mw;
        os[(id)kCVPixelBufferWidthKey] = @(_w); os[(id)kCVPixelBufferHeightKey] = @(_h); }
    _reader = [AVAssetReader assetReaderWithAsset:asset error:nil];
    _out = [AVAssetReaderTrackOutput assetReaderTrackOutputWithTrack:tr outputSettings:os];
    _out.alwaysCopiesSampleData = NO;
    [_reader addOutput:_out]; [_reader startReading];
    return self;
}
- (CVPixelBufferRef)next {  // +1 retained
    CMSampleBufferRef sb = [_out copyNextSampleBuffer];
    if (!sb) return NULL;
    CVPixelBufferRef pb = CMSampleBufferGetImageBuffer(sb);
    CVPixelBufferRef copy = makeBGRA(_w, _h);
    CVPixelBufferLockBaseAddress(pb, kCVPixelBufferLock_ReadOnly); CVPixelBufferLockBaseAddress(copy, 0);
    size_t sbpr = CVPixelBufferGetBytesPerRow(pb), dbpr = CVPixelBufferGetBytesPerRow(copy);
    for (int y = 0; y < _h; y++) memcpy((char *)CVPixelBufferGetBaseAddress(copy) + y * dbpr, (char *)CVPixelBufferGetBaseAddress(pb) + y * sbpr, _w * 4);
    CVPixelBufferUnlockBaseAddress(copy, 0); CVPixelBufferUnlockBaseAddress(pb, kCVPixelBufferLock_ReadOnly);
    CFRelease(sb);
    return copy;
}
@end

// ───────────────────────── 품질 지표 ─────────────────────────
// BGRA 두 장의 휘도(BT.601 근사) PSNR, rect 안에서만.
static double psnrY(CVPixelBufferRef a, CVPixelBufferRef b, CGRect r) {
    CVPixelBufferLockBaseAddress(a, kCVPixelBufferLock_ReadOnly); CVPixelBufferLockBaseAddress(b, kCVPixelBufferLock_ReadOnly);
    int W = (int)CVPixelBufferGetWidth(a), H = (int)CVPixelBufferGetHeight(a);
    int x0 = MAX(0, (int)r.origin.x), y0 = MAX(0, (int)r.origin.y);
    int x1 = MIN(W, (int)CGRectGetMaxX(r)), y1 = MIN(H, (int)CGRectGetMaxY(r));
    const uint8_t *pa = CVPixelBufferGetBaseAddress(a), *pb = CVPixelBufferGetBaseAddress(b);
    size_t ra = CVPixelBufferGetBytesPerRow(a), rb = CVPixelBufferGetBytesPerRow(b);
    double se = 0; long n = 0;
    for (int y = y0; y < y1; y++) {
        const uint8_t *qa = pa + y * ra, *qb = pb + y * rb;
        for (int x = x0; x < x1; x++) {
            int ya = (29 * qa[4 * x] + 150 * qa[4 * x + 1] + 77 * qa[4 * x + 2]) >> 8;
            int yb = (29 * qb[4 * x] + 150 * qb[4 * x + 1] + 77 * qb[4 * x + 2]) >> 8;
            int d = ya - yb; se += d * d; n++;
        }
    }
    CVPixelBufferUnlockBaseAddress(a, kCVPixelBufferLock_ReadOnly); CVPixelBufferUnlockBaseAddress(b, kCVPixelBufferLock_ReadOnly);
    double mse = se / MAX(n, 1);
    return mse <= 1e-9 ? 99.0 : 10 * log10(255.0 * 255.0 / mse);
}

static void blendBGRA(CVPixelBufferRef a, CVPixelBufferRef b, CVPixelBufferRef out, double phase) {
    CVPixelBufferLockBaseAddress(a, kCVPixelBufferLock_ReadOnly); CVPixelBufferLockBaseAddress(b, kCVPixelBufferLock_ReadOnly);
    CVPixelBufferLockBaseAddress(out, 0);
    int W = (int)CVPixelBufferGetWidth(a), H = (int)CVPixelBufferGetHeight(a);
    int wb = (int)lround(phase * 256), wa = 256 - wb;
    for (int y = 0; y < H; y++) {
        const uint8_t *qa = (uint8_t *)CVPixelBufferGetBaseAddress(a) + y * CVPixelBufferGetBytesPerRow(a);
        const uint8_t *qb = (uint8_t *)CVPixelBufferGetBaseAddress(b) + y * CVPixelBufferGetBytesPerRow(b);
        uint8_t *qo = (uint8_t *)CVPixelBufferGetBaseAddress(out) + y * CVPixelBufferGetBytesPerRow(out);
        for (int x = 0; x < W * 4; x++) qo[x] = (uint8_t)((qa[x] * wa + qb[x] * wb) >> 8);
    }
    CVPixelBufferUnlockBaseAddress(out, 0);
    CVPixelBufferUnlockBaseAddress(a, kCVPixelBufferLock_ReadOnly); CVPixelBufferUnlockBaseAddress(b, kCVPixelBufferLock_ReadOnly);
}

// ───────────────────────── 보호 장치(가드) ─────────────────────────
// 보간기(VT 또는 이후의 신경망)를 바꾸지 않고, 결과 위에 씌우는 세 가지 장치. 모두 휘도만 본다.
//   cut    : 장면 전환 감지. 64×36 축소 휘도의 평균 절대차(SAD)와 32칸 휘도 히스토그램 L1 거리가
//            둘 다 문턱을 넘으면 그 쌍은 보간하지 않고 앞 프레임을 그대로 쓴다.
//   static : 오래 정지한 화소 보호. 화소마다 |Y(n)-Y(n-1)| ≤ 3 이 K장(기본 120ms 분량) 이어졌으면(방송 화면에 박힌 자막·HUD·
//            상자 테두리) 보간 대신 두 원본의 평균(=원본과 거의 같음)을 쓴다. (--static-erode 1 이면 3×3 침식으로
//            경계를 보간에 남긴다. 720p 시험에서 가는 글자 획까지 지워 효과가 없어 기본은 끔.)
//   conf   : 움직임으로 설명되지 않는 변화. 32×32 타일마다 앞·뒤 프레임 차이가 큰데, 1/8 해상도 ±12칸(전체 ±96px)
//            탐색 + 1/4 해상도 ±2칸 다듬기로 찾은 최선의 평행 이동으로도 차이가 남으면(바뀌는 숫자·점수판·
//            새로 뜨는 알림) 그 타일은 앞 프레임으로 되돌린다. 타일 가중치는 쌍선형으로 펴서 경계 이음매를 없앤다.
//            반투명 채팅 상자처럼 한 타일에 두 층(느린 글자 + 빠른 배경)이 겹치면 이동 하나로는 설명이 안 되므로,
//            기본(--g-conf-layers 2)은 정지·최선 이동·두 번째 이동 가운데 화소별 최소 잔차로 판단한다.
typedef struct {
    int W, H;
    double cutSad, cutHist;   // 장면 전환 문턱(평균 절대차 /255, 히스토그램 L1 0~2)
    int staticTau, staticK, staticErode;   // 정지 판정 휘도 차, 이어진 장 수, 3×3 침식 여부
    double confAbs, confRel;  // 설명 안 되는 차 문턱: max(confAbs, confRel × 원래 차)
    int confLayers;           // 1 = 평행 이동 하나로 설명, 2 = 정지·최선·두 번째 이동 중 화소별 최소(두 층 움직임)
    uint8_t *yPrev, *yCur, *cnt, *prot;
    uint8_t *q0, *q1, *e0, *e1; // 1/4, 1/8 해상도 휘도
    float *tw;                // 타일 가중치
    float thumbPrev[64 * 36], thumbCur[64 * 36];
    double histPrev[32], histCur[32];
    double msThumb, msStatic, msConf; long nCalls, nConf;
} Guard;

static void lumaOf(CVPixelBufferRef pb, uint8_t *Y) {
    CVPixelBufferLockBaseAddress(pb, kCVPixelBufferLock_ReadOnly);
    int W = (int)CVPixelBufferGetWidth(pb), H = (int)CVPixelBufferGetHeight(pb);
    const uint8_t *p = CVPixelBufferGetBaseAddress(pb); size_t rb = CVPixelBufferGetBytesPerRow(pb);
    dispatch_apply(H, DISPATCH_APPLY_AUTO, ^(size_t y) {
        const uint8_t *q = p + y * rb; uint8_t *o = Y + y * W;
        for (int x = 0; x < W; x++) o[x] = (uint8_t)((29 * q[4 * x] + 150 * q[4 * x + 1] + 77 * q[4 * x + 2]) >> 8);
    });
    CVPixelBufferUnlockBaseAddress(pb, kCVPixelBufferLock_ReadOnly);
}

static void boxDown(const uint8_t *src, int W, int H, int f, uint8_t *dst) {  // f×f 평균 축소
    int w = W / f, h = H / f;
    dispatch_apply(h, DISPATCH_APPLY_AUTO, ^(size_t y) {
        for (int x = 0; x < w; x++) {
            int s = 0;
            for (int dy = 0; dy < f; dy++) { const uint8_t *r = src + (y * f + dy) * W + x * f; for (int dx = 0; dx < f; dx++) s += r[dx]; }
            dst[y * w + x] = (uint8_t)(s / (f * f));
        }
    });
}

static void thumbHist(const uint8_t *Y, int W, int H, float *th, double *hist) {
    memset(hist, 0, 32 * sizeof(double));
    for (int ty = 0; ty < 36; ty++) for (int tx = 0; tx < 64; tx++) {
        int x0 = tx * W / 64, x1 = (tx + 1) * W / 64, y0 = ty * H / 36, y1 = (ty + 1) * H / 36; long s = 0, n = 0;
        for (int y = y0; y < y1; y += 2) for (int x = x0; x < x1; x += 2) { s += Y[y * W + x]; n++; hist[Y[y * W + x] >> 3] += 1; }
        th[ty * 64 + tx] = (float)s / MAX(n, 1);
    }
    double tot = 0; for (int i = 0; i < 32; i++) tot += hist[i];
    for (int i = 0; i < 32; i++) hist[i] /= MAX(tot, 1);
}

static Guard *guardNew(int W, int H) {
    Guard *g = calloc(1, sizeof(Guard));
    g->W = W; g->H = H;
    g->cutSad = 0.10; g->cutHist = 0.30; g->staticTau = 3; g->staticK = 4; g->staticErode = 0; g->confAbs = 6.0; g->confRel = 0.45; g->confLayers = 2;
    g->yPrev = malloc(W * H); g->yCur = malloc(W * H); g->cnt = calloc(W * H, 1); g->prot = calloc(W * H, 1);
    g->q0 = malloc(W * H / 16); g->q1 = malloc(W * H / 16); g->e0 = malloc(W * H / 64); g->e1 = malloc(W * H / 64);
    g->tw = calloc((W / 32 + 1) * (H / 32 + 1), sizeof(float));
    return g;
}

// 새 입력 프레임(BGRA)을 받아 정지 카운터·축소본을 갱신한다. 첫 프레임이면 prev 도 같은 값으로.
static void guardPush(Guard *g, CVPixelBufferRef cur, BOOL first) {
    int W = g->W, H = g->H, n = W * H;
    uint8_t *t = g->yPrev; g->yPrev = g->yCur; g->yCur = t;
    memcpy(g->thumbPrev, g->thumbCur, sizeof g->thumbCur); memcpy(g->histPrev, g->histCur, sizeof g->histCur);
    double t0 = nowMs();
    lumaOf(cur, g->yCur);
    thumbHist(g->yCur, W, H, g->thumbCur, g->histCur);
    double t1 = nowMs();
    if (first) { memcpy(g->yPrev, g->yCur, n); memcpy(g->thumbPrev, g->thumbCur, sizeof g->thumbCur); memcpy(g->histPrev, g->histCur, sizeof g->histCur); }
    uint8_t *yp = g->yPrev, *yc = g->yCur, *cnt = g->cnt, *prot = g->prot; int tau = g->staticTau, K = g->staticK, er = g->staticErode;
    dispatch_apply(H, DISPATCH_APPLY_AUTO, ^(size_t y) {
        for (int x = 0; x < W; x++) { int i = (int)y * W + x; int d = abs((int)yc[i] - (int)yp[i]);
            cnt[i] = d <= tau ? (cnt[i] < 255 ? cnt[i] + 1 : 255) : 0; }
    });
    dispatch_apply(H, DISPATCH_APPLY_AUTO, ^(size_t y) {   // 3×3 침식(er=1) 또는 화소 그대로(er=0)
        for (int x = 0; x < W; x++) { int ok = 1;
            if (!er) { prot[y * W + x] = cnt[y * W + x] >= K; continue; }
            for (int dy = -1; dy <= 1 && ok; dy++) for (int dx = -1; dx <= 1; dx++) {
                int yy = MIN(MAX((int)y + dy, 0), H - 1), xx = MIN(MAX(x + dx, 0), W - 1);
                if (cnt[yy * W + xx] < K) { ok = 0; break; } }
            prot[y * W + x] = (uint8_t)ok; }
    });
    double t2 = nowMs();
    g->msThumb += t1 - t0; g->msStatic += t2 - t1; g->nCalls++;
}

static BOOL guardIsCut(Guard *g, double *sadOut, double *histOut) {
    double sad = 0, hd = 0;
    for (int i = 0; i < 64 * 36; i++) sad += fabsf(g->thumbCur[i] - g->thumbPrev[i]);
    sad /= 64 * 36 * 255.0;
    for (int i = 0; i < 32; i++) hd += fabs(g->histCur[i] - g->histPrev[i]);
    if (sadOut) *sadOut = sad;
    if (histOut) *histOut = hd;
    return sad >= g->cutSad && hd >= g->cutHist;
}

// 타일 가중치(1 = 앞 프레임으로 되돌림)를 g->tw 에 채운다. 반환: 되돌린 타일 비율.
static double guardConf(Guard *g) {
    double t0 = nowMs();
    int W = g->W, H = g->H, qw = W / 4, qh = H / 4, ew = W / 8, eh = H / 8;
    boxDown(g->yPrev, W, H, 4, g->q0); boxDown(g->yCur, W, H, 4, g->q1);
    boxDown(g->q0, qw, qh, 2, g->e0); boxDown(g->q1, qw, qh, 2, g->e1);
    int tx = W / 32, ty = H / 32; const int R = 12;
    const uint8_t *q0 = g->q0, *q1 = g->q1, *e0 = g->e0, *e1 = g->e1; float *tw = g->tw;
    double cAbs = g->confAbs, cRel = g->confRel; int layers = g->confLayers;
    __block int nBack = 0;
    dispatch_apply(ty, DISPATCH_APPLY_AUTO, ^(size_t j) {
        int back = 0;
        for (int i = 0; i < tx; i++) {
            int bx = i * 8, by = (int)j * 8; long d0 = 0;   // 1/4 해상도 8×8 타일의 원래 차
            for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) d0 += abs((int)q1[(by + y) * qw + bx + x] - (int)q0[(by + y) * qw + bx + x]);
            double D = d0 / 64.0; float wgt = 0;
            if (D > cAbs) {
                // 1/8 해상도 4×4 타일로 ±R 탐색(현재 타일이 앞 프레임의 어디서 왔나). 최선 이동과,
                // 그와 2칸 이상 떨어진 두 번째 이동(다른 층의 움직임)을 함께 찾는다.
                int cx = i * 4, cy = (int)j * 4; long sv[(2 * R + 1) * (2 * R + 1)];
                long best = LONG_MAX; int bdx = 0, bdy = 0;
                for (int dy = -R; dy <= R; dy++) for (int dx = -R; dx <= R; dx++) {
                    int sx = cx + dx, sy = cy + dy; long s = LONG_MAX;
                    if (!(sx < 0 || sy < 0 || sx + 4 > ew || sy + 4 > eh)) { s = 0;
                        for (int y = 0; y < 4; y++) for (int x = 0; x < 4; x++) s += abs((int)e1[(cy + y) * ew + cx + x] - (int)e0[(sy + y) * ew + sx + x]); }
                    sv[(dy + R) * (2 * R + 1) + dx + R] = s;
                    if (s < best) { best = s; bdx = dx; bdy = dy; }
                }
                long sec = LONG_MAX; int sdx = 0, sdy = 0;
                for (int dy = -R; dy <= R; dy++) for (int dx = -R; dx <= R; dx++) {
                    if (abs(dx - bdx) <= 1 && abs(dy - bdy) <= 1) continue;
                    long s = sv[(dy + R) * (2 * R + 1) + dx + R]; if (s < sec) { sec = s; sdx = dx; sdy = dy; }
                }
                // 1/4 해상도에서 ±2 다듬기
                int rd[2][2] = {{2 * bdx, 2 * bdy}, {2 * sdx, 2 * sdy}};
                for (int c = 0; c < 2; c++) {
                    long b2 = LONG_MAX; int ox = rd[c][0], oy = rd[c][1];
                    for (int dy = oy - 2; dy <= oy + 2; dy++) for (int dx = ox - 2; dx <= ox + 2; dx++) {
                        int sx = bx + dx, sy = by + dy; if (sx < 0 || sy < 0 || sx + 8 > qw || sy + 8 > qh) continue;
                        long s = 0;
                        for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) s += abs((int)q1[(by + y) * qw + bx + x] - (int)q0[(sy + y) * qw + sx + x]);
                        if (s < b2) { b2 = s; rd[c][0] = dx; rd[c][1] = dy; }
                    }
                }
                // 화소마다 남는 차: layers=1 이면 최선 이동만, 2 이면 min(정지, 최선 이동, 두 번째 이동)
                long m = 0;
                for (int y = 0; y < 8; y++) for (int x = 0; x < 8; x++) {
                    int v = q1[(by + y) * qw + bx + x], e = 255;
                    for (int c = 0; c < (layers == 2 ? 3 : 1); c++) {
                        int dx = c < 2 ? rd[c][0] : 0, dy = c < 2 ? rd[c][1] : 0;
                        int sx = MIN(MAX(bx + x + dx, 0), qw - 1), sy = MIN(MAX(by + y + dy, 0), qh - 1);
                        e = MIN(e, abs(v - (int)q0[sy * qw + sx]));
                    }
                    m += e;
                }
                double M = m / 64.0;
                if (M > MAX(cAbs, cRel * D)) { wgt = 1; back++; }
            }
            tw[j * tx + i] = wgt;
        }
        __sync_fetch_and_add(&nBack, back);
    });
    g->msConf += nowMs() - t0; g->nConf++;
    return (double)nBack / MAX(tx * ty, 1);
}

// out = 보간 결과(BGRA)에 장치를 적용한 결과. hold = 앞 프레임.
static void guardApply(Guard *g, CVPixelBufferRef interp, CVPixelBufferRef prev, CVPixelBufferRef cur, CVPixelBufferRef out, BOOL useStatic, BOOL useConf) {
    int W = g->W, H = g->H, tx = W / 32, ty = H / 32;
    CVPixelBufferLockBaseAddress(interp, kCVPixelBufferLock_ReadOnly); CVPixelBufferLockBaseAddress(prev, kCVPixelBufferLock_ReadOnly);
    CVPixelBufferLockBaseAddress(cur, kCVPixelBufferLock_ReadOnly); CVPixelBufferLockBaseAddress(out, 0);
    const uint8_t *pi = CVPixelBufferGetBaseAddress(interp), *pp = CVPixelBufferGetBaseAddress(prev), *pc = CVPixelBufferGetBaseAddress(cur);
    uint8_t *po = CVPixelBufferGetBaseAddress(out);
    size_t ri = CVPixelBufferGetBytesPerRow(interp), rp = CVPixelBufferGetBytesPerRow(prev), rc = CVPixelBufferGetBytesPerRow(cur), ro = CVPixelBufferGetBytesPerRow(out);
    const uint8_t *prot = g->prot; const float *tw = g->tw;
    dispatch_apply(H, DISPATCH_APPLY_AUTO, ^(size_t y) {
        const uint8_t *a = pi + y * ri, *b = pp + y * rp, *c = pc + y * rc; uint8_t *o = po + y * ro;
        double fy = ((double)y + 0.5) / 32.0 - 0.5; int j0 = (int)floor(fy); double wy = fy - j0;   // 타일 중심 기준 쌍선형 가중치
        int ja = MIN(MAX(j0, 0), ty - 1), jb = MIN(MAX(j0 + 1, 0), ty - 1);
        for (int x = 0; x < W; x++) {
            int idx = 4 * x;
            if (useStatic && prot[y * W + x]) { for (int k = 0; k < 4; k++) o[idx + k] = (uint8_t)((b[idx + k] + c[idx + k] + 1) >> 1); continue; }
            float w = 0;
            if (useConf) {
                double fx = ((double)x + 0.5) / 32.0 - 0.5; int i0 = (int)floor(fx); double wx = fx - i0;
                int ia = MIN(MAX(i0, 0), tx - 1), ib = MIN(MAX(i0 + 1, 0), tx - 1);
                w = (float)((1 - wy) * ((1 - wx) * tw[ja * tx + ia] + wx * tw[ja * tx + ib]) + wy * ((1 - wx) * tw[jb * tx + ia] + wx * tw[jb * tx + ib]));
            }
            if (w <= 0) { memcpy(o + idx, a + idx, 4); continue; }
            for (int k = 0; k < 4; k++) o[idx + k] = (uint8_t)lroundf(a[idx + k] * (1 - w) + b[idx + k] * w);
        }
    });
    CVPixelBufferUnlockBaseAddress(out, 0); CVPixelBufferUnlockBaseAddress(cur, kCVPixelBufferLock_ReadOnly);
    CVPixelBufferUnlockBaseAddress(prev, kCVPixelBufferLock_ReadOnly); CVPixelBufferUnlockBaseAddress(interp, kCVPixelBufferLock_ReadOnly);
}

static void copyBGRA(CVPixelBufferRef src, CVPixelBufferRef dst) {
    CVPixelBufferLockBaseAddress(src, kCVPixelBufferLock_ReadOnly); CVPixelBufferLockBaseAddress(dst, 0);
    size_t rs = CVPixelBufferGetBytesPerRow(src), rd = CVPixelBufferGetBytesPerRow(dst); int H = (int)CVPixelBufferGetHeight(src), W = (int)CVPixelBufferGetWidth(src);
    for (int y = 0; y < H; y++) memcpy((uint8_t *)CVPixelBufferGetBaseAddress(dst) + y * rd, (uint8_t *)CVPixelBufferGetBaseAddress(src) + y * rs, W * 4);
    CVPixelBufferUnlockBaseAddress(dst, 0); CVPixelBufferUnlockBaseAddress(src, kCVPixelBufferLock_ReadOnly);
}

static void writePNG(CVPixelBufferRef pb, NSString *path) {
    CVPixelBufferLockBaseAddress(pb, kCVPixelBufferLock_ReadOnly);
    CGColorSpaceRef cs = CGColorSpaceCreateDeviceRGB();
    CGContextRef ctx = CGBitmapContextCreate(CVPixelBufferGetBaseAddress(pb), CVPixelBufferGetWidth(pb), CVPixelBufferGetHeight(pb), 8,
                                             CVPixelBufferGetBytesPerRow(pb), cs, kCGImageAlphaNoneSkipFirst | kCGBitmapByteOrder32Little);
    CGImageRef img = CGBitmapContextCreateImage(ctx);
    CGImageDestinationRef d = CGImageDestinationCreateWithURL((__bridge CFURLRef)[NSURL fileURLWithPath:path], (__bridge CFStringRef)UTTypePNG.identifier, 1, NULL);
    CGImageDestinationAddImage(d, img, NULL); CGImageDestinationFinalize(d);
    CFRelease(d); CGImageRelease(img); CGContextRelease(ctx); CGColorSpaceRelease(cs);
    CVPixelBufferUnlockBaseAddress(pb, kCVPixelBufferLock_ReadOnly);
}

static int cmpd(const void *a, const void *b) { double x = *(double *)a, y = *(double *)b; return x < y ? -1 : x > y; }
static double pct(double *v, int n, double p) { if (!n) return NAN; qsort(v, n, sizeof(double), cmpd); return v[MIN(n - 1, (int)floor(p * (n - 1) + 0.5))]; }

// ───────────────────────── 실시간 모사 + 마감 서킷브레이커 ─────────────────────────
// 입력 n번째 프레임이 t0 + n·T 에 도착한다고 보고(절대 시간표: 늦어진 처리가 다음 도착을 밀지 않는다),
// 도착 즉시 (이전, 현재) 쌍을 비동기로 보간 요청한다. 동시에 1건만 처리한다(바쁘면 그 쌍은 건너뜀).
// 중간 프레임의 표시 마감 = 도착 + B, B = 1000/(출력fps-입력fps) (표시 지연 H = T/2 + B 모델).
// 브레이커(--breaker on|v1):
//   on(v2, 기본): 창 = 0.5초 분량(60fps 30건, 최소 8건). 창이 차면 평균 > 0.85·B 이거나 마감 초과가 10% 이상일 때,
//       또는 1건이라도 > 2·B 일 때 열림(보간 중지, 원본만 표시). 다시 시도한 직후 2건은 '쉬고 난 첫 요청'이 느린
//       현상(최대 0.2~1.1초) 때문에 > 2·B 규칙에서 뺀다.
//   v1(첫 시험): 창 8건, 평균 > 0.85·B, 마감 초과 2건, 또는 1건 > 2·B. 720p 60→120 정상 상황에서도 열려 버려 바꿨다.
//   공통: 열린 뒤 cooldown(처음 1초, 다시 열릴 때마다 2배, 최대 8초) 동안 요청하지 않고, 지나면 다시 시도한다.
//         10초 연속 정상이면 cooldown 을 1초로 되돌린다.
enum { kRing = 64 };
typedef struct { double ring[kRing]; uint8_t lateRing[kRing]; int n, idx, win, warm; long openUntil; double cooldown; long trips; double okSinceMs; int v1; } Breaker;

static void breakerTrip(Breaker *b, long nowFrame, double fps) {
    b->openUntil = nowFrame + (long)llround(b->cooldown * fps);
    b->cooldown = MIN(b->cooldown * 2, 8.0); b->n = 0; b->idx = 0; b->trips++; b->okSinceMs = 0;
    memset(b->lateRing, 0, sizeof b->lateRing); b->warm = b->v1 ? 0 : 2;
}

// 완료 1건을 반영한다. 열어야 하면 YES.
static BOOL breakerFeed(Breaker *b, double ms, BOOL late, double B) {
    b->ring[b->idx] = ms; b->lateRing[b->idx] = late; b->idx = (b->idx + 1) % b->win; if (b->n < b->win) b->n++;
    double mean = 0; int lates = 0; for (int i = 0; i < b->n; i++) { mean += b->ring[i]; lates += b->lateRing[i]; } mean /= MAX(b->n, 1);
    BOOL warming = b->warm > 0; if (warming) b->warm--;
    if (b->v1) return ms > 2 * B || lates >= 2 || (b->n == b->win && mean > 0.85 * B);
    if (!warming && ms > 2 * B) return YES;
    return b->n == b->win && (mean > 0.85 * B || lates * 10 >= b->win);
}

static int runRealtime(int W, int H, int inFps, double seconds, int breakerMode /* 0 끔, 1 v2, 2 v1 */, double budgetOverride) {
    BOOL useBreaker = breakerMode != 0;
    if (!VTLowLatencyFrameInterpolationConfiguration.isSupported) { printf("lowlat 미지원\n"); return 3; }
    VTLowLatencyFrameInterpolationConfiguration *c = [[VTLowLatencyFrameInterpolationConfiguration alloc] initWithFrameWidth:W frameHeight:H numberOfInterpolatedFrames:1];
    if (!c) { printf("구성 실패\n"); return 3; }
    VTFrameProcessor *proc = [VTFrameProcessor new]; NSError *err = nil;
    if (![proc startSessionWithConfiguration:c error:&err]) { printf("세션 시작 실패: %s\n", err.description.UTF8String); return 4; }
    // 장면을 미리 그려 둔다(측정 중 CoreGraphics 비용이 섞이지 않게). 90장을 돌려 쓴다.
    enum { nPre = 90 }; CVPixelBufferRef pre[nPre];
    VTPixelTransferSessionRef xfer = NULL; VTPixelTransferSessionCreate(NULL, &xfer);
    CVPixelBufferRef tmp = makeBGRA(W, H);
    for (int i = 0; i < nPre; i++) {
        renderScene(tmp, (double)i / inFps);
        pre[i] = makeFromAttrs(c.sourcePixelBufferAttributes, W, H, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        VTPixelTransferSessionTransferImage(xfer, tmp, pre[i]);
    }
    CVPixelBufferRef outB = makeFromAttrs(c.destinationPixelBufferAttributes, W, H, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
    // 준비: 5회 동기 처리(모델 적재·첫 실행을 측정에서 뺀다)
    for (int i = 1; i <= 5; i++) {
        VTFrameProcessorFrame *a = [[VTFrameProcessorFrame alloc] initWithBuffer:pre[i - 1] presentationTimeStamp:CMTimeMake(i - 1, inFps)];
        VTFrameProcessorFrame *b = [[VTFrameProcessorFrame alloc] initWithBuffer:pre[i] presentationTimeStamp:CMTimeMake(i, inFps)];
        VTFrameProcessorFrame *d = [[VTFrameProcessorFrame alloc] initWithBuffer:outB presentationTimeStamp:CMTimeMake(2 * i - 1, 2 * inFps)];
        [proc processWithParameters:[[VTLowLatencyFrameInterpolationParameters alloc] initWithSourceFrame:b previousFrame:a interpolationPhase:@[@0.5] destinationFrames:@[d]] error:nil];
    }
    double T = 1000.0 / inFps, B = budgetOverride > 0 ? budgetOverride : 1000.0 / (2 * inFps - inFps);
    long maxPairs = (long)(seconds * inFps) + 2;
    double *procMs = calloc(maxPairs, sizeof(double)); int8_t *shown = calloc(maxPairs, 1);
    __block long nDone = 0, onTime = 0, late = 0; __block BOOL busy = NO;
    long attempted = 0, busySkip = 0, bypass = 0, pairs = 0;
    __block Breaker br; memset(&br, 0, sizeof br);
    br.openUntil = -1; br.cooldown = 1.0; br.v1 = breakerMode == 2;
    br.win = br.v1 ? 8 : MIN(kRing, MAX(8, (int)lround(0.5 * inFps)));
    NSLock *lk = [NSLock new];
    dispatch_group_t grp = dispatch_group_create();
    double *jit = calloc(maxPairs, sizeof(double)), *subMs = calloc(maxPairs, sizeof(double)); long nSub = 0;
    // 실제 플레이어의 표시 스레드처럼 시간 제약(실시간) 스레드로 돌린다. 기본 스레드는 타이머 여유 때문에
    // mach_wait_until 이 4~5ms 늦게 깨어나 도착 시각이 밀렸다(첫 측정의 arrival_jitter p50 4.5ms).
    pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
    { mach_timebase_info_data_t tb; mach_timebase_info(&tb); double toAbs = 1e6 * tb.denom / tb.numer;
      thread_time_constraint_policy_data_t pol = { (uint32_t)(T * toAbs), (uint32_t)(1.0 * toAbs), (uint32_t)(2.0 * toAbs), 1 };
      kern_return_t kr = thread_policy_set(mach_thread_self(), THREAD_TIME_CONSTRAINT_POLICY, (thread_policy_t)&pol, THREAD_TIME_CONSTRAINT_POLICY_COUNT);
      printf("# 측정 스레드 실시간 정책: %s\n", kr == KERN_SUCCESS ? "적용" : "실패"); }
    double cpu0 = cpuMs(), t0 = nowMs() + 50;
    for (long n = 0; ; n++) {
        double target = t0 + n * T;
        if (target - t0 > seconds * 1000) break;
        { static mach_timebase_info_data_t tb; if (!tb.denom) mach_timebase_info(&tb);
          mach_wait_until((uint64_t)(target * 1e6 * tb.denom / tb.numer)); }
        if (n == 0) continue;
        long k = pairs++; jit[k] = nowMs() - target;
        [lk lock];
        BOOL isBusy = busy, open = useBreaker && n < br.openUntil;
        if (!isBusy && !open) busy = YES;
        [lk unlock];
        if (isBusy) { busySkip++; continue; }
        if (open) { bypass++; continue; }
        attempted++;
        CVPixelBufferRef a = pre[(n - 1) % nPre], b = pre[n % nPre];
        VTFrameProcessorFrame *fa = [[VTFrameProcessorFrame alloc] initWithBuffer:a presentationTimeStamp:CMTimeMake(n - 1, inFps)];
        VTFrameProcessorFrame *fb = [[VTFrameProcessorFrame alloc] initWithBuffer:b presentationTimeStamp:CMTimeMake(n, inFps)];
        VTFrameProcessorFrame *fd = [[VTFrameProcessorFrame alloc] initWithBuffer:outB presentationTimeStamp:CMTimeMake(2 * n - 1, 2 * inFps)];
        double submit = nowMs(), deadline = target + B; long frameNo = n;
        dispatch_group_enter(grp);
        [proc processWithParameters:[[VTLowLatencyFrameInterpolationParameters alloc] initWithSourceFrame:fb previousFrame:fa interpolationPhase:@[@0.5] destinationFrames:@[fd]]
                  completionHandler:^(id<VTFrameProcessorParameters> p, NSError *e) {
            double done = nowMs(), ms = done - submit;
            [lk lock];
            procMs[nDone++] = ms;
            BOOL ok = !e && done <= deadline;
            if (ok) { onTime++; shown[k] = 1; } else late++;
            if (useBreaker) {
                if (breakerFeed(&br, ms, !ok, B)) breakerTrip(&br, frameNo, inFps);
                else if (br.n == br.win) { if (!br.okSinceMs) br.okSinceMs = done; else if (done - br.okSinceMs > 10000) br.cooldown = 1.0; }
            }
            busy = NO;
            [lk unlock];
            dispatch_group_leave(grp);
        }];
        subMs[nSub++] = nowMs() - submit;   // 비동기 제출 호출 자체가 막는 시간
    }
    dispatch_group_wait(grp, DISPATCH_TIME_FOREVER);
    double wall = nowMs() - t0, cpu = cpuMs() - cpu0;
    long toggles = 0; for (long i = 1; i < pairs; i++) if (shown[i] != shown[i - 1]) toggles++;
    printf("# realtime %dx%d %d→%dfps, %.0fs, budget B=%.1fms, breaker=%s\n", W, H, inFps, 2 * inFps, seconds, B,
           breakerMode == 1 ? "on(v2)" : breakerMode == 2 ? "v1" : "off");
    printf("pairs=%ld attempted=%ld on_time=%ld late=%ld busy_skip=%ld bypass=%ld trips=%ld\n", pairs, attempted, onTime, late, busySkip, bypass, br.trips);
    printf("coverage(제때 보간된 쌍)=%.1f%%  wasted(늦어 버린 요청)=%.1f%%  toggles_per_min=%.1f\n",
           100.0 * onTime / MAX(pairs, 1), 100.0 * late / MAX(attempted, 1), toggles * 60000.0 / wall);
    if (nDone) printf("proc_ms p50=%.2f p95=%.2f p99=%.2f max=%.2f  arrival_jitter_p95=%.2fms\n", pct(procMs, (int)nDone, 0.5), pct(procMs, (int)nDone, 0.95),
                      pct(procMs, (int)nDone, 0.99), pct(procMs, (int)nDone, 1.0), pct(jit, (int)pairs, 0.95));
    if (nSub) printf("submit_call_ms p50=%.2f p95=%.2f max=%.2f  arrival_jitter_p50=%.2fms\n", pct(subMs, (int)nSub, 0.5), pct(subMs, (int)nSub, 0.95), pct(subMs, (int)nSub, 1.0), pct(jit, (int)pairs, 0.5));
    printf("cpu_pct_of_one_core=%.0f%%\n", 100 * cpu / wall);
    [proc endSession];
    return 0;
}

// ───────────────────────── 본체 ─────────────────────────
int main(int argc, const char **argv) {
    @autoreleasepool {
        NSString *source = @"synth", *algo = @"lowlat", *dump = nil;
        int W = 1920, H = 1080, frames = 120, inFps = 30, interp = 1, maxWidth = 0, PW = 0, PH = 0, inflight = 0;
        double loopSec = 0, rtSec = 0, budgetMs = 0; int cuts = 0, breakerMode = 0; BOOL guards = NO;
        NSMutableDictionary *gopt = [NSMutableDictionary new];   // 가드 문턱 덮어쓰기
        for (int i = 1; i < argc; i++) {
            NSString *k = @(argv[i]); NSString *v = i + 1 < argc ? @(argv[i + 1]) : @"";
            if ([k isEqual:@"--source"]) { source = v; i++; }
            else if ([k isEqual:@"--algo"]) { algo = v; i++; }
            else if ([k isEqual:@"--size"]) { NSArray *p = [v componentsSeparatedByString:@"x"]; W = [p[0] intValue]; H = [p[1] intValue]; i++; }
            else if ([k isEqual:@"--frames"]) { frames = v.intValue; i++; }
            else if ([k isEqual:@"--in-fps"]) { inFps = v.intValue; i++; }
            else if ([k isEqual:@"--interp"]) { interp = v.intValue; i++; }      // 입력 프레임 사이에 넣을 프레임 수(1 → 2배, 3 → 4배)
            else if ([k isEqual:@"--loop"]) { loopSec = v.doubleValue; i++; }    // 실시간 속도로 반복(사용률 측정용)
            else if ([k isEqual:@"--max-width"]) { maxWidth = v.intValue; i++; }
            else if ([k isEqual:@"--dump"]) { dump = v; i++; }
            else if ([k isEqual:@"--proc-size"]) { NSArray *p = [v componentsSeparatedByString:@"x"]; PW = [p[0] intValue]; PH = [p[1] intValue]; i++; }  // 줄여서 보간 후 다시 키움
            else if ([k isEqual:@"--inflight"]) { inflight = v.intValue; i++; }  // 비동기 동시 요청 수(처리량 측정, 화질 계산 생략)
            else if ([k isEqual:@"--cuts"]) { cuts = v.intValue; i++; }          // 합성 장면: 입력 N장마다 장면 전환
            else if ([k isEqual:@"--guards"]) { guards = YES; }                  // 보호 장치(cut·static·conf) 화질 비교
            else if ([k isEqual:@"--rt"]) { rtSec = v.doubleValue; i++; }        // 실시간 모사(절대 시간표, 비동기 1건) 초
            else if ([k isEqual:@"--breaker"]) { breakerMode = [v isEqual:@"on"] ? 1 : [v isEqual:@"v1"] ? 2 : 0; i++; }
            else if ([k isEqual:@"--budget-ms"]) { budgetMs = v.doubleValue; i++; }
            else if ([k hasPrefix:@"--g-"]) { gopt[[k substringFromIndex:4]] = @(v.doubleValue); i++; }  // --g-cut-sad --g-cut-hist --g-static-k --g-static-tau --g-static-erode --g-conf-abs --g-conf-rel
        }
        if (rtSec > 0) return runRealtime(W, H, inFps, rtSec, breakerMode, budgetMs);
        BOOL useLowLat = [algo isEqual:@"lowlat"], useFrc = [algo hasPrefix:@"frc"];
        BOOL synth = [source isEqual:@"synth"];
        FileSource *fs = nil;
        double gtFps;  // 정답 프레임 열의 속도
        if (synth) gtFps = inFps * (interp + 1);
        else { fs = [[FileSource alloc] initWithPath:source maxWidth:maxWidth]; W = fs.w; H = fs.h; gtFps = fs.fps; }
        int stride = interp + 1;  // 정답 열에서 입력으로 쓸 간격
        if (!synth) inFps = (int)lround(gtFps / stride);
        if (!PW) { PW = W; PH = H; }
        printf("# proc=%dx%d inflight=%d\n", PW, PH, inflight);
        printf("# source=%s algo=%s size=%dx%d in=%dfps out=%dfps interp=%d\n", source.lastPathComponent.UTF8String, algo.UTF8String, W, H, inFps, inFps * stride, interp);

        // 처리기 준비
        VTFrameProcessor *proc = nil; id<VTFrameProcessorConfiguration> cfg = nil;
        NSDictionary *srcAttrs = nil, *dstAttrs = nil;
        double startMs = 0;
        if (useLowLat || useFrc) {
            if (useLowLat) {
                if (!VTLowLatencyFrameInterpolationConfiguration.isSupported) { printf("lowlat: 이 기기에서 지원 안 됨\n"); return 3; }
                // 1개 → 0.5, 2개 → 0.25/0.5/0.75 지점 사용 가능(헤더 설명). 3배 이상은 2로 요청.
                NSInteger n = interp == 1 ? 1 : 2;
                VTLowLatencyFrameInterpolationConfiguration *c = [[VTLowLatencyFrameInterpolationConfiguration alloc] initWithFrameWidth:PW frameHeight:PH numberOfInterpolatedFrames:n];
                if (!c) { printf("lowlat: %dx%d 구성 실패(크기 한도 초과?)\n", PW, PH); return 3; }
                if (@available(macOS 27.0, *)) {
                    printf("# lowlat 최대 한 변 %ld, 최대 화소 %ld\n", (long)[VTLowLatencyFrameInterpolationConfiguration maximumDimensionForSpatialScaleFactor:1],
                           (long)[VTLowLatencyFrameInterpolationConfiguration maximumPixelCountForSpatialScaleFactor:1]);
                }
                cfg = c; srcAttrs = c.sourcePixelBufferAttributes; dstAttrs = c.destinationPixelBufferAttributes;
                printf("# 지원 화소 형식:"); for (NSNumber *f in c.frameSupportedPixelFormats) { OSType o = f.unsignedIntValue; printf(" %.4s", (char *)&o); } printf("\n");
            } else {
                if (!VTFrameRateConversionConfiguration.isSupported) { printf("frc: 지원 안 됨\n"); return 3; }
                VTFrameRateConversionConfiguration *c = [[VTFrameRateConversionConfiguration alloc] initWithFrameWidth:PW frameHeight:PH usePrecomputedFlow:NO
                    qualityPrioritization:[algo isEqual:@"frcq"] ? VTFrameRateConversionConfigurationQualityPrioritizationQuality : VTFrameRateConversionConfigurationQualityPrioritizationNormal
                    revision:VTFrameRateConversionConfiguration.defaultRevision];
                cfg = c; srcAttrs = c.sourcePixelBufferAttributes; dstAttrs = c.destinationPixelBufferAttributes;
                printf("# 지원 화소 형식:"); for (NSNumber *f in c.frameSupportedPixelFormats) { OSType o = f.unsignedIntValue; printf(" %.4s", (char *)&o); } printf("\n");
            }
            proc = [VTFrameProcessor new];
            NSError *err = nil; double t0 = nowMs();
            if (![proc startSessionWithConfiguration:cfg error:&err]) { printf("세션 시작 실패: %s\n", err.description.UTF8String); return 4; }
            startMs = nowMs() - t0;
            printf("startSession_ms=%.1f\n", startMs);
        }
        VTPixelTransferSessionRef toSrc = NULL, toBGRA = NULL;
        VTPixelTransferSessionCreate(NULL, &toSrc); VTPixelTransferSessionCreate(NULL, &toBGRA);

        int nIn = frames;  // 입력 프레임 수
        double *procMs = calloc(nIn * stride, sizeof(double)); int nProc = 0;
        double *xferMs = calloc(nIn * stride, sizeof(double)); int nXfer = 0;
        double *blendMs = calloc(nIn * stride, sizeof(double)); int nBlend = 0;
        Regions R = sceneRegions(W, H);
        CGRect full = CGRectMake(0, 0, W, H);
        CGRect regs[5] = { full, R.chat, R.subtitle, R.ball, R.timer };
        const char *regName[5] = { "전체", "채팅", "자막", "빠른공", "타이머" };
        int nReg = synth ? 5 : 1;
        double sumI[5] = {0}, sumH[5] = {0}, sumB[5] = {0}; int nQ = 0;
        double minI = 99;
        // 보호 장치 비교(--guards): 보간 결과에 cut / static / conf / 전부를 씌운 네 가지
        Guard *G = (guards && interp == 1) ? guardNew(W, H) : NULL;
        if (G) {
            if (gopt[@"cut-sad"]) G->cutSad = [gopt[@"cut-sad"] doubleValue];
            if (gopt[@"cut-hist"]) G->cutHist = [gopt[@"cut-hist"] doubleValue];
            // 정지 판정 길이는 시간으로 둔다(기본 120ms → 30fps 4장, 60fps 7장). --g-static-k 로 장 수를 직접 줄 수 있다.
            G->staticK = MAX(2, (int)lround((gopt[@"static-ms"] ? [gopt[@"static-ms"] doubleValue] : 120.0) * inFps / 1000.0));
            if (gopt[@"static-k"]) G->staticK = [gopt[@"static-k"] intValue];
            if (gopt[@"static-tau"]) G->staticTau = [gopt[@"static-tau"] intValue];
            if (gopt[@"static-erode"]) G->staticErode = [gopt[@"static-erode"] intValue];
            if (gopt[@"conf-abs"]) G->confAbs = [gopt[@"conf-abs"] doubleValue];
            if (gopt[@"conf-rel"]) G->confRel = [gopt[@"conf-rel"] doubleValue];
            if (gopt[@"conf-layers"]) G->confLayers = [gopt[@"conf-layers"] intValue];
            printf("# guard cut_sad=%.2f cut_hist=%.2f static_tau=%d static_k=%d static_erode=%d conf_abs=%.1f conf_rel=%.2f conf_layers=%d\n",
                   G->cutSad, G->cutHist, G->staticTau, G->staticK, G->staticErode, G->confAbs, G->confRel, G->confLayers);
        }
        CVPixelBufferRef gBuf = G ? makeBGRA(W, H) : NULL;
        const char *gName[5] = { "+cut", "+static", "+conf", "+전부", "+cut+static" };
        double sumG[5][5] = {{0}}; int nG = 0; double confFracSum = 0;
        int tp = 0, fp = 0, fn = 0, tn = 0; double maxSadN = 0, maxHistN = 0, minSadC = 9, minHistC = 9;
        double cutP[5] = {0}; int nCutP = 0;   // 실제 장면 전환 쌍의 전체 PSNR: 그대로 / 섞기 / 보간 / 보간+전부 / 보간+static·conf(cut 없음)

        CVPixelBufferRef prevBGRA = NULL, prevSrc = NULL;
        CVPixelBufferRef gtMid[8] = {0};
        CVPixelBufferRef outBuf[8] = {0}, outBGRA = makeBGRA(W, H), blendBuf = makeBGRA(W, H);
        for (int j = 0; j < interp; j++) outBuf[j] = makeFromAttrs(dstAttrs, PW, PH, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
        dispatch_semaphore_t slots = dispatch_semaphore_create(MAX(inflight, 1));
        dispatch_group_t inflightGroup = dispatch_group_create();
        __block double asyncDoneMs = 0;
        double cpu0 = cpuMs(), wall0 = nowMs();
        int deadlineMiss = 0, loopIters = 0;
        double interval = 1000.0 / inFps;
        BOOL looping = loopSec > 0;
        int gtIndex = 0;
        for (int fi = 0; fi < nIn || looping; fi++) {
            if (looping && nowMs() - wall0 > loopSec * 1000) break;
            double tickStart = nowMs();
            // 정답 열에서 stride 칸 진행: 중간 프레임(정답)들 + 다음 입력 프레임
            CVPixelBufferRef curBGRA = NULL;
            if (fi > 0) for (int j = 0; j < interp; j++) {
                if (gtMid[j]) CVPixelBufferRelease(gtMid[j]);
                if (synth) { gtMid[j] = makeBGRA(W, H); gVariant = cuts ? (gtIndex + j + 1) / (cuts * stride) : 0; renderScene(gtMid[j], (double)(gtIndex + j + 1) / gtFps); }
                else { gtMid[j] = [fs next]; if (!gtMid[j]) break; }
            }
            if (fi > 0) gtIndex += stride;
            if (synth) { curBGRA = makeBGRA(W, H); gVariant = cuts ? gtIndex / (cuts * stride) : 0; renderScene(curBGRA, (double)gtIndex / gtFps); }
            else {
                curBGRA = [fs next];
                if (!curBGRA) {
                    if (!looping) break;
                    fs = [[FileSource alloc] initWithPath:source maxWidth:maxWidth];  // 처음부터 다시
                    if (prevBGRA) { CVPixelBufferRelease(prevBGRA); prevBGRA = NULL; }
                    if (prevSrc) { CVPixelBufferRelease(prevSrc); prevSrc = NULL; }
                    gtIndex = 0; fi = -1; continue;
                }
            }
            if (G) guardPush(G, curBGRA, prevBGRA == NULL);
            CVPixelBufferRef curSrc = curBGRA;
            if (proc) {
                curSrc = makeFromAttrs(srcAttrs, PW, PH, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);
                double tx = nowMs(); VTPixelTransferSessionTransferImage(toSrc, curBGRA, curSrc); xferMs[nXfer++ % (nIn * stride)] = nowMs() - tx;
            } else CVPixelBufferRetain(curSrc);

            if (prevBGRA) {
                NSMutableArray *phases = [NSMutableArray new], *dst = [NSMutableArray new];
                CMTime tPrev = CMTimeMake(fi - 1, inFps), tCur = CMTimeMake(fi, inFps);
                for (int j = 0; j < interp; j++) {
                    double p = (double)(j + 1) / stride;
                    [phases addObject:@(p)];
                    CVPixelBufferRef ob = outBuf[j];
                    if (inflight > 0) ob = makeFromAttrs(dstAttrs, PW, PH, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange);  // 요청마다 새 출력
                    [dst addObject:[[VTFrameProcessorFrame alloc] initWithBuffer:ob presentationTimeStamp:CMTimeMake((fi - 1) * stride + j + 1, inFps * stride)]];
                    if (inflight > 0) CVPixelBufferRelease(ob);
                }
                VTFrameProcessorFrame *fPrev = [[VTFrameProcessorFrame alloc] initWithBuffer:prevSrc presentationTimeStamp:tPrev];
                VTFrameProcessorFrame *fCur = [[VTFrameProcessorFrame alloc] initWithBuffer:curSrc presentationTimeStamp:tCur];
                if (proc) {
                    id<VTFrameProcessorParameters> params;
                    if (useLowLat) params = [[VTLowLatencyFrameInterpolationParameters alloc] initWithSourceFrame:fCur previousFrame:fPrev interpolationPhase:phases destinationFrames:dst];
                    else params = [[VTFrameRateConversionParameters alloc] initWithSourceFrame:fPrev nextFrame:fCur opticalFlow:nil interpolationPhase:phases
                                     submissionMode:(fi == 1 ? VTFrameRateConversionParametersSubmissionModeRandom : VTFrameRateConversionParametersSubmissionModeSequential) destinationFrames:dst];
                    if (!params) { printf("parameters 생성 실패\n"); return 5; }
                    if (inflight > 0) {
                        dispatch_semaphore_wait(slots, DISPATCH_TIME_FOREVER);
                        dispatch_group_enter(inflightGroup);
                        double tp = nowMs(); int slot = nProc++ % (nIn * stride);
                        [proc processWithParameters:params completionHandler:^(id<VTFrameProcessorParameters> pp, NSError *e) {
                            if (e) printf("process 실패: %s\n", e.description.UTF8String);
                            procMs[slot] = nowMs() - tp; asyncDoneMs = nowMs();
                            dispatch_semaphore_signal(slots); dispatch_group_leave(inflightGroup);
                        }];
                    } else {
                    NSError *err = nil; double tp = nowMs();
                    if (![proc processWithParameters:params error:&err]) { printf("process 실패: %s\n", err.description.UTF8String); return 5; }
                    procMs[nProc++ % (nIn * stride)] = nowMs() - tp;
                    }
                }
                // 품질: 첫 반복(loop 아님)일 때만
                if (!looping && gtMid[0] && inflight == 0) {
                    for (int j = 0; j < interp && gtMid[j]; j++) {
                        double p = (double)(j + 1) / stride;
                        double tb = nowMs(); blendBGRA(prevBGRA, curBGRA, blendBuf, p); blendMs[nBlend++] = nowMs() - tb;
                        if (proc) VTPixelTransferSessionTransferImage(toBGRA, outBuf[j], outBGRA);
                        for (int r = 0; r < nReg; r++) {
                            double pi = proc ? psnrY(outBGRA, gtMid[j], regs[r]) : NAN;
                            sumI[r] += pi;
                            sumH[r] += psnrY(p < 0.5 ? prevBGRA : (p > 0.5 ? curBGRA : prevBGRA), gtMid[j], regs[r]);
                            sumB[r] += psnrY(blendBuf, gtMid[j], regs[r]);
                            if (r == 0 && pi < minI) minI = pi;
                        }
                        nQ++;
                        if (G && proc && j == 0) {
                            double sad, hd; BOOL isCut = guardIsCut(G, &sad, &hd);
                            BOOL truth = cuts > 0 && gtIndex % (cuts * stride) == 0;
                            if (truth) { if (isCut) tp++; else fn++; minSadC = MIN(minSadC, sad); minHistC = MIN(minHistC, hd); }
                            else { if (isCut) fp++; else tn++; maxSadN = MAX(maxSadN, sad); maxHistN = MAX(maxHistN, hd); }
                            confFracSum += guardConf(G);
                            for (int v = 0; v < 5; v++) {
                                if (v == 0) copyBGRA(isCut ? prevBGRA : outBGRA, gBuf);
                                else if (isCut && (v == 3 || v == 4)) copyBGRA(prevBGRA, gBuf);
                                else if (v == 4) guardApply(G, outBGRA, prevBGRA, curBGRA, gBuf, YES, NO);
                                else guardApply(G, outBGRA, prevBGRA, curBGRA, gBuf, v == 1 || v == 3, v == 2 || v == 3);
                                for (int r = 0; r < nReg; r++) sumG[v][r] += psnrY(gBuf, gtMid[j], regs[r]);
                                if (truth && v == 3) cutP[3] += psnrY(gBuf, gtMid[j], full);
                                if (dump && fi == 20 && v == 3) {
                                    NSString *tag = [NSString stringWithFormat:@"%@_%@_%dx%d_%dto%d", synth ? @"synth" : @"file", algo, W, H, inFps, inFps * stride];
                                    [[NSFileManager defaultManager] createDirectoryAtPath:dump withIntermediateDirectories:YES attributes:nil error:nil];
                                    writePNG(gBuf, [dump stringByAppendingPathComponent:[tag stringByAppendingString:@"_guarded.png"]]);
                                }
                            }
                            if (truth) {
                                cutP[0] += psnrY(prevBGRA, gtMid[j], full); cutP[1] += psnrY(blendBuf, gtMid[j], full); cutP[2] += psnrY(outBGRA, gtMid[j], full);
                                guardApply(G, outBGRA, prevBGRA, curBGRA, gBuf, YES, YES); cutP[4] += psnrY(gBuf, gtMid[j], full);
                                nCutP++;
                            }
                            nG++;
                        }
                        // 자료용 그림: 장면에 빠른 공·채팅이 다 보이는 프레임 하나
                        if (dump && fi == 20 && j == 0) {
                            [[NSFileManager defaultManager] createDirectoryAtPath:dump withIntermediateDirectories:YES attributes:nil error:nil];
                            NSString *tag = [NSString stringWithFormat:@"%@_%@_%dx%d_%dto%d", synth ? @"synth" : @"file", algo, W, H, inFps, inFps * stride];
                            writePNG(gtMid[j], [dump stringByAppendingPathComponent:[tag stringByAppendingString:@"_truth.png"]]);
                            writePNG(blendBuf, [dump stringByAppendingPathComponent:[tag stringByAppendingString:@"_blend.png"]]);
                            writePNG(prevBGRA, [dump stringByAppendingPathComponent:[tag stringByAppendingString:@"_prev.png"]]);
                            if (proc) writePNG(outBGRA, [dump stringByAppendingPathComponent:[tag stringByAppendingString:@"_interp.png"]]);
                        }
                    }
                }
            }
            if (prevBGRA) CVPixelBufferRelease(prevBGRA);
            if (prevSrc) CVPixelBufferRelease(prevSrc);
            prevBGRA = curBGRA; prevSrc = curSrc;
            loopIters++;
            if (looping) {
                double spent = nowMs() - tickStart;
                if (spent > interval) deadlineMiss++;
                else usleep((useconds_t)((interval - spent) * 1000));
            }
        }
        dispatch_group_wait(inflightGroup, DISPATCH_TIME_FOREVER);
        double wall = nowMs() - wall0, cpu = cpuMs() - cpu0;
        if (inflight > 0) printf("async: %d calls in %.0f ms → 처리량 %.1f 보간/s (요청당 지연 p50 %.1f ms)\n", nProc, asyncDoneMs - wall0,
                                 nProc * 1000.0 / (asyncDoneMs - wall0), pct(procMs, MIN(nProc, nIn * stride), 0.5));
        int np = MIN(nProc, nIn * stride);
        double meanP = 0; for (int i = 0; i < np; i++) meanP += procMs[i]; meanP /= MAX(np, 1);
        printf("frames_in=%d calls=%d proc_ms mean=%.2f p50=%.2f p95=%.2f max=%.2f\n", loopIters, nProc, meanP,
               pct(procMs, np, 0.5), pct(procMs, np, 0.95), pct(procMs, np, 1.0));
        int nx = MIN(nXfer, nIn * stride);
        if (nx) printf("bgra_to_src_xfer_ms p50=%.2f\n", pct(xferMs, nx, 0.5));
        if (nBlend) printf("cpu_blend_ms p50=%.2f\n", pct(blendMs, nBlend, 0.5));
        printf("wall_s=%.1f cpu_s=%.1f cpu_pct_of_one_core=%.0f%%\n", wall / 1000, cpu / 1000, 100 * cpu / wall);
        if (looping) printf("loop: in_fps=%d deadline_miss=%d/%d (%.1f%%)\n", inFps, deadlineMiss, loopIters, 100.0 * deadlineMiss / MAX(loopIters, 1));
        if (nQ) {
            printf("PSNR_dB(휘도, 정답 대비, %d장 평균)  영역: 보간 / 그대로두기 / 단순섞기\n", nQ);
            for (int r = 0; r < nReg; r++) printf("  %-6s %6.2f / %6.2f / %6.2f\n", regName[r], sumI[r] / nQ, sumH[r] / nQ, sumB[r] / nQ);
            if (proc) printf("  전체 최저(보간) %.2f\n", minI);
        }
        if (G && nG) {
            printf("GUARD PSNR_dB(%d장 평균)  영역: %s / %s / %s / %s / %s\n", nG, gName[0], gName[1], gName[2], gName[3], gName[4]);
            for (int r = 0; r < nReg; r++) printf("  %-6s %6.2f / %6.2f / %6.2f / %6.2f / %6.2f\n", regName[r], sumG[0][r] / nG, sumG[1][r] / nG, sumG[2][r] / nG, sumG[3][r] / nG, sumG[4][r] / nG);
            printf("guard_cut: TP=%d FP=%d FN=%d TN=%d  (문턱 SAD≥%.2f 그리고 hist≥%.2f)  정상쌍 최대 SAD %.3f hist %.3f / 전환쌍 최소 SAD %.3f hist %.3f\n",
                   tp, fp, fn, tn, G->cutSad, G->cutHist, maxSadN, maxHistN, nCutP ? minSadC : NAN, nCutP ? minHistC : NAN);
            if (nCutP) printf("guard_cut_pairs(%d쌍, 전체 PSNR): 그대로 %.2f / 섞기 %.2f / 보간 %.2f / 보간+전부 %.2f / 보간+static·conf %.2f\n", nCutP,
                              cutP[0] / nCutP, cutP[1] / nCutP, cutP[2] / nCutP, cutP[3] / nCutP, cutP[4] / nCutP);
            printf("guard_conf: 되돌린 타일 평균 %.1f%%\n", 100 * confFracSum / nG);
            printf("guard_cpu_ms(10코어 병렬, 1080p 기준 아님 — 이 해상도): 휘도+축소·히스토그램 %.2f / 정지 마스크 %.2f / conf 탐색 %.2f (입력 1장당 평균)\n",
                   G->msThumb / MAX(G->nCalls, 1), G->msStatic / MAX(G->nCalls, 1), G->msConf / MAX(G->nConf, 1));
        }
        if (proc) [proc endSession];
        return 0;
    }
}

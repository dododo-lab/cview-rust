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

// y 는 위에서부터(화면 좌표). CG 는 아래에서부터라 뒤집는다.
static void renderScene(CVPixelBufferRef pb, double t) {
    CVPixelBufferLockBaseAddress(pb, 0);
    int W = (int)CVPixelBufferGetWidth(pb), H = (int)CVPixelBufferGetHeight(pb);
    CGColorSpaceRef cs = CGColorSpaceCreateDeviceRGB();
    CGContextRef ctx = CGBitmapContextCreate(CVPixelBufferGetBaseAddress(pb), W, H, 8, CVPixelBufferGetBytesPerRow(pb), cs,
                                             kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
    CGContextTranslateCTM(ctx, 0, H); CGContextScaleCTM(ctx, 1, -1);  // 위→아래 좌표
    double s = W / 1920.0;

    // 1) 가로로 흘러가는 배경(카메라 팬): 1200 px/s
    double pan = fmod(t * 1200.0 * s, 64.0 * 40 * s);
    for (int i = -1; i < 40; i++) {
        double x = i * 64.0 * s - pan + 64.0 * 40 * s * (i < 0);
        for (int k = 0; k < 2; k++) {
            double xx = x + k * 64.0 * 40 * s;
            unsigned hsh = (unsigned)((i + 41) % 40) * 2654435761u;
            CGFloat c1 = ((hsh >> 8) & 255) / 255.0, c2 = ((hsh >> 16) & 255) / 255.0;
            CGContextSetRGBFillColor(ctx, 0.15 + 0.5 * c1, 0.2 + 0.4 * c2, 0.35 + 0.3 * (1 - c1), 1);
            CGContextFillRect(ctx, CGRectMake(xx, 0, 64.0 * s + 1, H));
            CGContextSetRGBFillColor(ctx, 1, 1, 1, 0.25);
            for (int y = 0; y < H; y += (int)(48 * s)) CGContextFillRect(ctx, CGRectMake(xx + 8 * s, y, 4 * s, 24 * s));
        }
    }
    // 2) 회전 막대(물체 운동): 180°/s
    CGContextSaveGState(ctx);
    CGContextTranslateCTM(ctx, W * 0.30, H * 0.24);
    CGContextRotateCTM(ctx, t * M_PI);
    CGContextSetRGBFillColor(ctx, 0.95, 0.75, 0.1, 1);
    CGContextFillRect(ctx, CGRectMake(-260 * s, -22 * s, 520 * s, 44 * s));
    CGContextRestoreGState(ctx);
    // 3) 빠른 공: 2400 px/s 왕복
    Regions R = sceneRegions(W, H);
    double span = R.ball.size.width - 120 * s;
    double ph = fmod(t * 2400.0 * s, 2 * span);
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

// ───────────────────────── 본체 ─────────────────────────
int main(int argc, const char **argv) {
    @autoreleasepool {
        NSString *source = @"synth", *algo = @"lowlat", *dump = nil;
        int W = 1920, H = 1080, frames = 120, inFps = 30, interp = 1, maxWidth = 0, PW = 0, PH = 0, inflight = 0;
        double loopSec = 0;
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
        }
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
                if (synth) { gtMid[j] = makeBGRA(W, H); renderScene(gtMid[j], (double)(gtIndex + j + 1) / gtFps); }
                else { gtMid[j] = [fs next]; if (!gtMid[j]) break; }
            }
            if (fi > 0) gtIndex += stride;
            if (synth) { curBGRA = makeBGRA(W, H); renderScene(curBGRA, (double)gtIndex / gtFps); }
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
        if (proc) [proc endSession];
        return 0;
    }
}

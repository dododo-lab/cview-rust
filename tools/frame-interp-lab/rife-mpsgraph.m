// RIFE IFNet(Practical-RIFE 4.25 / 4.25.lite) 구조를 MPSGraph(Metal GPU)로 옮긴 "시간 측정용 복제본".
//
// 가중치는 무작위다. 학습된 가중치(Google Drive 배포, MIT)는 받지 않았다. 신경망의 처리 시간은
// 가중치 값과 무관하고 층 구조·텐서 크기·정밀도로 정해지므로, 이 도구는 "이 Mac GPU에서 1080p RIFE가
// 몇 ms 걸리는가"만 답한다. 화질(PSNR)은 재지 않는다(출력은 의미 없는 영상).
//
// 구조 출처: TNTwise/rife-ncnn-vulkan 의 models/rife-v4.25{,-lite}/flownet.param (MIT)에서 읽은 층 구성.
//   Head(영상 1장마다, 가중치 공유): conv 3→16 s2, conv 16→16, conv 16→16, deconv 16→4 k4 s2 (LeakyReLU 0.2)
//   IFBlock: (입력을 1/scale 로 축소) conv0 = conv in→c/2 s2, conv c/2→c s2 → ResConv(c) × 8
//            → deconv c→52 k4 s2 → PixelShuffle(2) = 13ch(흐름 4 + 마스크 1 + 특징 8) → ×scale 확대
//   4.25     : c = 192/128/96/64/32, 축소 비율 16/8/4/2/1
//   4.25.lite: c = 192/128/96/64/24, 축소 비율 32/16/8/4/1
//   블록 0 입력 15ch(img0 3 + img1 3 + f0 4 + f1 4 + t 1), 블록 1~4 입력 24ch + 흐름 4ch.
//   블록마다 전체 해상도 역방향 워핑(grid_sample) 4회: f0·f1(블록 입력용), img0·img1(블록 출력 뒤).
//
// 빌드·실행은 run.sh 의 'RIFE MPSGraph' 절 또는:
//   clang -O2 -fobjc-arc -framework Foundation -framework Metal -framework MetalPerformanceShaders \
//     -framework MetalPerformanceShadersGraph rife-mpsgraph.m -o rife-mpsgraph
//   ./rife-mpsgraph --size 1920x1080 --variant 4.25 --scale 1 --dtype f16 --iters 100

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <MetalPerformanceShaders/MetalPerformanceShaders.h>
#import <MetalPerformanceShadersGraph/MetalPerformanceShadersGraph.h>
#include <mach/mach_time.h>
#include <math.h>

static double nowMs(void) {
    static mach_timebase_info_data_t tb;
    if (tb.denom == 0) mach_timebase_info(&tb);
    return (double)mach_absolute_time() * tb.numer / tb.denom / 1e6;
}
static int cmpd(const void *a, const void *b) { double x = *(double *)a, y = *(double *)b; return x < y ? -1 : x > y; }
static double pct(double *v, int n, double p) { qsort(v, n, sizeof(double), cmpd); return v[MIN(n - 1, (int)floor(p * (n - 1) + 0.5))]; }

static BOOL gSkipWarp = NO;      // 분석용: 워핑을 항등으로 바꿔 합성곱만의 시간을 본다
static BOOL gSkipDeconv = NO;    // 분석용: 전치 합성곱을 최근접 확대+1×1 합성곱으로 바꾼다
static uint64_t gRng = 0x9E3779B97F4A7C15ULL;
static float frand(void) { gRng = gRng * 6364136223846793005ULL + 1442695040888963407ULL; return ((gRng >> 40) & 0xFFFFFF) / (float)0x1000000 * 2.f - 1.f; }

typedef struct {
    MPSGraph *g;
    MPSDataType dt;       // 합성곱 정밀도
    BOOL warp32;          // 워핑만 FP32(혼합 정밀도)
    double macs;          // 곱셈-누산 수(합성곱·전치 합성곱만)
    long params;
} Net;

static MPSGraphTensor *constRand(Net *n, NSArray<NSNumber *> *shape, float scale) {
    long count = 1; for (NSNumber *s in shape) count *= s.longValue;
    NSMutableData *d;
    if (n->dt == MPSDataTypeFloat16) {
        d = [NSMutableData dataWithLength:count * 2]; _Float16 *p = d.mutableBytes;
        for (long i = 0; i < count; i++) p[i] = (_Float16)(frand() * scale);
    } else {
        d = [NSMutableData dataWithLength:count * 4]; float *p = d.mutableBytes;
        for (long i = 0; i < count; i++) p[i] = frand() * scale;
    }
    return [n->g constantWithData:d shape:shape dataType:n->dt];
}

static NSArray<NSNumber *> *shapeOf(MPSGraphTensor *t) { return t.shape; }
static int dimH(MPSGraphTensor *t) { return shapeOf(t)[1].intValue; }
static int dimW(MPSGraphTensor *t) { return shapeOf(t)[2].intValue; }

typedef struct { MPSGraphTensor *w, *b; int cin, cout, k, s; } ConvW;

static ConvW makeConv(Net *n, int cin, int cout, int k, int s) {
    ConvW c = { constRand(n, @[@(k), @(k), @(cin), @(cout)], sqrtf(2.f / (k * k * cin)) * 0.5f),
                constRand(n, @[@1, @1, @1, @(cout)], 0.01f), cin, cout, k, s };
    n->params += (long)k * k * cin * cout;
    return c;
}

static MPSGraphTensor *conv(Net *n, MPSGraphTensor *x, ConvW c, BOOL relu) {
    int pad = c.k / 2;
    MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:c.s strideInY:c.s
        dilationRateInX:1 dilationRateInY:1 groups:1 paddingLeft:pad paddingRight:pad paddingTop:pad paddingBottom:pad
        paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
    MPSGraphTensor *y = [n->g convolution2DWithSourceTensor:x weightsTensor:c.w descriptor:d name:nil];
    y = [n->g additionWithPrimaryTensor:y secondaryTensor:c.b name:nil];
    n->macs += (double)dimH(y) * dimW(y) * c.cout * c.cin * c.k * c.k;
    return relu ? [n->g leakyReLUWithTensor:y alpha:0.2 name:nil] : y;
}

// ConvTranspose2d(k=4, s=2, p=1): 크기 2배.
static ConvW makeDeconv(Net *n, int cin, int cout) {
    ConvW c = { constRand(n, @[@4, @4, @(cout), @(cin)], sqrtf(2.f / (16 * cin)) * 0.5f),
                constRand(n, @[@1, @1, @1, @(cout)], 0.01f), cin, cout, 4, 2 };
    n->params += 16L * cin * cout;
    return c;
}
static MPSGraphTensor *deconv(Net *n, MPSGraphTensor *x, ConvW c) {
    int H = dimH(x) * 2, W = dimW(x) * 2;
    if (gSkipDeconv) {
        MPSGraphTensor *u = [n->g resizeTensor:x size:@[@(H), @(W)] mode:MPSGraphResizeNearest centerResult:YES alignCorners:NO layout:MPSGraphTensorNamedDataLayoutNHWC name:nil];
        MPSGraphTensor *w1 = [n->g sliceTensor:[n->g sliceTensor:c.w dimension:0 start:0 length:1 name:nil] dimension:1 start:0 length:1 name:nil];
        w1 = [n->g transposeTensor:w1 dimension:2 withDimension:3 name:nil];
        MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:1 strideInY:1 dilationRateInX:1 dilationRateInY:1 groups:1
            paddingLeft:0 paddingRight:0 paddingTop:0 paddingBottom:0 paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
        return [n->g convolution2DWithSourceTensor:u weightsTensor:w1 descriptor:d name:nil];
    }
    MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:2 strideInY:2
        dilationRateInX:1 dilationRateInY:1 groups:1 paddingLeft:1 paddingRight:1 paddingTop:1 paddingBottom:1
        paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
    MPSGraphTensor *y = [n->g convolutionTranspose2DWithSourceTensor:x weightsTensor:c.w outputShape:@[@1, @(H), @(W), @(c.cout)] descriptor:d name:nil];
    y = [n->g additionWithPrimaryTensor:y secondaryTensor:c.b name:nil];
    n->macs += (double)dimH(x) * dimW(x) * c.cout * c.cin * 16;
    return y;
}

static MPSGraphTensor *resize(Net *n, MPSGraphTensor *x, int h, int w) {
    if (dimH(x) == h && dimW(x) == w) return x;
    return [n->g resizeTensor:x size:@[@(h), @(w)] mode:MPSGraphResizeBilinear centerResult:YES alignCorners:NO
                       layout:MPSGraphTensorNamedDataLayoutNHWC name:nil];
}
static MPSGraphTensor *slice(Net *n, MPSGraphTensor *x, int start, int len) {
    return [n->g sliceTensor:x dimension:3 start:start length:len name:nil];
}
static MPSGraphTensor *cat(Net *n, NSArray<MPSGraphTensor *> *ts) { return [n->g concatTensors:ts dimension:3 name:nil]; }
static MPSGraphTensor *mulS(Net *n, MPSGraphTensor *x, double s) {
    return [n->g multiplicationWithPrimaryTensor:x secondaryTensor:[n->g constantWithScalar:s dataType:n->dt] name:nil];
}

// 역방향 워핑: out(x) = src(x + flow(x)), 쌍선형. flow 는 픽셀 단위(상대 좌표).
static int gWarps = 0;
static MPSGraphTensor *warp(Net *n, MPSGraphTensor *src, MPSGraphTensor *flow) {
    gWarps++;
    if (gSkipWarp) return [n->g additionWithPrimaryTensor:src secondaryTensor:[n->g reductionSumWithTensor:flow axis:3 name:nil] name:nil];
    MPSGraphTensor *s = src, *f = flow;
    if (n->warp32 && n->dt != MPSDataTypeFloat32) {
        s = [n->g castTensor:src toType:MPSDataTypeFloat32 name:nil];
        f = [n->g castTensor:flow toType:MPSDataTypeFloat32 name:nil];
    }
    MPSGraphTensor *y = [n->g sampleGridWithSourceTensor:s coordinateTensor:f layout:MPSGraphTensorNamedDataLayoutNHWC
        normalizeCoordinates:NO relativeCoordinates:YES alignCorners:NO paddingMode:MPSGraphPaddingModeClampToEdge
        samplingMode:MPSGraphResizeBilinear constantValue:0 name:nil];
    if (y.dataType != n->dt) y = [n->g castTensor:y toType:n->dt name:nil];
    return y;
}

typedef struct { ConvW c0a, c0b, res[8], last; MPSGraphTensor *beta[8]; int c, scale; } Block;

static Block makeBlock(Net *n, int cin, int c, int scale) {
    Block b; b.c = c; b.scale = scale;
    b.c0a = makeConv(n, cin, c / 2, 3, 2); b.c0b = makeConv(n, c / 2, c, 3, 2);
    for (int i = 0; i < 8; i++) { b.res[i] = makeConv(n, c, c, 3, 1); b.beta[i] = constRand(n, @[@1, @1, @1, @(c)], 1.0f); }
    b.last = makeDeconv(n, c, 52);
    return b;
}

// 반환: flow(4) / mask(1) / feat(8) 을 이어 붙인 13ch, 전체 해상도.
static MPSGraphTensor *runBlock(Net *n, Block *b, MPSGraphTensor *x, MPSGraphTensor *flow) {
    int H = dimH(x), W = dimW(x), h = H / b->scale, w = W / b->scale;
    MPSGraphTensor *xs = resize(n, x, h, w);
    if (flow) xs = cat(n, @[xs, mulS(n, resize(n, flow, h, w), 1.0 / b->scale)]);
    MPSGraphTensor *f = conv(n, xs, b->c0a, YES);
    f = conv(n, f, b->c0b, YES);
    for (int i = 0; i < 8; i++) {
        MPSGraphTensor *y = conv(n, f, b->res[i], NO);
        y = [n->g multiplicationWithPrimaryTensor:y secondaryTensor:b->beta[i] name:nil];
        f = [n->g leakyReLUWithTensor:[n->g additionWithPrimaryTensor:y secondaryTensor:f name:nil] alpha:0.2 name:nil];
    }
    MPSGraphTensor *t = deconv(n, f, b->last);
    t = [n->g depthToSpace2DTensor:t widthAxis:2 heightAxis:1 depthAxis:3 blockSize:2 usePixelShuffleOrder:YES name:nil];
    t = resize(n, t, H, W);
    // flow 는 ×scale, 나머지는 그대로. 비용 차이가 거의 없어 13ch 전체에 곱하지 않고 흐름만 잘라 곱한다.
    MPSGraphTensor *fl = mulS(n, slice(n, t, 0, 4), b->scale);
    return cat(n, @[fl, slice(n, t, 4, 9)]);
}

// ───────────── 빠른 구성(--fast): 같은 계산을 MPSGraph 에 유리한 연산으로 바꿔 쓴다 ─────────────
// 미세 측정(mps-ops-micro.m)에서 1080p 시간 대부분이 합성곱이 아니라 전치 합성곱·전체 해상도 concat·
// 전체 해상도 채널 slice 에서 나왔다. 아래는 학습된 가중치를 재배열하면 원래 IFNet 과 같은 값을 내는 구성이다.
//  1) ConvTranspose(k4,s2,p1) = 출력 위상 4개(2×2 합성곱, 비대칭 패딩)를 이어 붙인 뒤 depthToSpace(2).
//     ('3x3' 모드: 위상 4개를 3×3 합성곱 하나(출력 4배, 일부 가중치 0)로 합침 — 연산 수는 2.25배, 커널 수는 적음)
//  2) 블록 출력 13ch(흐름 0→2ch, 흐름 1→2ch, 마스크 1ch, 특징 8ch)을 처음부터 따로 만든다 → 전체 해상도 slice 없음.
//  3) img 와 Head 특징을 한 번만 이어 붙여(7ch) 워핑 1번으로 처리 → 블록당 워핑 4회 → 2회.
//  4) 축소하는 블록은 각 입력을 먼저 줄인 뒤 작은 해상도에서 concat(쌍선형 축소는 채널별 선형이라 결과 같음).
static int gFastMode = 0;   // 0 = 원래 구성, 1 = phase, 2 = 3x3

static MPSGraphTensor *convPad(Net *n, MPSGraphTensor *x, int cin, int cout, int k, int pl, int pr, int pt, int pb) {
    MPSGraphTensor *w = constRand(n, @[@(k), @(k), @(cin), @(cout)], sqrtf(2.f / (k * k * cin)) * 0.5f);
    n->params += (long)k * k * cin * cout;
    MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:1 strideInY:1
        dilationRateInX:1 dilationRateInY:1 groups:1 paddingLeft:pl paddingRight:pr paddingTop:pt paddingBottom:pb
        paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
    MPSGraphTensor *y = [n->g convolution2DWithSourceTensor:x weightsTensor:w descriptor:d name:nil];
    n->macs += (double)dimH(y) * dimW(y) * cout * cin * k * k;
    return y;
}
static MPSGraphTensor *d2s(Net *n, MPSGraphTensor *x, int b) {
    return [n->g depthToSpace2DTensor:x widthAxis:2 heightAxis:1 depthAxis:3 blockSize:b usePixelShuffleOrder:YES name:nil];
}
// ConvTranspose(k4,s2,p1) cin→cout 와 같은 결과(가중치 재배열 시). 출력은 2배 크기.
static MPSGraphTensor *deconvFast(Net *n, MPSGraphTensor *x, int cin, int cout) {
    MPSGraphTensor *y;
    if (gFastMode == 2) y = convPad(n, x, cin, 4 * cout, 3, 1, 1, 1, 1);
    else {
        NSMutableArray *ph = [NSMutableArray new];
        for (int a = 0; a < 2; a++) for (int b = 0; b < 2; b++)
            [ph addObject:convPad(n, x, cin, cout, 2, b ? 0 : 1, b ? 1 : 0, a ? 0 : 1, a ? 1 : 0)];
        y = cat(n, ph);
    }
    return d2s(n, y, 2);
}
// deconv c→4·k 뒤 PixelShuffle(2) 로 k 채널을 만드는 블록 출력 한 갈래.
static MPSGraphTensor *lastBranch(Net *n, MPSGraphTensor *f, int c, int k) { return d2s(n, deconvFast(n, f, c, 4 * k), 2); }

static MPSGraphTensor *buildFast(Net *n, MPSGraphTensor *img0, MPSGraphTensor *img1, int PH, int PW, const int *cs, const int *sl, int nBlocks) {
    ConvW h0 = makeConv(n, 3, 16, 3, 2), h1 = makeConv(n, 16, 16, 3, 1), h2 = makeConv(n, 16, 16, 3, 1);
    // Head 의 마지막 deconv 16→4 는 두 영상이 같은 가중치를 쓰지만, 여기서는 시간만 보므로 따로 만들어도 비용이 같다.
    MPSGraphTensor *(^head)(MPSGraphTensor *) = ^(MPSGraphTensor *x) {
        MPSGraphTensor *y = conv(n, x, h0, YES); y = conv(n, y, h1, YES); y = conv(n, y, h2, YES);
        return deconvFast(n, y, 16, 4);
    };
    MPSGraphTensor *a0 = cat(n, @[img0, head(img0)]), *a1 = cat(n, @[img1, head(img1)]);   // 7ch
    MPSGraphTensor *fl0 = nil, *fl1 = nil, *mask = nil, *feat = nil;
    for (int i = 0; i < nBlocks; i++) {
        int s = sl[i], h = PH / s, w = PW / s, c = cs[i];
        MPSGraphTensor *ts = [n->g constantWithScalar:0.5 shape:@[@1, @(h), @(w), @1] dataType:n->dt];
        NSArray *parts;
        if (!fl0) parts = @[resize(n, a0, h, w), resize(n, a1, h, w), ts];
        else {
            MPSGraphTensor *w0 = warp(n, a0, fl0), *w1 = warp(n, a1, fl1);
            parts = @[resize(n, w0, h, w), resize(n, w1, h, w), ts, resize(n, mask, h, w), resize(n, feat, h, w),
                      mulS(n, resize(n, fl0, h, w), 1.0 / s), mulS(n, resize(n, fl1, h, w), 1.0 / s)];
        }
        MPSGraphTensor *x = cat(n, parts);
        int cin = fl0 ? 28 : 15;
        ConvW c0a = makeConv(n, cin, c / 2, 3, 2), c0b = makeConv(n, c / 2, c, 3, 2);
        MPSGraphTensor *f = conv(n, conv(n, x, c0a, YES), c0b, YES);
        for (int r = 0; r < 8; r++) {
            ConvW rc = makeConv(n, c, c, 3, 1);
            MPSGraphTensor *beta = constRand(n, @[@1, @1, @1, @(c)], 1.0f);
            MPSGraphTensor *y = [n->g multiplicationWithPrimaryTensor:conv(n, f, rc, NO) secondaryTensor:beta name:nil];
            f = [n->g leakyReLUWithTensor:[n->g additionWithPrimaryTensor:y secondaryTensor:f name:nil] alpha:0.2 name:nil];
        }
        MPSGraphTensor *o0 = lastBranch(n, f, c, 2), *o1 = lastBranch(n, f, c, 2), *om = lastBranch(n, f, c, 1), *of = lastBranch(n, f, c, 8);
        o0 = mulS(n, resize(n, o0, PH, PW), s); o1 = mulS(n, resize(n, o1, PH, PW), s);
        mask = resize(n, om, PH, PW); feat = resize(n, of, PH, PW);
        fl0 = fl0 ? [n->g additionWithPrimaryTensor:fl0 secondaryTensor:o0 name:nil] : o0;
        fl1 = fl1 ? [n->g additionWithPrimaryTensor:fl1 secondaryTensor:o1 name:nil] : o1;
    }
    MPSGraphTensor *wi0 = warp(n, img0, fl0), *wi1 = warp(n, img1, fl1);
    MPSGraphTensor *m = [n->g sigmoidWithTensor:mask name:nil];
    MPSGraphTensor *one = [n->g constantWithScalar:1 dataType:n->dt];
    return [n->g additionWithPrimaryTensor:[n->g multiplicationWithPrimaryTensor:wi0 secondaryTensor:m name:nil]
                           secondaryTensor:[n->g multiplicationWithPrimaryTensor:wi1 secondaryTensor:[n->g subtractionWithPrimaryTensor:one secondaryTensor:m name:nil] name:nil] name:nil];
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        int W = 1920, H = 1080, iters = 100, warm = 10, nBlocks = 5;
        NSString *variant = @"4.25", *dtype = @"f16";
        double scale = 1.0; BOOL warp32 = NO; int opt = 1; BOOL noAutoLayout = NO;
        for (int i = 1; i < argc; i++) {
            NSString *k = @(argv[i]); NSString *v = i + 1 < argc ? @(argv[i + 1]) : @"";
            if ([k isEqual:@"--size"]) { NSArray *p = [v componentsSeparatedByString:@"x"]; W = [p[0] intValue]; H = [p[1] intValue]; i++; }
            else if ([k isEqual:@"--variant"]) { variant = v; i++; }
            else if ([k isEqual:@"--scale"]) { scale = v.doubleValue; i++; }   // Practical-RIFE --scale (0.5 = 흐름을 절반 해상도에서)
            else if ([k isEqual:@"--dtype"]) { dtype = v; i++; }               // f16 | f32
            else if ([k isEqual:@"--warp32"]) { warp32 = YES; }                 // 합성곱 f16 + 워핑 f32
            else if ([k isEqual:@"--iters"]) { iters = v.intValue; i++; }
            else if ([k isEqual:@"--opt"]) { opt = v.intValue; i++; }          // MPSGraph 최적화 단계(1 = ANE·CPU 배치 허용, 기본값)
            else if ([k isEqual:@"--no-auto-layout"]) { noAutoLayout = YES; }   // macOS 27: 합성곱 자동 레이아웃 변환 끔
            else if ([k isEqual:@"--skip-warp"]) { gSkipWarp = YES; }
            else if ([k isEqual:@"--skip-deconv"]) { gSkipDeconv = YES; }
            else if ([k isEqual:@"--blocks"]) { nBlocks = v.intValue; i++; }   // 분석용: 앞에서부터 N개 블록만
            else if ([k isEqual:@"--fast"]) { gFastMode = [v isEqual:@"3x3"] ? 2 : 1; i++; }  // phase | 3x3
        }
        BOOL lite = [variant isEqual:@"lite"];
        int cs[5] = { 192, 128, 96, 64, lite ? 24 : 32 };
        int base[5] = { 16, 8, 4, 2, 1 }, baseLite[5] = { 32, 16, 8, 4, 1 };
        int sl[5];
        for (int i = 0; i < 5; i++) sl[i] = (int)lround((lite ? baseLite[i] : base[i]) / scale);
        // 패딩: 가장 깊은 블록이 (1/scale)·(1/4)까지 줄이므로 그 배수로 맞춘다.
        int mult = sl[0] * 4;
        int PW = (W + mult - 1) / mult * mult, PH = (H + mult - 1) / mult * mult;

        id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
        Net net = { [MPSGraph new], [dtype isEqual:@"f32"] ? MPSDataTypeFloat32 : MPSDataTypeFloat16, warp32, 0, 0 };
        Net *n = &net;
        MPSGraphTensor *img0 = [n->g placeholderWithShape:@[@1, @(PH), @(PW), @3] dataType:n->dt name:@"img0"];
        MPSGraphTensor *img1 = [n->g placeholderWithShape:@[@1, @(PH), @(PW), @3] dataType:n->dt name:@"img1"];

        MPSGraphTensor *merged;
        if (gFastMode) merged = buildFast(n, img0, img1, PH, PW, cs, sl, nBlocks);
        else {
            // Head (두 영상이 가중치 공유)
            ConvW h0 = makeConv(n, 3, 16, 3, 2), h1 = makeConv(n, 16, 16, 3, 1), h2 = makeConv(n, 16, 16, 3, 1);
            ConvW h3 = makeDeconv(n, 16, 4);
            MPSGraphTensor *(^head)(MPSGraphTensor *) = ^(MPSGraphTensor *x) {
                MPSGraphTensor *y = conv(n, x, h0, YES); y = conv(n, y, h1, YES); y = conv(n, y, h2, YES);
                return deconv(n, y, h3);
            };
            MPSGraphTensor *f0 = head(img0), *f1 = head(img1);
            MPSGraphTensor *ts = [n->g constantWithScalar:0.5 shape:@[@1, @(PH), @(PW), @1] dataType:n->dt];

            Block blocks[5];
            for (int i = 0; i < 5; i++) blocks[i] = makeBlock(n, i == 0 ? 15 : 24 + 4, cs[i], sl[i]);
            MPSGraphTensor *flow = nil, *mask = nil, *feat = nil, *wi0 = img0, *wi1 = img1;
            for (int i = 0; i < nBlocks; i++) {
                MPSGraphTensor *out;
                if (!flow) out = runBlock(n, &blocks[i], cat(n, @[img0, img1, f0, f1, ts]), nil);
                else {
                    MPSGraphTensor *wf0 = warp(n, f0, slice(n, flow, 0, 2)), *wf1 = warp(n, f1, slice(n, flow, 2, 2));
                    out = runBlock(n, &blocks[i], cat(n, @[wi0, wi1, wf0, wf1, ts, mask, feat]), flow);
                }
                MPSGraphTensor *fd = slice(n, out, 0, 4);
                flow = flow ? [n->g additionWithPrimaryTensor:flow secondaryTensor:fd name:nil] : fd;
                mask = slice(n, out, 4, 1); feat = slice(n, out, 5, 8);
                wi0 = warp(n, img0, slice(n, flow, 0, 2)); wi1 = warp(n, img1, slice(n, flow, 2, 2));
            }
            MPSGraphTensor *m = [n->g sigmoidWithTensor:mask name:nil];
            MPSGraphTensor *one = [n->g constantWithScalar:1 dataType:n->dt];
            merged = [n->g additionWithPrimaryTensor:[n->g multiplicationWithPrimaryTensor:wi0 secondaryTensor:m name:nil]
                                                     secondaryTensor:[n->g multiplicationWithPrimaryTensor:wi1 secondaryTensor:[n->g subtractionWithPrimaryTensor:one secondaryTensor:m name:nil] name:nil] name:nil];
        }

        if (gSkipWarp || gSkipDeconv || nBlocks < 5) printf("# 분석 모드: skip_warp=%d skip_deconv=%d blocks=%d (실제 RIFE 아님)\n", gSkipWarp, gSkipDeconv, nBlocks);
        printf("# device=%s metal4=%d variant=%s scale=%.2f ladder=%d/%d/%d/%d/%d c=%d/%d/%d/%d/%d dtype=%s warp32=%d fast=%s\n",
               dev.name.UTF8String, (int)[dev supportsFamily:MTLGPUFamilyMetal4], variant.UTF8String, scale,
               sl[0], sl[1], sl[2], sl[3], sl[4], cs[0], cs[1], cs[2], cs[3], cs[4], dtype.UTF8String, warp32,
               gFastMode == 2 ? "3x3" : gFastMode == 1 ? "phase" : "off");
        printf("# input=%dx%d padded=%dx%d params=%.2fM conv_GMAC=%.1f warps=%d\n", W, H, PW, PH, n->params / 1e6, n->macs / 1e9, gWarps);

        MPSGraphDevice *gdev = [MPSGraphDevice deviceWithMTLDevice:dev];
        MPSGraphShapedType *st = [[MPSGraphShapedType alloc] initWithShape:@[@1, @(PH), @(PW), @3] dataType:n->dt];
        MPSGraphCompilationDescriptor *cd = [MPSGraphCompilationDescriptor new];
        cd.optimizationLevel = opt ? MPSGraphOptimizationLevel1 : MPSGraphOptimizationLevel0;
        cd.waitForCompilationCompletion = YES;
        if (noAutoLayout) { if (@available(macOS 27.0, *)) [cd disableAutoLayoutConversion]; }
        double tc = nowMs();
        MPSGraphExecutable *exe = [n->g compileWithDevice:gdev feeds:@{ img0: st, img1: st } targetTensors:@[merged] targetOperations:nil compilationDescriptor:cd];
        printf("compile_ms=%.0f\n", nowMs() - tc);

        size_t elem = n->dt == MPSDataTypeFloat16 ? 2 : 4, bytes = (size_t)PW * PH * 3 * elem;
        id<MTLBuffer> b0 = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        id<MTLBuffer> b1 = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        id<MTLBuffer> bo = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
        // 입력은 0~1 범위의 매끈한 무늬(값 자체는 시간에 영향 없음).
        for (size_t i = 0; i < (size_t)PW * PH * 3; i++) {
            float v0 = 0.5f + 0.4f * sinf(i * 0.0007f), v1 = 0.5f + 0.4f * sinf(i * 0.0007f + 0.05f);
            if (elem == 2) { ((_Float16 *)b0.contents)[i] = v0; ((_Float16 *)b1.contents)[i] = v1; }
            else { ((float *)b0.contents)[i] = v0; ((float *)b1.contents)[i] = v1; }
        }
        NSArray *shape = @[@1, @(PH), @(PW), @3];
        MPSGraphTensorData *t0 = [[MPSGraphTensorData alloc] initWithMTLBuffer:b0 shape:shape dataType:n->dt];
        MPSGraphTensorData *t1 = [[MPSGraphTensorData alloc] initWithMTLBuffer:b1 shape:shape dataType:n->dt];
        MPSGraphTensorData *to = [[MPSGraphTensorData alloc] initWithMTLBuffer:bo shape:shape dataType:n->dt];
        id<MTLCommandQueue> q = [dev newCommandQueue];
        double *ms = calloc(iters, sizeof(double));
        for (int it = -warm; it < iters; it++) {
            MPSCommandBuffer *cb = [MPSCommandBuffer commandBufferFromCommandQueue:q];
            double t = nowMs();
            [exe encodeToCommandBuffer:cb inputsArray:@[t0, t1] resultsArray:@[to] executionDescriptor:nil];
            [cb commit]; [cb waitUntilCompleted];
            double wall = nowMs() - t;
            if (cb.error) { printf("실행 오류: %s\n", cb.error.description.UTF8String); return 2; }
            if (it >= 0) ms[it] = wall;
        }
        // 출력이 유한한지(NaN 확인) — 무작위 가중치라 값의 의미는 없다.
        int nonfinite = 0;
        for (size_t i = 0; i < (size_t)PW * PH * 3; i += 997) {
            float v = elem == 2 ? (float)((_Float16 *)bo.contents)[i] : ((float *)bo.contents)[i];
            if (!isfinite(v)) nonfinite++;
        }
        double mean = 0; for (int i = 0; i < iters; i++) mean += ms[i]; mean /= iters;
        printf("wall_ms mean=%.2f p50=%.2f p95=%.2f p99=%.2f max=%.2f  (encode+GPU+대기, %d회)\n", mean,
               pct(ms, iters, 0.5), pct(ms, iters, 0.95), pct(ms, iters, 0.99), pct(ms, iters, 1.0), iters);
        printf("effective_TFLOPS(합성곱 기준)=%.2f  nonfinite_samples=%d  opt=%d\n",
               2 * n->macs / (pct(ms, iters, 0.5) * 1e-3) / 1e12, nonfinite, opt);
        return 0;
    }
}

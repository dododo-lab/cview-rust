// RIFE IFNet(Practical-RIFE 4.25 / 4.25.lite)을 MPSGraph 로 구성하고 실행하는 공용 코드, 그리고
// PyTorch 연산 의미를 그대로 따른 CPU 기준 구현. rife-mpsgraph.m(시간·정합)과 bench.m(--algo rife, 화질)이 함께 쓴다.
//
// 구조는 내려받은 train_log/IFNet_HDv3.py(4.25, 4.25.lite)와 같다. 실행하지 않고 읽기만 했다.
//   Head(가중치 공유): conv 3→16 s2, conv 16→16, conv 16→16, ConvTranspose 16→4 k4 s2 p1 (LeakyReLU 0.2)
//   IFBlock: interpolate(1/scale) → conv in→c/2 s2 → conv c/2→c s2 → ResConv(c)×8 → ConvTranspose c→52 k4 s2 p1
//            → PixelShuffle(2) = 13ch → interpolate(×scale), flow = [0:4]×scale, mask = [4], feat = [5:13]
//   블록 0 입력 cat(img0, img1, f0, f1, t), 블록 1~4 입력 cat(warp(img0), warp(img1), warp(f0), warp(f1), t, mask, feat) + flow
//   끝: sigmoid(mask) 로 warp(img0)·warp(img1) 섞기. scale_list 4.25 = [16,8,4,2,1]/scale, lite = [32,16,8,4,1]/scale
//   warp = grid_sample(bilinear, padding border, align_corners=True), 흐름 채널 0 = x, 1 = y (픽셀 단위)
//
// 가중치: rife-export.py 가 만든 <폴더>/weights.bin + index.json (PyTorch 이름, "module." 뗌). 없으면 무작위.
//
// 빠른 구성(fast = 1 phase, 2 3x3): 같은 값을 내도록 가중치를 재배열한 구성이다.
//  - ConvTranspose(k4,s2,p1) = 출력 위상 4개(2×2 합성곱, 비대칭 패딩)를 이어 붙인 뒤 depthToSpace(2, TF 순서).
//    3x3 모드는 위상 4개를 3×3 합성곱 하나(쓰지 않는 탭은 0)로 합친다.
//  - 블록 출력 13ch 을 흐름 0(2)·흐름 1(2)·마스크(1)·특징(8) 갈래로 따로 만든다(전치 합성곱 출력 채널을 4개씩 나눔).
//  - img 와 Head 특징을 한 번만 이어 붙여(7ch) 워핑 한 번으로 처리한다. 블록 입력 채널 순서가 바뀌므로
//    conv0 첫 합성곱의 입력 채널을 맞춰 재배열한다.
//  - 축소 블록은 입력을 먼저 줄인 뒤 이어 붙인다(쌍선형 축소는 채널별이라 결과 같음).
#pragma once
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <MetalPerformanceShaders/MetalPerformanceShaders.h>
#import <MetalPerformanceShadersGraph/MetalPerformanceShadersGraph.h>
#include <mach/mach_time.h>
#include <math.h>

static double rifeNowMs(void) {
    static mach_timebase_info_data_t tb;
    if (tb.denom == 0) mach_timebase_info(&tb);
    return (double)mach_absolute_time() * tb.numer / tb.denom / 1e6;
}

typedef struct {
    int lite;          // 0 = 4.25, 1 = 4.25.lite
    double scale;      // Practical-RIFE --scale (1, 0.5 ...)
    int fast;          // 0 원래 구성, 1 phase, 2 3x3
    MPSDataType dt;    // 합성곱 정밀도
    BOOL warp32;       // 워핑만 FP32
    int blocks;        // 분석용: 앞에서부터 N개 블록(원래 구성만)
    BOOL skipWarp;     // 분석용: 워핑을 항등으로(원래 구성만)
    BOOL gridAlign;    // sampleGrid alignCorners (정합 시험으로 정함)
} RifeCfg;

static RifeCfg rifeDefaultCfg(void) { RifeCfg c = { 0, 1.0, 0, MPSDataTypeFloat16, NO, 5, NO, YES }; return c; }

static int rifeChannels(int lite, int i) { static const int c[5] = { 192, 128, 96, 64, 32 }; return (lite && i == 4) ? 24 : c[i]; }
static int rifeBlockScale(int lite, double scale, int i) {
    static const int a[5] = { 16, 8, 4, 2, 1 }, b[5] = { 32, 16, 8, 4, 1 };
    return (int)lround((lite ? b[i] : a[i]) / scale);
}
static int rifePadMult(int lite, double scale) { return rifeBlockScale(lite, scale, 0) * 4; }

typedef struct {
    NSDictionary *index;   // 이름 → {shape, offset}
    NSData *blob;          // float32
} RifeWeights;

static RifeWeights *rifeLoadWeights(NSString *dir) {
    if (!dir.length) return NULL;
    NSData *j = [NSData dataWithContentsOfFile:[dir stringByAppendingPathComponent:@"index.json"]];
    NSData *b = [NSData dataWithContentsOfFile:[dir stringByAppendingPathComponent:@"weights.bin"] options:NSDataReadingMappedIfSafe error:nil];
    if (!j || !b) { fprintf(stderr, "가중치를 읽지 못함: %s\n", dir.UTF8String); exit(6); }
    RifeWeights *w = calloc(1, sizeof(RifeWeights));
    w->index = [NSJSONSerialization JSONObjectWithData:j options:0 error:nil];
    w->blob = b;
    return w;
}

typedef struct {
    MPSGraph *g;
    RifeCfg cfg;
    RifeWeights *w;
    double macs; long params; int warps;
    uint64_t rng;
} RifeNet;

static float rifeRand(RifeNet *n) {
    n->rng = n->rng * 6364136223846793005ULL + 1442695040888963407ULL;
    return ((n->rng >> 40) & 0xFFFFFF) / (float)0x1000000 * 2.f - 1.f;
}

// 이름의 가중치(float*), 무작위 모드면 NULL. 크기를 확인한다.
static const float *rifeW(RifeWeights *w, NSString *name, long expect) {
    if (!w) return NULL;
    NSDictionary *e = w->index[name];
    if (!e) { fprintf(stderr, "가중치 없음: %s\n", name.UTF8String); exit(6); }
    long cnt = 1; for (NSNumber *s in e[@"shape"]) cnt *= s.longValue;
    if (expect >= 0 && cnt != expect) { fprintf(stderr, "가중치 크기 불일치 %s: %ld (기대 %ld)\n", name.UTF8String, cnt, expect); exit(6); }
    return (const float *)w->blob.bytes + [e[@"offset"] longValue];
}

static MPSGraphTensor *rifeConst(RifeNet *n, const float *v, long count, NSArray<NSNumber *> *shape, MPSDataType dt) {
    NSMutableData *d;
    if (dt == MPSDataTypeFloat16) { d = [NSMutableData dataWithLength:count * 2]; _Float16 *p = d.mutableBytes; for (long i = 0; i < count; i++) p[i] = (_Float16)v[i]; }
    else { d = [NSMutableData dataWithBytes:v length:count * 4]; }
    return [n->g constantWithData:d shape:shape dataType:dt];
}

static int rifeH(MPSGraphTensor *t) { return t.shape[1].intValue; }
static int rifeWd(MPSGraphTensor *t) { return t.shape[2].intValue; }

typedef struct { MPSGraphTensor *w, *b; int cin, cout, k, s; } RifeConv;

// PyTorch Conv2d 가중치 [O][I][k][k] → MPSGraph HWIO. perm[i_new] = 원래 입력 채널 번호(없으면 그대로).
static RifeConv rifeConvW(RifeNet *n, NSString *prefix, int cin, int cout, int k, int s, const int *perm) {
    long cnt = (long)k * k * cin * cout;
    const float *W = rifeW(n->w, [prefix stringByAppendingString:@".weight"], cnt);
    const float *B = rifeW(n->w, [prefix stringByAppendingString:@".bias"], cout);
    float *hw = malloc(cnt * 4), *bb = malloc(cout * 4);
    float sc = sqrtf(2.f / (k * k * cin)) * 0.5f;
    for (int kh = 0; kh < k; kh++) for (int kw = 0; kw < k; kw++) for (int i = 0; i < cin; i++) for (int o = 0; o < cout; o++) {
        int pi = perm ? perm[i] : i;
        hw[((kh * k + kw) * cin + i) * cout + o] = W ? W[(((long)o * cin + pi) * k + kh) * k + kw] : rifeRand(n) * sc;
    }
    for (int o = 0; o < cout; o++) bb[o] = B ? B[o] : rifeRand(n) * 0.01f;
    RifeConv c = { rifeConst(n, hw, cnt, @[@(k), @(k), @(cin), @(cout)], n->cfg.dt), rifeConst(n, bb, cout, @[@1, @1, @1, @(cout)], n->cfg.dt), cin, cout, k, s };
    free(hw); free(bb);
    n->params += cnt + cout;
    return c;
}

static MPSGraphTensor *rifeConvRun(RifeNet *n, MPSGraphTensor *x, RifeConv c, BOOL relu) {
    int pad = c.k / 2;
    MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:c.s strideInY:c.s
        dilationRateInX:1 dilationRateInY:1 groups:1 paddingLeft:pad paddingRight:pad paddingTop:pad paddingBottom:pad
        paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
    MPSGraphTensor *y = [n->g convolution2DWithSourceTensor:x weightsTensor:c.w descriptor:d name:nil];
    y = [n->g additionWithPrimaryTensor:y secondaryTensor:c.b name:nil];
    n->macs += (double)rifeH(y) * rifeWd(y) * c.cout * c.cin * c.k * c.k;
    return relu ? [n->g leakyReLUWithTensor:y alpha:0.2 name:nil] : y;
}

// 원래 구성의 ConvTranspose2d(k4,s2,p1). PyTorch 가중치 [I][O][4][4] → MPSGraph [4][4][O][I].
static MPSGraphTensor *rifeDeconvOrig(RifeNet *n, MPSGraphTensor *x, NSString *prefix, int cin, int cout) {
    long cnt = 16L * cin * cout;
    const float *W = rifeW(n->w, [prefix stringByAppendingString:@".weight"], cnt);
    const float *B = rifeW(n->w, [prefix stringByAppendingString:@".bias"], cout);
    float *hw = malloc(cnt * 4), *bb = malloc(cout * 4), sc = sqrtf(2.f / (16 * cin)) * 0.5f;
    for (int kh = 0; kh < 4; kh++) for (int kw = 0; kw < 4; kw++) for (int o = 0; o < cout; o++) for (int i = 0; i < cin; i++)
        hw[((kh * 4 + kw) * cout + o) * cin + i] = W ? W[(((long)i * cout + o) * 4 + kh) * 4 + kw] : rifeRand(n) * sc;
    for (int o = 0; o < cout; o++) bb[o] = B ? B[o] : rifeRand(n) * 0.01f;
    MPSGraphTensor *w = rifeConst(n, hw, cnt, @[@4, @4, @(cout), @(cin)], n->cfg.dt), *b = rifeConst(n, bb, cout, @[@1, @1, @1, @(cout)], n->cfg.dt);
    free(hw); free(bb); n->params += cnt + cout;
    int H = rifeH(x) * 2, Wd = rifeWd(x) * 2;
    MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:2 strideInY:2
        dilationRateInX:1 dilationRateInY:1 groups:1 paddingLeft:1 paddingRight:1 paddingTop:1 paddingBottom:1
        paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
    MPSGraphTensor *y = [n->g convolutionTranspose2DWithSourceTensor:x weightsTensor:w outputShape:@[@1, @(H), @(Wd), @(cout)] descriptor:d name:nil];
    n->macs += (double)rifeH(x) * rifeWd(x) * cout * cin * 16;
    return [n->g additionWithPrimaryTensor:y secondaryTensor:b name:nil];
}

static MPSGraphTensor *rifeD2S(RifeNet *n, MPSGraphTensor *x, int b, BOOL pixelShuffleOrder) {
    return [n->g depthToSpace2DTensor:x widthAxis:2 heightAxis:1 depthAxis:3 blockSize:b usePixelShuffleOrder:pixelShuffleOrder name:nil];
}

// 빠른 구성의 ConvTranspose2d(k4,s2,p1): 원래 출력 채널 [start, start+count) 만 만든다. 결과는 2배 크기.
// 출력 (2m+a, 2n+b) 은 입력 행 {m-1, m}(a=0, 탭 kh 3·1) 또는 {m, m+1}(a=1, 탭 kh 2·0) 만 쓴다(열도 같음).
static MPSGraphTensor *rifeDeconvFast(RifeNet *n, MPSGraphTensor *x, NSString *prefix, int cin, int coutAll, int start, int count) {
    long full = 16L * cin * coutAll;
    const float *W = rifeW(n->w, [prefix stringByAppendingString:@".weight"], full);
    const float *B = rifeW(n->w, [prefix stringByAppendingString:@".bias"], coutAll);
    float *Wr = NULL, *Br = NULL;
    if (!W) {   // 무작위: 같은 모양의 원래 가중치를 만든 뒤 같은 방식으로 재배열
        Wr = malloc(full * 4); Br = malloc(coutAll * 4); float sc = sqrtf(2.f / (16 * cin)) * 0.5f;
        for (long i = 0; i < full; i++) Wr[i] = rifeRand(n) * sc;
        for (int o = 0; o < coutAll; o++) Br[o] = rifeRand(n) * 0.01f;
        W = Wr; B = Br;
    }
    #define WT(i, o, kh, kw) W[(((long)(i) * coutAll + (start + (o))) * 4 + (kh)) * 4 + (kw)]
    MPSGraphTensor *y;
    int h = rifeH(x), wd = rifeWd(x);
    if (n->cfg.fast == 2) {
        static const int T3[2][3] = { { 3, 1, -1 }, { -1, 2, 0 } };   // 3×3 탭 → 원래 kh(없으면 -1)
        long cnt = 9L * cin * 4 * count; float *k3 = calloc(cnt, 4), *bb = malloc(4 * count * 4);
        for (int a = 0; a < 2; a++) for (int b = 0; b < 2; b++) for (int r = 0; r < 3; r++) for (int s = 0; s < 3; s++) {
            int kh = T3[a][r], kw = T3[b][s]; if (kh < 0 || kw < 0) continue;
            for (int i = 0; i < cin; i++) for (int o = 0; o < count; o++)
                k3[((r * 3 + s) * cin + i) * (4 * count) + (a * 2 + b) * count + o] = WT(i, o, kh, kw);
        }
        for (int p = 0; p < 4; p++) for (int o = 0; o < count; o++) bb[p * count + o] = B[start + o];
        MPSGraphTensor *wt = rifeConst(n, k3, cnt, @[@3, @3, @(cin), @(4 * count)], n->cfg.dt), *bt = rifeConst(n, bb, 4 * count, @[@1, @1, @1, @(4 * count)], n->cfg.dt);
        free(k3); free(bb);
        MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:1 strideInY:1 dilationRateInX:1 dilationRateInY:1 groups:1
            paddingLeft:1 paddingRight:1 paddingTop:1 paddingBottom:1 paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
        y = [n->g additionWithPrimaryTensor:[n->g convolution2DWithSourceTensor:x weightsTensor:wt descriptor:d name:nil] secondaryTensor:bt name:nil];
        n->macs += (double)h * wd * cin * 4 * count * 9;
    } else {
        static const int T2[2][2] = { { 3, 1 }, { 2, 0 } };   // 2×2 탭 → 원래 kh
        NSMutableArray *ph = [NSMutableArray new];
        for (int a = 0; a < 2; a++) for (int b = 0; b < 2; b++) {
            long cnt = 4L * cin * count; float *k2 = malloc(cnt * 4), *bb = malloc(count * 4);
            for (int r = 0; r < 2; r++) for (int s = 0; s < 2; s++) for (int i = 0; i < cin; i++) for (int o = 0; o < count; o++)
                k2[((r * 2 + s) * cin + i) * count + o] = WT(i, o, T2[a][r], T2[b][s]);
            for (int o = 0; o < count; o++) bb[o] = B[start + o];
            MPSGraphTensor *wt = rifeConst(n, k2, cnt, @[@2, @2, @(cin), @(count)], n->cfg.dt), *bt = rifeConst(n, bb, count, @[@1, @1, @1, @(count)], n->cfg.dt);
            free(k2); free(bb);
            MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:1 strideInY:1 dilationRateInX:1 dilationRateInY:1 groups:1
                paddingLeft:b ? 0 : 1 paddingRight:b ? 1 : 0 paddingTop:a ? 0 : 1 paddingBottom:a ? 1 : 0
                paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
            [ph addObject:[n->g additionWithPrimaryTensor:[n->g convolution2DWithSourceTensor:x weightsTensor:wt descriptor:d name:nil] secondaryTensor:bt name:nil]];
            n->macs += (double)h * wd * cin * count * 4;
        }
        y = [n->g concatTensors:ph dimension:3 name:nil];
    }
    #undef WT
    if (Wr) { free(Wr); free(Br); }
    n->params += 16L * cin * count + count;
    return rifeD2S(n, y, 2, NO);   // 채널 = 위상(a·2+b)·count + o → TF 순서
}

static MPSGraphTensor *rifeResize(RifeNet *n, MPSGraphTensor *x, int h, int w) {
    if (rifeH(x) == h && rifeWd(x) == w) return x;
    return [n->g resizeTensor:x size:@[@(h), @(w)] mode:MPSGraphResizeBilinear centerResult:YES alignCorners:NO layout:MPSGraphTensorNamedDataLayoutNHWC name:nil];
}
static MPSGraphTensor *rifeSlice(RifeNet *n, MPSGraphTensor *x, int s, int l) { return [n->g sliceTensor:x dimension:3 start:s length:l name:nil]; }
static MPSGraphTensor *rifeCat(RifeNet *n, NSArray *ts) { return [n->g concatTensors:ts dimension:3 name:nil]; }
static MPSGraphTensor *rifeMul(RifeNet *n, MPSGraphTensor *x, double s) {
    return s == 1.0 ? x : [n->g multiplicationWithPrimaryTensor:x secondaryTensor:[n->g constantWithScalar:s dataType:n->cfg.dt] name:nil];
}

// warp(src, flow): out(p) = src(p + flow(p)), 쌍선형, 가장자리 고정. flow 채널 0 = x.
static MPSGraphTensor *rifeWarp(RifeNet *n, MPSGraphTensor *src, MPSGraphTensor *flow) {
    n->warps++;
    if (n->cfg.skipWarp) return [n->g additionWithPrimaryTensor:src secondaryTensor:[n->g reductionSumWithTensor:flow axis:3 name:nil] name:nil];
    MPSGraphTensor *s = src, *f = flow;
    if (n->cfg.warp32 && n->cfg.dt != MPSDataTypeFloat32) {
        s = [n->g castTensor:src toType:MPSDataTypeFloat32 name:nil];
        f = [n->g castTensor:flow toType:MPSDataTypeFloat32 name:nil];
    }
    MPSGraphTensor *y = [n->g sampleGridWithSourceTensor:s coordinateTensor:f layout:MPSGraphTensorNamedDataLayoutNHWC
        normalizeCoordinates:NO relativeCoordinates:YES alignCorners:n->cfg.gridAlign paddingMode:MPSGraphPaddingModeClampToEdge
        samplingMode:MPSGraphResizeBilinear constantValue:0 name:nil];
    if (y.dataType != n->cfg.dt) y = [n->g castTensor:y toType:n->cfg.dt name:nil];
    return y;
}

static MPSGraphTensor *rifeResConvs(RifeNet *n, MPSGraphTensor *f, int bi, int c) {
    for (int r = 0; r < 8; r++) {
        NSString *p = [NSString stringWithFormat:@"block%d.convblock.%d", bi, r];
        RifeConv rc = rifeConvW(n, [p stringByAppendingString:@".conv"], c, c, 3, 1, NULL);
        const float *bw = rifeW(n->w, [p stringByAppendingString:@".beta"], c);
        float *bv = malloc(c * 4); for (int i = 0; i < c; i++) bv[i] = bw ? bw[i] : 1.0f + 0.1f * rifeRand(n);
        MPSGraphTensor *beta = rifeConst(n, bv, c, @[@1, @1, @1, @(c)], n->cfg.dt); free(bv); n->params += c;
        MPSGraphTensor *y = [n->g multiplicationWithPrimaryTensor:rifeConvRun(n, f, rc, NO) secondaryTensor:beta name:nil];
        f = [n->g leakyReLUWithTensor:[n->g additionWithPrimaryTensor:y secondaryTensor:f name:nil] alpha:0.2 name:nil];
    }
    return f;
}

static MPSGraphTensor *rifeHead(RifeNet *n, MPSGraphTensor *x) {
    RifeConv h0 = rifeConvW(n, @"encode.cnn0", 3, 16, 3, 2, NULL), h1 = rifeConvW(n, @"encode.cnn1", 16, 16, 3, 1, NULL), h2 = rifeConvW(n, @"encode.cnn2", 16, 16, 3, 1, NULL);
    MPSGraphTensor *y = rifeConvRun(n, rifeConvRun(n, rifeConvRun(n, x, h0, YES), h1, YES), h2, YES);
    return n->cfg.fast ? rifeDeconvFast(n, y, @"encode.cnn3", 16, 4, 0, 4) : rifeDeconvOrig(n, y, @"encode.cnn3", 16, 4);
}

// 원래 구성(PyTorch 와 같은 순서). 반환 = 합성 결과(1,PH,PW,3).
static MPSGraphTensor *rifeBuildOrig(RifeNet *n, MPSGraphTensor *img0, MPSGraphTensor *img1, int PH, int PW) {
    MPSGraphTensor *f0 = rifeHead(n, img0), *f1 = rifeHead(n, img1);
    MPSGraphTensor *ts = [n->g constantWithScalar:0.5 shape:@[@1, @(PH), @(PW), @1] dataType:n->cfg.dt];
    MPSGraphTensor *flow = nil, *mask = nil, *feat = nil, *wi0 = img0, *wi1 = img1;
    for (int i = 0; i < n->cfg.blocks; i++) {
        int s = rifeBlockScale(n->cfg.lite, n->cfg.scale, i), c = rifeChannels(n->cfg.lite, i), h = PH / s, w = PW / s;
        MPSGraphTensor *x;
        if (!flow) x = rifeCat(n, @[img0, img1, f0, f1, ts]);
        else x = rifeCat(n, @[wi0, wi1, rifeWarp(n, f0, rifeSlice(n, flow, 0, 2)), rifeWarp(n, f1, rifeSlice(n, flow, 2, 2)), ts, mask, feat]);
        MPSGraphTensor *xs = rifeResize(n, x, h, w);
        if (flow) xs = rifeCat(n, @[xs, rifeMul(n, rifeResize(n, flow, h, w), 1.0 / s)]);
        NSString *p = [NSString stringWithFormat:@"block%d", i];
        RifeConv c0a = rifeConvW(n, [p stringByAppendingString:@".conv0.0.0"], flow ? 28 : 15, c / 2, 3, 2, NULL);
        RifeConv c0b = rifeConvW(n, [p stringByAppendingString:@".conv0.1.0"], c / 2, c, 3, 2, NULL);
        MPSGraphTensor *f = rifeResConvs(n, rifeConvRun(n, rifeConvRun(n, xs, c0a, YES), c0b, YES), i, c);
        MPSGraphTensor *t = rifeD2S(n, rifeDeconvOrig(n, f, [p stringByAppendingString:@".lastconv.0"], c, 52), 2, YES);
        t = rifeResize(n, t, PH, PW);
        MPSGraphTensor *fd = rifeMul(n, rifeSlice(n, t, 0, 4), s);
        flow = flow ? [n->g additionWithPrimaryTensor:flow secondaryTensor:fd name:nil] : fd;
        mask = rifeSlice(n, t, 4, 1); feat = rifeSlice(n, t, 5, 8);
        wi0 = rifeWarp(n, img0, rifeSlice(n, flow, 0, 2)); wi1 = rifeWarp(n, img1, rifeSlice(n, flow, 2, 2));
    }
    MPSGraphTensor *m = [n->g sigmoidWithTensor:mask name:nil], *one = [n->g constantWithScalar:1 dataType:n->cfg.dt];
    return [n->g additionWithPrimaryTensor:[n->g multiplicationWithPrimaryTensor:wi0 secondaryTensor:m name:nil]
                           secondaryTensor:[n->g multiplicationWithPrimaryTensor:wi1 secondaryTensor:[n->g subtractionWithPrimaryTensor:one secondaryTensor:m name:nil] name:nil] name:nil];
}

static MPSGraphTensor *rifeBuildFast(RifeNet *n, MPSGraphTensor *img0, MPSGraphTensor *img1, int PH, int PW) {
    // 빠른 구성 입력 순서 → 원래 입력 채널 번호
    static const int perm0[15] = { 0, 1, 2, 6, 7, 8, 9, 3, 4, 5, 10, 11, 12, 13, 14 };
    static const int permN[28] = { 0, 1, 2, 6, 7, 8, 9, 3, 4, 5, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27 };
    MPSGraphTensor *a0 = rifeCat(n, @[img0, rifeHead(n, img0)]), *a1 = rifeCat(n, @[img1, rifeHead(n, img1)]);   // 7ch
    MPSGraphTensor *fl0 = nil, *fl1 = nil, *mask = nil, *feat = nil;
    for (int i = 0; i < n->cfg.blocks; i++) {
        int s = rifeBlockScale(n->cfg.lite, n->cfg.scale, i), c = rifeChannels(n->cfg.lite, i), h = PH / s, w = PW / s;
        MPSGraphTensor *ts = [n->g constantWithScalar:0.5 shape:@[@1, @(h), @(w), @1] dataType:n->cfg.dt];
        NSArray *parts;
        if (!fl0) parts = @[rifeResize(n, a0, h, w), rifeResize(n, a1, h, w), ts];
        else {
            MPSGraphTensor *w0 = rifeWarp(n, a0, fl0), *w1 = rifeWarp(n, a1, fl1);
            parts = @[rifeResize(n, w0, h, w), rifeResize(n, w1, h, w), ts, rifeResize(n, mask, h, w), rifeResize(n, feat, h, w),
                      rifeMul(n, rifeResize(n, fl0, h, w), 1.0 / s), rifeMul(n, rifeResize(n, fl1, h, w), 1.0 / s)];
        }
        NSString *p = [NSString stringWithFormat:@"block%d", i];
        RifeConv c0a = rifeConvW(n, [p stringByAppendingString:@".conv0.0.0"], fl0 ? 28 : 15, c / 2, 3, 2, fl0 ? permN : perm0);
        RifeConv c0b = rifeConvW(n, [p stringByAppendingString:@".conv0.1.0"], c / 2, c, 3, 2, NULL);
        MPSGraphTensor *f = rifeResConvs(n, rifeConvRun(n, rifeConvRun(n, rifeCat(n, parts), c0a, YES), c0b, YES), i, c);
        // 13ch = [흐름0 0:2, 흐름1 2:4, 마스크 4, 특징 5:13] ← 전치 합성곱 출력 채널 4개씩
        NSString *lc = [p stringByAppendingString:@".lastconv.0"];
        MPSGraphTensor *o0 = rifeD2S(n, rifeDeconvFast(n, f, lc, c, 52, 0, 8), 2, YES);
        MPSGraphTensor *o1 = rifeD2S(n, rifeDeconvFast(n, f, lc, c, 52, 8, 8), 2, YES);
        MPSGraphTensor *om = rifeD2S(n, rifeDeconvFast(n, f, lc, c, 52, 16, 4), 2, YES);
        MPSGraphTensor *of = rifeD2S(n, rifeDeconvFast(n, f, lc, c, 52, 20, 32), 2, YES);
        if (s == 1) {
            // MPSGraph 우회: depthToSpace 두 번(TF 순서 → PixelShuffle 순서) 뒤에 확대 없이 바로 더하기가 붙으면
            // 왼쪽 가장자리 값이 틀린다(macOS 27.3, 4.25 정합 시험에서 최대 차 0.56). identity 로 끊으면 맞는다.
            // ×1·+0 은 최적화로 지워져 효과가 없었다.
            o0 = [n->g identityWithTensor:o0 name:nil]; o1 = [n->g identityWithTensor:o1 name:nil];
            om = [n->g identityWithTensor:om name:nil]; of = [n->g identityWithTensor:of name:nil];
        }
        o0 = rifeMul(n, rifeResize(n, o0, PH, PW), s); o1 = rifeMul(n, rifeResize(n, o1, PH, PW), s);
        mask = rifeResize(n, om, PH, PW); feat = rifeResize(n, of, PH, PW);
        fl0 = fl0 ? [n->g additionWithPrimaryTensor:fl0 secondaryTensor:o0 name:nil] : o0;
        fl1 = fl1 ? [n->g additionWithPrimaryTensor:fl1 secondaryTensor:o1 name:nil] : o1;
    }
    MPSGraphTensor *wi0 = rifeWarp(n, img0, fl0), *wi1 = rifeWarp(n, img1, fl1);
    MPSGraphTensor *m = [n->g sigmoidWithTensor:mask name:nil], *one = [n->g constantWithScalar:1 dataType:n->cfg.dt];
    return [n->g additionWithPrimaryTensor:[n->g multiplicationWithPrimaryTensor:wi0 secondaryTensor:m name:nil]
                           secondaryTensor:[n->g multiplicationWithPrimaryTensor:wi1 secondaryTensor:[n->g subtractionWithPrimaryTensor:one secondaryTensor:m name:nil] name:nil] name:nil];
}

// ───────── 실행 엔진: 컴파일된 그래프 + 입력·출력 버퍼 ─────────
typedef struct {
    RifeCfg cfg; int W, H, PW, PH; double macs; long params; int warps; double compileMs;
    id<MTLDevice> dev; id<MTLCommandQueue> q;
    MPSGraphExecutable *exe;
    id<MTLBuffer> b0, b1, bo;
    MPSGraphTensorData *t0, *t1, *to;
} RifeEngine;

static RifeEngine *rifeEngineNew(id<MTLDevice> dev, int W, int H, RifeCfg cfg, RifeWeights *w) {
    RifeEngine *e = calloc(1, sizeof(RifeEngine));
    e->cfg = cfg; e->W = W; e->H = H; e->dev = dev; e->q = [dev newCommandQueue];
    int mult = rifePadMult(cfg.lite, cfg.scale);
    e->PW = (W + mult - 1) / mult * mult; e->PH = (H + mult - 1) / mult * mult;
    RifeNet net = { [MPSGraph new], cfg, w, 0, 0, 0, 0x9E3779B97F4A7C15ULL };
    MPSGraphTensor *img0 = [net.g placeholderWithShape:@[@1, @(e->PH), @(e->PW), @3] dataType:cfg.dt name:@"img0"];
    MPSGraphTensor *img1 = [net.g placeholderWithShape:@[@1, @(e->PH), @(e->PW), @3] dataType:cfg.dt name:@"img1"];
    MPSGraphTensor *out = cfg.fast ? rifeBuildFast(&net, img0, img1, e->PH, e->PW) : rifeBuildOrig(&net, img0, img1, e->PH, e->PW);
    e->macs = net.macs; e->params = net.params; e->warps = net.warps;
    MPSGraphShapedType *st = [[MPSGraphShapedType alloc] initWithShape:@[@1, @(e->PH), @(e->PW), @3] dataType:cfg.dt];
    MPSGraphCompilationDescriptor *cd = [MPSGraphCompilationDescriptor new];
    cd.waitForCompilationCompletion = YES;
    double t = rifeNowMs();
    e->exe = [net.g compileWithDevice:[MPSGraphDevice deviceWithMTLDevice:dev] feeds:@{ img0: st, img1: st } targetTensors:@[out] targetOperations:nil compilationDescriptor:cd];
    e->compileMs = rifeNowMs() - t;
    size_t bytes = (size_t)e->PW * e->PH * 3 * (cfg.dt == MPSDataTypeFloat16 ? 2 : 4);
    e->b0 = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
    e->b1 = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
    e->bo = [dev newBufferWithLength:bytes options:MTLResourceStorageModeShared];
    NSArray *shape = @[@1, @(e->PH), @(e->PW), @3];
    e->t0 = [[MPSGraphTensorData alloc] initWithMTLBuffer:e->b0 shape:shape dataType:cfg.dt];
    e->t1 = [[MPSGraphTensorData alloc] initWithMTLBuffer:e->b1 shape:shape dataType:cfg.dt];
    e->to = [[MPSGraphTensorData alloc] initWithMTLBuffer:e->bo shape:shape dataType:cfg.dt];
    return e;
}

// rgb: W×H×3 float(0~1), 행 우선. 패딩은 0(Practical-RIFE 의 F.pad 와 같음).
static void rifeSetInput(RifeEngine *e, int which, const float *rgb) {
    id<MTLBuffer> b = which ? e->b1 : e->b0;
    BOOL h16 = e->cfg.dt == MPSDataTypeFloat16;
    memset(b.contents, 0, b.length);
    for (int y = 0; y < e->H; y++) for (int x = 0; x < e->W * 3; x++) {
        size_t di = ((size_t)y * e->PW) * 3 + x; float v = rgb[(size_t)y * e->W * 3 + x];
        if (h16) ((_Float16 *)b.contents)[di] = (_Float16)v; else ((float *)b.contents)[di] = v;
    }
}
static void rifeGetOutput(RifeEngine *e, float *rgb) {
    BOOL h16 = e->cfg.dt == MPSDataTypeFloat16;
    for (int y = 0; y < e->H; y++) for (int x = 0; x < e->W * 3; x++) {
        size_t si = ((size_t)y * e->PW) * 3 + x;
        rgb[(size_t)y * e->W * 3 + x] = h16 ? (float)((_Float16 *)e->bo.contents)[si] : ((float *)e->bo.contents)[si];
    }
}
// 한 번 실행(동기). 반환: 인코딩 + GPU + 완료 대기 벽시계 ms.
static double rifeRun(RifeEngine *e) {
    MPSCommandBuffer *cb = [MPSCommandBuffer commandBufferFromCommandQueue:e->q];
    double t = rifeNowMs();
    [e->exe encodeToCommandBuffer:cb inputsArray:@[e->t0, e->t1] resultsArray:@[e->to] executionDescriptor:nil];
    [cb commit]; [cb waitUntilCompleted];
    if (cb.error) { fprintf(stderr, "RIFE 실행 오류: %s\n", cb.error.description.UTF8String); exit(7); }
    return rifeNowMs() - t;
}

// ───────── CPU 기준 구현(PyTorch 의미, NCHW float32, 배치 1) ─────────
typedef struct { float *d; int c, h, w; } RT;
static RT rtNew(int c, int h, int w) { RT t = { calloc((size_t)c * h * w, 4), c, h, w }; return t; }
static float *rtAt(RT t, int c, int y, int x) { return t.d + ((size_t)c * t.h + y) * t.w + x; }

static RT refConv(RT x, const float *W, const float *B, int cout, int k, int s, BOOL relu) {
    int p = k / 2, Ho = (x.h + 2 * p - k) / s + 1, Wo = (x.w + 2 * p - k) / s + 1;
    RT y = rtNew(cout, Ho, Wo);
    dispatch_apply(cout, DISPATCH_APPLY_AUTO, ^(size_t o) {
        for (int oy = 0; oy < Ho; oy++) for (int ox = 0; ox < Wo; ox++) {
            double acc = B[o];
            for (int i = 0; i < x.c; i++) for (int kh = 0; kh < k; kh++) {
                int iy = oy * s - p + kh; if (iy < 0 || iy >= x.h) continue;
                for (int kw = 0; kw < k; kw++) { int ix = ox * s - p + kw; if (ix < 0 || ix >= x.w) continue;
                    acc += (double)*rtAt(x, i, iy, ix) * W[(((size_t)o * x.c + i) * k + kh) * k + kw]; }
            }
            float v = (float)acc; *rtAt(y, (int)o, oy, ox) = relu && v < 0 ? 0.2f * v : v;
        }
    });
    return y;
}
static RT refDeconv(RT x, const float *W, const float *B, int cout) {   // k4 s2 p1, W[I][O][4][4]
    RT y = rtNew(cout, x.h * 2, x.w * 2);
    dispatch_apply(cout, DISPATCH_APPLY_AUTO, ^(size_t o) {
        for (int oy = 0; oy < y.h; oy++) for (int ox = 0; ox < y.w; ox++) {
            double acc = B[o];
            for (int kh = 0; kh < 4; kh++) { int t = oy + 1 - kh; if (t < 0 || (t & 1)) continue; int iy = t / 2; if (iy >= x.h) continue;
                for (int kw = 0; kw < 4; kw++) { int u = ox + 1 - kw; if (u < 0 || (u & 1)) continue; int ix = u / 2; if (ix >= x.w) continue;
                    for (int i = 0; i < x.c; i++) acc += (double)*rtAt(x, i, iy, ix) * W[(((size_t)i * cout + o) * 4 + kh) * 4 + kw]; } }
            *rtAt(y, (int)o, oy, ox) = (float)acc;
        }
    });
    return y;
}
// F.interpolate(bilinear, align_corners=False, scale_factor=f): 원본 좌표 = (dst+0.5)/f - 0.5, 0 미만은 0.
static RT refResize(RT x, int oh, int ow) {
    if (oh == x.h && ow == x.w) { RT y = rtNew(x.c, oh, ow); memcpy(y.d, x.d, (size_t)x.c * oh * ow * 4); return y; }
    RT y = rtNew(x.c, oh, ow); double ry = (double)x.h / oh, rx = (double)x.w / ow;
    for (int c = 0; c < x.c; c++) for (int oy = 0; oy < oh; oy++) {
        double sy = fmax((oy + 0.5) * ry - 0.5, 0); int y0 = (int)sy, y1 = MIN(y0 + 1, x.h - 1); double ly = sy - y0;
        for (int ox = 0; ox < ow; ox++) {
            double sx = fmax((ox + 0.5) * rx - 0.5, 0); int x0 = (int)sx, x1 = MIN(x0 + 1, x.w - 1); double lx = sx - x0;
            *rtAt(y, c, oy, ox) = (float)((1 - ly) * ((1 - lx) * *rtAt(x, c, y0, x0) + lx * *rtAt(x, c, y0, x1)) + ly * ((1 - lx) * *rtAt(x, c, y1, x0) + lx * *rtAt(x, c, y1, x1)));
        }
    }
    return y;
}
// grid_sample(bilinear, border, align_corners=True) with 픽셀 흐름 (flow 채널 fc = x, fc+1 = y)
static RT refWarp(RT x, RT fl, int fc) {
    RT y = rtNew(x.c, x.h, x.w);
    for (int py = 0; py < x.h; py++) for (int px = 0; px < x.w; px++) {
        double sx = fmin(fmax(px + *rtAt(fl, fc, py, px), 0), x.w - 1), sy = fmin(fmax(py + *rtAt(fl, fc + 1, py, px), 0), x.h - 1);
        int x0 = (int)floor(sx), y0 = (int)floor(sy), x1 = MIN(x0 + 1, x.w - 1), y1 = MIN(y0 + 1, x.h - 1); double lx = sx - x0, ly = sy - y0;
        for (int c = 0; c < x.c; c++)
            *rtAt(y, c, py, px) = (float)((1 - ly) * ((1 - lx) * *rtAt(x, c, y0, x0) + lx * *rtAt(x, c, y0, x1)) + ly * ((1 - lx) * *rtAt(x, c, y1, x0) + lx * *rtAt(x, c, y1, x1)));
    }
    return y;
}
static RT refCat(RT *ts, int n) {
    int c = 0; for (int i = 0; i < n; i++) c += ts[i].c;
    RT y = rtNew(c, ts[0].h, ts[0].w); size_t plane = (size_t)y.h * y.w, off = 0;
    for (int i = 0; i < n; i++) { memcpy(y.d + off * plane, ts[i].d, ts[i].c * plane * 4); off += ts[i].c; }
    return y;
}
static RT refSlice(RT x, int s, int l) { RT y = rtNew(l, x.h, x.w); memcpy(y.d, x.d + (size_t)s * x.h * x.w, (size_t)l * x.h * x.w * 4); return y; }
static RT refPixelShuffle(RT x) {   // PyTorch PixelShuffle(2)
    RT y = rtNew(x.c / 4, x.h * 2, x.w * 2);
    for (int c = 0; c < y.c; c++) for (int h = 0; h < x.h; h++) for (int w = 0; w < x.w; w++) for (int dy = 0; dy < 2; dy++) for (int dx = 0; dx < 2; dx++)
        *rtAt(y, c, h * 2 + dy, w * 2 + dx) = *rtAt(x, c * 4 + dy * 2 + dx, h, w);
    return y;
}
static void rtFree(RT t) { free(t.d); }

static RT refHead(RifeWeights *w, RT x) {
    RT a = refConv(x, rifeW(w, @"encode.cnn0.weight", -1), rifeW(w, @"encode.cnn0.bias", -1), 16, 3, 2, YES);
    RT b = refConv(a, rifeW(w, @"encode.cnn1.weight", -1), rifeW(w, @"encode.cnn1.bias", -1), 16, 3, 1, YES);
    RT c = refConv(b, rifeW(w, @"encode.cnn2.weight", -1), rifeW(w, @"encode.cnn2.bias", -1), 16, 3, 1, YES);
    RT d = refDeconv(c, rifeW(w, @"encode.cnn3.weight", -1), rifeW(w, @"encode.cnn3.bias", -1), 4);
    rtFree(a); rtFree(b); rtFree(c); return d;
}

// img0, img1: 3×PH×PW(패딩된 크기). 반환: 합성 결과 3×PH×PW.
static RT refForward(RifeWeights *w, int lite, double scale, RT img0, RT img1) {
    int PH = img0.h, PW = img0.w;
    RT f0 = refHead(w, img0), f1 = refHead(w, img1), ts = rtNew(1, PH, PW);
    for (size_t i = 0; i < (size_t)PH * PW; i++) ts.d[i] = 0.5f;
    RT flow = { 0 }, mask = { 0 }, feat = { 0 }, wi0 = img0, wi1 = img1;
    for (int bi = 0; bi < 5; bi++) {
        int s = rifeBlockScale(lite, scale, bi), c = rifeChannels(lite, bi);
        RT x;
        if (!flow.d) { RT l[5] = { img0, img1, f0, f1, ts }; x = refCat(l, 5); }
        else { RT wf0 = refWarp(f0, flow, 0), wf1 = refWarp(f1, flow, 2); RT l[7] = { wi0, wi1, wf0, wf1, ts, mask, feat }; x = refCat(l, 7); rtFree(wf0); rtFree(wf1); }
        RT xs = refResize(x, PH / s, PW / s); rtFree(x);
        if (flow.d) {
            RT fs = refResize(flow, PH / s, PW / s); for (size_t i = 0; i < (size_t)fs.c * fs.h * fs.w; i++) fs.d[i] /= s;
            RT l[2] = { xs, fs }; RT t = refCat(l, 2); rtFree(xs); rtFree(fs); xs = t;
        }
        NSString *p = [NSString stringWithFormat:@"block%d", bi];
        #define WN(suf) rifeW(w, [p stringByAppendingString:@suf], -1)
        RT a = refConv(xs, WN(".conv0.0.0.weight"), WN(".conv0.0.0.bias"), c / 2, 3, 2, YES);
        RT f = refConv(a, WN(".conv0.1.0.weight"), WN(".conv0.1.0.bias"), c, 3, 2, YES); rtFree(a); rtFree(xs);
        for (int r = 0; r < 8; r++) {
            NSString *q = [NSString stringWithFormat:@"block%d.convblock.%d", bi, r];
            RT y = refConv(f, rifeW(w, [q stringByAppendingString:@".conv.weight"], -1), rifeW(w, [q stringByAppendingString:@".conv.bias"], -1), c, 3, 1, NO);
            const float *beta = rifeW(w, [q stringByAppendingString:@".beta"], c);
            for (int ch = 0; ch < c; ch++) for (size_t k = 0; k < (size_t)f.h * f.w; k++) {
                float v = y.d[ch * (size_t)f.h * f.w + k] * beta[ch] + f.d[ch * (size_t)f.h * f.w + k];
                f.d[ch * (size_t)f.h * f.w + k] = v < 0 ? 0.2f * v : v;
            }
            rtFree(y);
        }
        RT dc = refDeconv(f, WN(".lastconv.0.weight"), WN(".lastconv.0.bias"), 52); rtFree(f);
        #undef WN
        RT ps = refPixelShuffle(dc); rtFree(dc);
        RT t = refResize(ps, PH, PW); rtFree(ps);
        RT fd = refSlice(t, 0, 4); for (size_t i = 0; i < (size_t)4 * PH * PW; i++) fd.d[i] *= s;
        if (flow.d) { for (size_t i = 0; i < (size_t)4 * PH * PW; i++) flow.d[i] += fd.d[i]; rtFree(fd); } else flow = fd;
        if (mask.d) { rtFree(mask); rtFree(feat); }
        mask = refSlice(t, 4, 1); feat = refSlice(t, 5, 8); rtFree(t);
        if (wi0.d != img0.d) { rtFree(wi0); rtFree(wi1); }
        wi0 = refWarp(img0, flow, 0); wi1 = refWarp(img1, flow, 2);
    }
    RT out = rtNew(3, PH, PW);
    for (int c = 0; c < 3; c++) for (size_t k = 0; k < (size_t)PH * PW; k++) {
        float m = 1.f / (1.f + expf(-mask.d[k]));
        out.d[c * (size_t)PH * PW + k] = wi0.d[c * (size_t)PH * PW + k] * m + wi1.d[c * (size_t)PH * PW + k] * (1 - m);
    }
    rtFree(f0); rtFree(f1); rtFree(ts); rtFree(flow); rtFree(mask); rtFree(feat); rtFree(wi0); rtFree(wi1);
    return out;
}

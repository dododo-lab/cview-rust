// RIFE 1080p 복제본(rife-mpsgraph.m)의 시간이 어디서 나는지 보는 연산별 미세 측정.
// 연산 하나만 든 MPSGraph 를 만들어 N회 동기 실행하고 1회 시간(p50)을 낸다. 빈 그래프 시간도 함께 낸다.
//   ./mps-ops-micro [--dtype f16|f32] [--iters N]
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <MetalPerformanceShaders/MetalPerformanceShaders.h>
#import <MetalPerformanceShadersGraph/MetalPerformanceShadersGraph.h>
#include <mach/mach_time.h>
#include <math.h>

static double nowMs(void) { static mach_timebase_info_data_t tb; if (!tb.denom) mach_timebase_info(&tb); return (double)mach_absolute_time() * tb.numer / tb.denom / 1e6; }
static int cmpd(const void *a, const void *b) { double x = *(double *)a, y = *(double *)b; return x < y ? -1 : x > y; }
static double p50(double *v, int n) { qsort(v, n, sizeof(double), cmpd); return v[n / 2]; }

static id<MTLDevice> gDev; static id<MTLCommandQueue> gQ; static MPSDataType gDt = MPSDataTypeFloat16; static int gIters = 30;

static MPSGraphTensor *weights(MPSGraph *g, NSArray *shape) {
    long n = 1; for (NSNumber *s in shape) n *= s.longValue;
    NSMutableData *d = [NSMutableData dataWithLength:n * (gDt == MPSDataTypeFloat16 ? 2 : 4)];
    return [g constantWithData:d shape:shape dataType:gDt];
}

// feeds: 입력 shape 목록. build: 입력 텐서들 → 출력 텐서.
static double timeGraph(NSArray<NSArray *> *inShapes, MPSGraphTensor *(^build)(MPSGraph *, NSArray<MPSGraphTensor *> *)) {
    MPSGraph *g = [MPSGraph new];
    NSMutableArray *phs = [NSMutableArray new]; NSMutableDictionary *feeds = [NSMutableDictionary new]; NSMutableArray *datas = [NSMutableArray new];
    for (NSArray *s in inShapes) {
        MPSGraphTensor *p = [g placeholderWithShape:s dataType:gDt name:nil];
        [phs addObject:p]; feeds[p] = [[MPSGraphShapedType alloc] initWithShape:s dataType:gDt];
        long n = 1; for (NSNumber *x in s) n *= x.longValue;
        id<MTLBuffer> b = [gDev newBufferWithLength:n * (gDt == MPSDataTypeFloat16 ? 2 : 4) options:MTLResourceStorageModePrivate];
        [datas addObject:[[MPSGraphTensorData alloc] initWithMTLBuffer:b shape:s dataType:gDt]];
    }
    MPSGraphTensor *out = build(g, phs);
    MPSGraphCompilationDescriptor *cd = [MPSGraphCompilationDescriptor new]; cd.waitForCompilationCompletion = YES;
    MPSGraphExecutable *exe = [g compileWithDevice:[MPSGraphDevice deviceWithMTLDevice:gDev] feeds:feeds targetTensors:@[out] targetOperations:nil compilationDescriptor:cd];
    long n = 1; for (NSNumber *x in out.shape) n *= x.longValue;
    id<MTLBuffer> ob = [gDev newBufferWithLength:n * (gDt == MPSDataTypeFloat16 ? 2 : 4) options:MTLResourceStorageModePrivate];
    MPSGraphTensorData *od = [[MPSGraphTensorData alloc] initWithMTLBuffer:ob shape:out.shape dataType:gDt];
    double *ms = calloc(gIters, sizeof(double));
    for (int it = -5; it < gIters; it++) {
        MPSCommandBuffer *cb = [MPSCommandBuffer commandBufferFromCommandQueue:gQ];
        double t = nowMs();
        [exe encodeToCommandBuffer:cb inputsArray:datas resultsArray:@[od] executionDescriptor:nil];
        [cb commit]; [cb waitUntilCompleted];
        if (it >= 0) ms[it] = nowMs() - t;
    }
    double r = p50(ms, gIters); free(ms); return r;
}

static MPSGraphConvolution2DOpDescriptor *convDesc(int s, int pad) {
    return [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:s strideInY:s dilationRateInX:1 dilationRateInY:1 groups:1
        paddingLeft:pad paddingRight:pad paddingTop:pad paddingBottom:pad paddingStyle:MPSGraphPaddingStyleExplicit
        dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
}

static void convCase(const char *name, int H, int W, int cin, int cout, int s) {
    double ms = timeGraph(@[@[@1, @(H), @(W), @(cin)]], ^(MPSGraph *g, NSArray *in) {
        return [g convolution2DWithSourceTensor:in[0] weightsTensor:weights(g, @[@3, @3, @(cin), @(cout)]) descriptor:convDesc(s, 1) name:nil];
    });
    double gmac = (double)(H / s) * (W / s) * cin * cout * 9 / 1e9;
    printf("  %-34s %7.3f ms  (%.2f GMAC, %.2f TFLOPS)\n", name, ms, gmac, 2 * gmac / ms);
}
static void deconvCase(const char *name, int H, int W, int cin, int cout) {
    double ms = timeGraph(@[@[@1, @(H), @(W), @(cin)]], ^(MPSGraph *g, NSArray *in) {
        return [g convolutionTranspose2DWithSourceTensor:in[0] weightsTensor:weights(g, @[@4, @4, @(cout), @(cin)])
                                             outputShape:@[@1, @(H * 2), @(W * 2), @(cout)] descriptor:convDesc(2, 1) name:nil];
    });
    double gmac = (double)H * W * cin * cout * 16 / 1e9;
    printf("  %-34s %7.3f ms  (%.2f GMAC, %.2f TFLOPS)\n", name, ms, gmac, 2 * gmac / ms);
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        for (int i = 1; i < argc; i++) {
            if (!strcmp(argv[i], "--dtype") && i + 1 < argc) { gDt = !strcmp(argv[++i], "f32") ? MPSDataTypeFloat32 : MPSDataTypeFloat16; }
            else if (!strcmp(argv[i], "--iters") && i + 1 < argc) gIters = atoi(argv[++i]);
        }
        gDev = MTLCreateSystemDefaultDevice(); gQ = [gDev newCommandQueue];
        int H = 1088, W = 1920;
        printf("# %s dtype=%s, 1회 p50(동기 실행, 빈 그래프 포함)\n", gDev.name.UTF8String, gDt == MPSDataTypeFloat16 ? "f16" : "f32");
        printf("  %-34s %7.3f ms\n", "빈 그래프(작은 덧셈)", timeGraph(@[@[@1, @8, @8, @4]], ^(MPSGraph *g, NSArray *in) { return [g additionWithPrimaryTensor:in[0] secondaryTensor:in[0] name:nil]; }));
        printf("[Head — 영상 1장]\n");
        convCase("conv 3→16 s2 @1920x1088", H, W, 3, 16, 2);
        convCase("conv 16→16 @960x544", H / 2, W / 2, 16, 16, 1);
        deconvCase("deconv 16→4 @960x544→full", H / 2, W / 2, 16, 4);
        printf("[블록 4(scale 1, c=32) 내부]\n");
        convCase("conv0a 28→16 s2 @1920x1088", H, W, 28, 16, 2);
        convCase("conv0b 16→32 s2 @960x544", H / 2, W / 2, 16, 32, 2);
        convCase("ResConv 32→32 @480x272", H / 4, W / 4, 32, 32, 1);
        deconvCase("deconv 32→52 @480x272", H / 4, W / 4, 32, 52);
        printf("[블록 3(scale 2, c=64) 내부]\n");
        convCase("ResConv 64→64 @240x136", H / 8, W / 8, 64, 64, 1);
        printf("[블록 0(scale 16, c=192) 내부]\n");
        convCase("ResConv 192→192 @30x17", H / 64, W / 64, 192, 192, 1);
        printf("[전체 해상도 보조 연산]\n");
        printf("  %-34s %7.3f ms\n", "concat 3+3+4+4+1+1+8 → 24ch", timeGraph(@[@[@1, @(H), @(W), @3], @[@1, @(H), @(W), @3], @[@1, @(H), @(W), @4], @[@1, @(H), @(W), @4], @[@1, @(H), @(W), @1], @[@1, @(H), @(W), @1], @[@1, @(H), @(W), @8]],
            ^(MPSGraph *g, NSArray *in) { return [g concatTensors:in dimension:3 name:nil]; }));
        printf("  %-34s %7.3f ms\n", "resize 28ch full → 1/2", timeGraph(@[@[@1, @(H), @(W), @28]], ^(MPSGraph *g, NSArray *in) {
            return [g resizeTensor:in[0] size:@[@(H / 2), @(W / 2)] mode:MPSGraphResizeBilinear centerResult:YES alignCorners:NO layout:MPSGraphTensorNamedDataLayoutNHWC name:nil]; }));
        printf("  %-34s %7.3f ms\n", "resize 13ch 1/2 → full", timeGraph(@[@[@1, @(H / 2), @(W / 2), @13]], ^(MPSGraph *g, NSArray *in) {
            return [g resizeTensor:in[0] size:@[@(H), @(W)] mode:MPSGraphResizeBilinear centerResult:YES alignCorners:NO layout:MPSGraphTensorNamedDataLayoutNHWC name:nil]; }));
        printf("  %-34s %7.3f ms\n", "depthToSpace 52ch→13ch ×2", timeGraph(@[@[@1, @(H / 4), @(W / 4), @52]], ^(MPSGraph *g, NSArray *in) {
            return [g depthToSpace2DTensor:in[0] widthAxis:2 heightAxis:1 depthAxis:3 blockSize:2 usePixelShuffleOrder:YES name:nil]; }));
        printf("  %-34s %7.3f ms\n", "slice 13ch → 4ch", timeGraph(@[@[@1, @(H), @(W), @13]], ^(MPSGraph *g, NSArray *in) { return [g sliceTensor:in[0] dimension:3 start:0 length:4 name:nil]; }));
        printf("  %-34s %7.3f ms\n", "grid_sample 3ch (흐름 2ch)", timeGraph(@[@[@1, @(H), @(W), @3], @[@1, @(H), @(W), @2]], ^(MPSGraph *g, NSArray *in) {
            return [g sampleGridWithSourceTensor:in[0] coordinateTensor:in[1] layout:MPSGraphTensorNamedDataLayoutNHWC normalizeCoordinates:NO relativeCoordinates:YES alignCorners:NO paddingMode:MPSGraphPaddingModeClampToEdge samplingMode:MPSGraphResizeBilinear constantValue:0 name:nil]; }));
        printf("  %-34s %7.3f ms\n", "grid_sample 4ch (흐름 2ch)", timeGraph(@[@[@1, @(H), @(W), @4], @[@1, @(H), @(W), @2]], ^(MPSGraph *g, NSArray *in) {
            return [g sampleGridWithSourceTensor:in[0] coordinateTensor:in[1] layout:MPSGraphTensorNamedDataLayoutNHWC normalizeCoordinates:NO relativeCoordinates:YES alignCorners:NO paddingMode:MPSGraphPaddingModeClampToEdge samplingMode:MPSGraphResizeBilinear constantValue:0 name:nil]; }));
        return 0;
    }
}

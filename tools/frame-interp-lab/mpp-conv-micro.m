// Metal 4 MetalPerformancePrimitives(MPP) tensor_ops::convolution2d 를 MSL 셰이더 안에서 돌려
// 같은 3×3 합성곱을 MPSGraph 와 비교하는 미세 측정. 셰이더는 OS 런타임 컴파일러로 빌드한다
// (Xcode 의 오프라인 Metal Toolchain 이 없어도 된다).
//   ./mpp-conv-micro [--iters N]
// 결과: 층마다 MPP 시간, MPSGraph 시간, 두 출력의 최대 차이(같은 가중치·입력).
#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#import <MetalPerformanceShaders/MetalPerformanceShaders.h>
#import <MetalPerformanceShadersGraph/MetalPerformanceShadersGraph.h>
#include <mach/mach_time.h>
#include <math.h>

static double nowMs(void) { static mach_timebase_info_data_t tb; if (!tb.denom) mach_timebase_info(&tb); return (double)mach_absolute_time() * tb.numer / tb.denom / 1e6; }
static int cmpd(const void *a, const void *b) { double x = *(double *)a, y = *(double *)b; return x < y ? -1 : x > y; }
static double p50(double *v, int n) { qsort(v, n, sizeof(double), cmpd); return v[n / 2]; }

// TC = 출력 채널 타일, TW×TH = 공간 타일. 입력은 미리 1픽셀 패딩된 NHWC(=MSL 차원 순서 C,W,H,N).
static NSString *kSrc = @
"#include <metal_stdlib>\n"
"#include <metal_tensor>\n"
"#include <MetalPerformancePrimitives/MetalPerformancePrimitives.h>\n"
"using namespace metal;\n"
"using namespace mpp::tensor_ops;\n"
"kernel void conv3x3(device half *src [[buffer(0)]], device half *wts [[buffer(1)]], device half *dst [[buffer(2)]],\n"
"                    constant int4 &dims [[buffer(3)]], uint2 tg [[threadgroup_position_in_grid]]) {\n"
"  int C = dims.x, W = dims.y, H = dims.z, O = dims.w;\n"
"  auto S = tensor<device half, dextents<int32_t, 4>, tensor_inline>(src, dextents<int32_t, 4>(C, W + 2, H + 2, 1));\n"
"  auto K = tensor<device half, dextents<int32_t, 4>, tensor_inline>(wts, dextents<int32_t, 4>(O, C, 3, 3));\n"
"  auto D = tensor<device half, dextents<int32_t, 4>, tensor_inline>(dst, dextents<int32_t, 4>(O, W, H, 1));\n"
"  constexpr auto desc = convolution2d_descriptor(int4(TC, TW, TH, 1), int4(CIN, TW + 2, TH + 2, 1), int2(3, 3));\n"
"  convolution2d<desc, execution_simdgroups<NSG>> op;\n"
"#if MODE == 1\n"
"  op.set_offsets(int2(int(tg.x) * TW, int(tg.y) * TH));\n"
"  op.run(S, K, D);\n"
"#else\n"
"  auto s = S.slice(0, int(tg.x) * TW, int(tg.y) * TH, 0);\n"
"  auto d = D.slice(0, int(tg.x) * TW, int(tg.y) * TH, 0);\n"
"  op.run(s, K, d);\n"
"#endif\n"
"}\n";

static id<MTLDevice> gDev; static id<MTLCommandQueue> gQ; static int gIters = 30;

static void fillHalf(id<MTLBuffer> b, size_t n, float scale, unsigned seed) {
    _Float16 *p = b.contents; uint32_t r = seed;
    for (size_t i = 0; i < n; i++) { r = r * 1664525u + 1013904223u; p[i] = (_Float16)(((r >> 8) & 0xFFFF) / 65535.f * 2 - 1) * scale; }
}

static void runCase(int H, int W, int C, int O, int TC, int TW, int TH, int NSG) {
    NSError *err = nil;
    MTLCompileOptions *opt = [MTLCompileOptions new];
    opt.languageVersion = MTLLanguageVersion4_0;
    opt.preprocessorMacros = @{ @"TC": @(TC), @"TW": @(TW), @"TH": @(TH), @"CIN": @(C), @"NSG": @(NSG), @"MODE": @(getenv("MPP_MODE") ? atoi(getenv("MPP_MODE")) : 0) };
    double tc = nowMs();
    id<MTLLibrary> lib = [gDev newLibraryWithSource:kSrc options:opt error:&err];
    if (!lib) { printf("  MPP 셰이더 컴파일 실패: %s\n", err.localizedDescription.UTF8String); exit(3); }
    id<MTLComputePipelineState> ps = [gDev newComputePipelineStateWithFunction:[lib newFunctionWithName:@"conv3x3"] error:&err];
    if (!ps) { printf("  파이프라인 실패: %s\n", err.localizedDescription.UTF8String); exit(3); }
    double compileMs = nowMs() - tc;
    size_t nSrc = (size_t)(H + 2) * (W + 2) * C, nW = 9 * (size_t)C * O, nDst = (size_t)H * W * O;
    id<MTLBuffer> src = [gDev newBufferWithLength:nSrc * 2 options:MTLResourceStorageModeShared];
    id<MTLBuffer> wts = [gDev newBufferWithLength:nW * 2 options:MTLResourceStorageModeShared];
    id<MTLBuffer> dst = [gDev newBufferWithLength:nDst * 2 options:MTLResourceStorageModeShared];
    fillHalf(src, nSrc, 1.0f, 1); fillHalf(wts, nW, sqrtf(1.f / (9 * C)), 2);
    // 패딩 테두리는 0
    _Float16 *ps16 = src.contents;
    for (int y = 0; y < H + 2; y++) for (int x = 0; x < W + 2; x++) if (!y || !x || y == H + 1 || x == W + 1) for (int c = 0; c < C; c++) ps16[((size_t)y * (W + 2) + x) * C + c] = 0;
    int dims[4] = { C, W, H, O };
    MTLSize grid = MTLSizeMake((W + TW - 1) / TW, (H + TH - 1) / TH, 1);
    MTLSize tpg = MTLSizeMake(ps.threadExecutionWidth * NSG, 1, 1);
    double *ms = calloc(gIters, sizeof(double));
    for (int it = -5; it < gIters; it++) {
        id<MTLCommandBuffer> cb = [gQ commandBuffer];
        id<MTLComputeCommandEncoder> e = [cb computeCommandEncoder];
        [e setComputePipelineState:ps];
        [e setBuffer:src offset:0 atIndex:0]; [e setBuffer:wts offset:0 atIndex:1]; [e setBuffer:dst offset:0 atIndex:2];
        [e setBytes:dims length:sizeof(dims) atIndex:3];
        // O 가 TC 보다 크면 출력 채널 방향으로도 나눠야 하지만, 여기서는 TC == O 로만 잰다.
        [e dispatchThreadgroups:grid threadsPerThreadgroup:tpg];
        [e endEncoding];
        double t = nowMs(); [cb commit]; [cb waitUntilCompleted];
        if (cb.error) { printf("  실행 오류: %s\n", cb.error.localizedDescription.UTF8String); exit(4); }
        if (it >= 0) ms[it] = (cb.GPUEndTime - cb.GPUStartTime) * 1e3;
    }
    double mppMs = p50(ms, gIters);

    // 같은 연산을 MPSGraph 로(패딩 없는 입력 + 명시적 패딩 1) 실행해 시간·값을 비교한다.
    MPSGraph *g = [MPSGraph new];
    MPSGraphTensor *in = [g placeholderWithShape:@[@1, @(H + 2), @(W + 2), @(C)] dataType:MPSDataTypeFloat16 name:nil];
    MPSGraphTensor *wt = [g placeholderWithShape:@[@3, @3, @(C), @(O)] dataType:MPSDataTypeFloat16 name:nil];
    MPSGraphConvolution2DOpDescriptor *d = [MPSGraphConvolution2DOpDescriptor descriptorWithStrideInX:1 strideInY:1 dilationRateInX:1 dilationRateInY:1 groups:1
        paddingLeft:0 paddingRight:0 paddingTop:0 paddingBottom:0 paddingStyle:MPSGraphPaddingStyleExplicit dataLayout:MPSGraphTensorNamedDataLayoutNHWC weightsLayout:MPSGraphTensorNamedDataLayoutHWIO];
    MPSGraphTensor *out = [g convolution2DWithSourceTensor:in weightsTensor:wt descriptor:d name:nil];
    MPSGraphCompilationDescriptor *cd = [MPSGraphCompilationDescriptor new]; cd.waitForCompilationCompletion = YES;
    MPSGraphExecutable *exe = [g compileWithDevice:[MPSGraphDevice deviceWithMTLDevice:gDev]
        feeds:@{ in: [[MPSGraphShapedType alloc] initWithShape:in.shape dataType:MPSDataTypeFloat16], wt: [[MPSGraphShapedType alloc] initWithShape:wt.shape dataType:MPSDataTypeFloat16] }
        targetTensors:@[out] targetOperations:nil compilationDescriptor:cd];
    id<MTLBuffer> ref = [gDev newBufferWithLength:nDst * 2 options:MTLResourceStorageModeShared];
    NSArray *ins = @[[[MPSGraphTensorData alloc] initWithMTLBuffer:src shape:in.shape dataType:MPSDataTypeFloat16],
                     [[MPSGraphTensorData alloc] initWithMTLBuffer:wts shape:wt.shape dataType:MPSDataTypeFloat16]];
    MPSGraphTensorData *od = [[MPSGraphTensorData alloc] initWithMTLBuffer:ref shape:@[@1, @(H), @(W), @(O)] dataType:MPSDataTypeFloat16];
    for (int it = -5; it < gIters; it++) {
        MPSCommandBuffer *cb = [MPSCommandBuffer commandBufferFromCommandQueue:gQ];
        double t = nowMs();
        [exe encodeToCommandBuffer:cb inputsArray:ins resultsArray:@[od] executionDescriptor:nil];
        [cb commit]; [cb waitUntilCompleted];
        if (it >= 0) ms[it] = nowMs() - t;
    }
    double mpsMs = p50(ms, gIters);
    double maxd = 0; _Float16 *a = dst.contents, *b = ref.contents;
    for (size_t i = 0; i < nDst; i++) { double dd = fabs((double)a[i] - (double)b[i]); if (dd > maxd) maxd = dd; }
    if (getenv("MPP_CPU_CHECK")) {   // 작은 크기에서 CPU 정답과 둘 다 비교(인덱스 규약 확인용)
        _Float16 *xs = src.contents, *ww = wts.contents; double e1 = 0, e2 = 0; long nn = 0;
        for (int y = 0; y < H; y += 3) for (int x = 0; x < W; x += 5) for (int o = 0; o < O; o++) {
            double acc = 0;
            for (int ky = 0; ky < 3; ky++) for (int kx = 0; kx < 3; kx++) for (int c = 0; c < C; c++)
                acc += (double)xs[((size_t)(y + ky) * (W + 2) + (x + kx)) * C + c] * (double)ww[(((size_t)ky * 3 + kx) * C + c) * O + o];
            size_t idx = ((size_t)y * W + x) * O + o;
            e1 = fmax(e1, fabs(acc - (double)a[idx])); e2 = fmax(e2, fabs(acc - (double)b[idx])); nn++;
            if (nn <= 3) printf("    cpu %.4f mpp %.4f mps %.4f\n", acc, (double)a[idx], (double)b[idx]);
        }
        printf("    CPU 대비 최대차: MPP %.4f, MPSGraph %.4f (%ld 표본)\n", e1, e2, nn);
        // 규약 후보: 0 기본, 1 커널 뒤집기, 2 ky/kx 바꿈, 3 가중치 [kh][kw][o][i], 4 [kw][kh][i][o], 5 [o][i][kh][kw]... 출력 위치 이동 dx,dy
        const char *nm[] = {"기본", "커널뒤집기", "ky<->kx", "khkwOI", "OIkhkw", "IOkhkw", "kwkhIO"};
        for (int conv = 0; conv < 7; conv++) for (int dy = -1; dy <= 1; dy++) for (int dx = -1; dx <= 1; dx++) {
            double em = 0; long cnt = 0;
            for (int y = 1; y < H - 1; y += 3) for (int x = 1; x < W - 1; x += 5) for (int o = 0; o < O; o++) {
                double acc = 0;
                for (int ky = 0; ky < 3; ky++) for (int kx = 0; kx < 3; kx++) for (int c = 0; c < C; c++) {
                    int a1 = ky, b1 = kx; size_t wi;
                    if (conv == 1) { a1 = 2 - ky; b1 = 2 - kx; }
                    if (conv == 2) { a1 = kx; b1 = ky; }
                    if (conv == 3) wi = (((size_t)a1 * 3 + b1) * O + o) * C + c;
                    else if (conv == 4) wi = (((size_t)o * C + c) * 3 + a1) * 3 + b1;
                    else if (conv == 5) wi = (((size_t)c * O + o) * 3 + a1) * 3 + b1;
                    else if (conv == 6) wi = (((size_t)b1 * 3 + a1) * C + c) * O + o;
                    else wi = (((size_t)a1 * 3 + b1) * C + c) * O + o;
                    acc += (double)xs[((size_t)(y + dy + ky) * (W + 2) + (x + dx + kx)) * C + c] * (double)ww[wi];
                }
                em = fmax(em, fabs(acc - (double)a[((size_t)y * W + x) * O + o])); cnt++;
            }
            if (conv == 0 && dx == 0 && dy == 0) {
                double et = 0; for (int y = 0; y < TH; y++) for (int x = 0; x < TW; x++) for (int o = 0; o < O; o++) {
                    double acc = 0;
                    for (int ky = 0; ky < 3; ky++) for (int kx = 0; kx < 3; kx++) for (int c = 0; c < C; c++)
                        acc += (double)xs[((size_t)(y + ky) * (W + 2) + (x + kx)) * C + c] * (double)ww[(((size_t)ky * 3 + kx) * C + c) * O + o];
                    et = fmax(et, fabs(acc - (double)a[((size_t)y * W + x) * O + o])); }
                printf("    첫 타일만 최대차 %.4f\n", et);
            }
            if (em < 0.05) printf("    일치 후보: %s dy=%d dx=%d 최대차 %.4f\n", nm[conv], dy, dx, em);
        }
    }
    double gmac = (double)H * W * C * O * 9 / 1e9;
    printf("  %4dx%-4d %3d→%-3d tile %dx%dx%d sg%d: MPP GPU %6.3f ms (%.2f TFLOPS) | MPSGraph 벽시계 %6.3f ms | 최대차 %.4f | 컴파일 %.0f ms\n",
           W, H, C, O, TC, TW, TH, NSG, mppMs, 2 * gmac / mppMs, mpsMs, maxd, compileMs);
    free(ms);
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        for (int i = 1; i < argc; i++) if (!strcmp(argv[i], "--iters") && i + 1 < argc) gIters = atoi(argv[++i]);
        gDev = MTLCreateSystemDefaultDevice(); gQ = [gDev newCommandQueue];
        printf("# %s, Metal4 family=%d, MPP tensor_ops::convolution2d (f16) vs MPSGraph conv\n", gDev.name.UTF8String, (int)[gDev supportsFamily:MTLGPUFamilyMetal4]);
        // RIFE 1080p(1920×1088) 블록 4·3·Head 의 3×3 합성곱 모양
        if (getenv("MPP_CPU_CHECK")) { runCase(16, 32, 16, 16, 16, 16, 8, 4); return 0; }
        runCase(272, 480, 32, 32, 32, 16, 8, 4);
        runCase(272, 480, 32, 32, 32, 32, 8, 4);
        runCase(136, 240, 64, 64, 64, 16, 8, 4);
        runCase(544, 960, 16, 16, 16, 32, 8, 4);
        return 0;
    }
}

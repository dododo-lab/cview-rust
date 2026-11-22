// RIFE IFNet(Practical-RIFE 4.25 / 4.25.lite)을 MPSGraph(Metal GPU)로 돌려 시간을 재고, CPU 기준 구현과 값을 맞춰 본다.
// 그래프 구성·가중치 재배열·CPU 기준 구현은 rife-graph.h 에 있다.
//
// 시간:  ./rife-mpsgraph --size 1920x1080 [--variant 4.25|lite] [--scale 1|0.5] [--dtype f16|f32] [--warp32]
//                         [--fast phase|3x3] [--weights <폴더>] [--iters N]
//        가중치를 주지 않으면 무작위 가중치다(처리 시간은 값과 무관).
// 정합:  ./rife-mpsgraph --check --weights <폴더> [--variant ..] [--size 256x128]
//        같은 입력을 CPU 기준 구현(PyTorch 연산 의미, FP32)과 MPSGraph 의 여러 구성으로 돌려 출력 차이를 낸다.
//        <폴더> 는 rife-export.py 출력(weights.bin, index.json). 가중치는 저장소에 넣지 않는다.
// 분석:  --blocks N(앞 N개 블록만), --skip-warp(워핑을 항등으로) — 원래 구성에서만, 실제 RIFE 가 아님.
//
// 빌드: clang -O2 -fobjc-arc -framework Foundation -framework Metal -framework MetalPerformanceShaders \
//         -framework MetalPerformanceShadersGraph rife-mpsgraph.m -o rife-mpsgraph

#import "rife-graph.h"

static int cmpd(const void *a, const void *b) { double x = *(double *)a, y = *(double *)b; return x < y ? -1 : x > y; }
static double pct(double *v, int n, double p) { qsort(v, n, sizeof(double), cmpd); return v[MIN(n - 1, (int)floor(p * (n - 1) + 0.5))]; }

static const char *cfgName(RifeCfg c) {
    static char buf[128];
    snprintf(buf, sizeof buf, "%s %s scale=%.2f %s%s grid_align=%d", c.lite ? "lite" : "4.25", c.fast == 2 ? "fast=3x3" : c.fast == 1 ? "fast=phase" : "원래구성",
             c.scale, c.dt == MPSDataTypeFloat16 ? "f16" : "f32", c.warp32 ? "+warp32" : "", c.gridAlign);
    return buf;
}

// 정합 시험용 입력: 매끈한 무늬 + 가장자리 + 작은 원, 뒤 프레임은 (dx, dy) 만큼 옮긴 것.
static void makeCheckFrame(float *rgb, int W, int H, double dx, double dy) {
    for (int y = 0; y < H; y++) for (int x = 0; x < W; x++) {
        double u = x - dx, v = y - dy;
        double r = 0.5 + 0.35 * sin(u * 0.11) * cos(v * 0.07), g = 0.5 + 0.3 * sin((u + v) * 0.05), b = (fmod(floor(u / 16) + floor(v / 16), 2) ? 0.8 : 0.2);
        double cx = u - W * 0.4, cy = v - H * 0.5; if (cx * cx + cy * cy < 18 * 18) { r = 0.95; g = 0.1; b = 0.15; }
        rgb[((size_t)y * W + x) * 3] = (float)r; rgb[((size_t)y * W + x) * 3 + 1] = (float)g; rgb[((size_t)y * W + x) * 3 + 2] = (float)b;
    }
}

static int runCheck(id<MTLDevice> dev, RifeWeights *w, int lite, double scale, int W, int H) {
    int mult = rifePadMult(lite, scale), PW = (W + mult - 1) / mult * mult, PH = (H + mult - 1) / mult * mult;
    float *a = malloc((size_t)W * H * 12), *b = malloc((size_t)W * H * 12), *o = malloc((size_t)W * H * 12);
    makeCheckFrame(a, W, H, 0, 0); makeCheckFrame(b, W, H, 6, 2);
    RT i0 = rtNew(3, PH, PW), i1 = rtNew(3, PH, PW);
    for (int y = 0; y < H; y++) for (int x = 0; x < W; x++) for (int c = 0; c < 3; c++) {
        *rtAt(i0, c, y, x) = a[((size_t)y * W + x) * 3 + c]; *rtAt(i1, c, y, x) = b[((size_t)y * W + x) * 3 + c]; }
    double t = rifeNowMs();
    RT ref = refForward(w, lite, scale, i0, i1);
    printf("# 정합 %s scale=%.2f 입력 %dx%d(패딩 %dx%d), 뒤 프레임 (+6, +2)px 이동, CPU 기준 %.0f ms\n", lite ? "4.25.lite" : "4.25", scale, W, H, PW, PH, rifeNowMs() - t);
    // 기준 출력의 성질: 중간 프레임 = 입력을 (+3, +1) 옮긴 것과 가까워야 한다(흐름 부호·순서 점검).
    float *mid = malloc((size_t)W * H * 12); makeCheckFrame(mid, W, H, 3, 1);
    double se = 0, seA = 0, seB = 0; long n = 0;
    for (int y = 8; y < H - 8; y++) for (int x = 8; x < W - 8; x++) for (int c = 0; c < 3; c++) {
        double r = *rtAt(ref, c, y, x), m = mid[((size_t)y * W + x) * 3 + c];
        se += (r - m) * (r - m); seA += (a[((size_t)y * W + x) * 3 + c] - m) * (a[((size_t)y * W + x) * 3 + c] - m);
        double bl = 0.5 * (a[((size_t)y * W + x) * 3 + c] + b[((size_t)y * W + x) * 3 + c]); seB += (bl - m) * (bl - m); n++; }
    printf("  기준 출력 vs 실제 중간(+3,+1) PSNR %.2f dB (앞 프레임 %.2f, 단순 섞기 %.2f)\n", 10 * log10(1 / (se / n)), 10 * log10(1 / (seA / n)), 10 * log10(1 / (seB / n)));
    if (getenv("RIFE_CHECK_BLOCKS")) {   // 원래 구성과 빠른 구성을 블록 수별로 비교(어디서 갈라지나)
        for (int nb = 1; nb <= 5; nb++) for (int fm = 1; fm <= 2; fm++) {
            RifeCfg c0 = rifeDefaultCfg(); c0.lite = lite; c0.scale = scale; c0.dt = MPSDataTypeFloat32; c0.blocks = nb;
            RifeCfg c1 = c0; c1.fast = fm;
            RifeEngine *e0 = rifeEngineNew(dev, W, H, c0, w), *e1 = rifeEngineNew(dev, W, H, c1, w);
            float *o1 = malloc((size_t)W * H * 12);
            rifeSetInput(e0, 0, a); rifeSetInput(e0, 1, b); rifeRun(e0); rifeGetOutput(e0, o);
            rifeSetInput(e1, 0, a); rifeSetInput(e1, 1, b); rifeRun(e1); rifeGetOutput(e1, o1);
            double mx = 0; int mxx = 0, mxy = 0; long big = 0;
            for (int y = 0; y < H; y++) for (int x = 0; x < W; x++) for (int ch = 0; ch < 3; ch++) {
                double d = fabs(o[((size_t)y * W + x) * 3 + ch] - o1[((size_t)y * W + x) * 3 + ch]);
                if (d > 1e-3) big++;
                if (d > mx) { mx = d; mxx = x; mxy = y; } }
            printf("  blocks=%d fast=%d: 최대 차 %.2e @(%d,%d), 1e-3 넘는 값 %ld개\n", nb, fm, mx, mxx, mxy, big);
            free(o1);
        }
        return 0;
    }
    RifeCfg cs[7]; int nc = 0;
    RifeCfg c = rifeDefaultCfg(); c.lite = lite; c.scale = scale;
    c.dt = MPSDataTypeFloat32; c.fast = 0; c.gridAlign = YES; cs[nc++] = c;
    c.gridAlign = NO; cs[nc++] = c; c.gridAlign = YES;
    c.fast = 1; cs[nc++] = c;
    c.fast = 2; cs[nc++] = c;
    c.dt = MPSDataTypeFloat16; c.fast = 0; cs[nc++] = c;
    c.fast = 2; cs[nc++] = c;
    c.warp32 = YES; cs[nc++] = c;
    for (int k = 0; k < nc; k++) {
        RifeEngine *e = rifeEngineNew(dev, W, H, cs[k], w);
        rifeSetInput(e, 0, a); rifeSetInput(e, 1, b); rifeRun(e); rifeGetOutput(e, o);
        double mx = 0, mean = 0; int nonfinite = 0;
        for (int y = 0; y < H; y++) for (int x = 0; x < W; x++) for (int ch = 0; ch < 3; ch++) {
            double v = o[((size_t)y * W + x) * 3 + ch], r = *rtAt(ref, ch, y, x);
            if (!isfinite(v)) { nonfinite++; continue; }
            double d = fabs(v - r); mx = fmax(mx, d); mean += d; }
        printf("  %-44s 최대 차 %.2e  평균 차 %.2e  비유한 %d\n", cfgName(cs[k]), mx, mean / ((double)W * H * 3), nonfinite);
    }
    return 0;
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        int W = 1920, H = 1080, iters = 100, warm = 10; BOOL check = NO;
        NSString *weightsDir = nil;
        RifeCfg cfg = rifeDefaultCfg();
        for (int i = 1; i < argc; i++) {
            NSString *k = @(argv[i]); NSString *v = i + 1 < argc ? @(argv[i + 1]) : @"";
            if ([k isEqual:@"--size"]) { NSArray *p = [v componentsSeparatedByString:@"x"]; W = [p[0] intValue]; H = [p[1] intValue]; i++; }
            else if ([k isEqual:@"--variant"]) { cfg.lite = [v isEqual:@"lite"]; i++; }
            else if ([k isEqual:@"--scale"]) { cfg.scale = v.doubleValue; i++; }
            else if ([k isEqual:@"--dtype"]) { cfg.dt = [v isEqual:@"f32"] ? MPSDataTypeFloat32 : MPSDataTypeFloat16; i++; }
            else if ([k isEqual:@"--warp32"]) { cfg.warp32 = YES; }
            else if ([k isEqual:@"--iters"]) { iters = v.intValue; i++; }
            else if ([k isEqual:@"--fast"]) { cfg.fast = [v isEqual:@"3x3"] ? 2 : 1; i++; }
            else if ([k isEqual:@"--blocks"]) { cfg.blocks = v.intValue; i++; }
            else if ([k isEqual:@"--skip-warp"]) { cfg.skipWarp = YES; }
            else if ([k isEqual:@"--grid-align"]) { cfg.gridAlign = v.intValue != 0; i++; }
            else if ([k isEqual:@"--weights"]) { weightsDir = v; i++; }
            else if ([k isEqual:@"--check"]) { check = YES; }
        }
        id<MTLDevice> dev = MTLCreateSystemDefaultDevice();
        RifeWeights *w = rifeLoadWeights(weightsDir);
        if (check) {
            if (!w) { fprintf(stderr, "--check 에는 --weights 가 필요하다\n"); return 2; }
            if (W == 1920 && H == 1080) { W = 256; H = 128; }
            return runCheck(dev, w, cfg.lite, cfg.scale, W, H);
        }
        if (cfg.skipWarp || cfg.blocks < 5) printf("# 분석 모드: skip_warp=%d blocks=%d (실제 RIFE 아님)\n", cfg.skipWarp, cfg.blocks);
        RifeEngine *e = rifeEngineNew(dev, W, H, cfg, w);
        int sl[5]; for (int i = 0; i < 5; i++) sl[i] = rifeBlockScale(cfg.lite, cfg.scale, i);
        printf("# device=%s metal4=%d variant=%s scale=%.2f ladder=%d/%d/%d/%d/%d c=%d/%d/%d/%d/%d dtype=%s warp32=%d fast=%s weights=%s\n",
               dev.name.UTF8String, (int)[dev supportsFamily:MTLGPUFamilyMetal4], cfg.lite ? "lite" : "4.25", cfg.scale,
               sl[0], sl[1], sl[2], sl[3], sl[4], rifeChannels(cfg.lite, 0), rifeChannels(cfg.lite, 1), rifeChannels(cfg.lite, 2), rifeChannels(cfg.lite, 3), rifeChannels(cfg.lite, 4),
               cfg.dt == MPSDataTypeFloat16 ? "f16" : "f32", cfg.warp32, cfg.fast == 2 ? "3x3" : cfg.fast == 1 ? "phase" : "off", w ? "loaded" : "random");
        printf("# input=%dx%d padded=%dx%d params=%.2fM conv_GMAC=%.1f warps=%d\n", W, H, e->PW, e->PH, e->params / 1e6, e->macs / 1e9, e->warps);
        printf("compile_ms=%.0f\n", e->compileMs);
        float *rgb = malloc((size_t)W * H * 12);
        makeCheckFrame(rgb, W, H, 0, 0); rifeSetInput(e, 0, rgb);
        makeCheckFrame(rgb, W, H, 6, 2); rifeSetInput(e, 1, rgb);
        double *ms = calloc(iters, sizeof(double));
        for (int it = -warm; it < iters; it++) { double t = rifeRun(e); if (it >= 0) ms[it] = t; }
        rifeGetOutput(e, rgb);
        int nonfinite = 0; for (size_t i = 0; i < (size_t)W * H * 3; i += 997) if (!isfinite(rgb[i])) nonfinite++;
        double mean = 0; for (int i = 0; i < iters; i++) mean += ms[i]; mean /= iters;
        printf("wall_ms mean=%.2f p50=%.2f p95=%.2f p99=%.2f max=%.2f  (encode+GPU+대기, %d회)\n", mean,
               pct(ms, iters, 0.5), pct(ms, iters, 0.95), pct(ms, iters, 0.99), pct(ms, iters, 1.0), iters);
        printf("effective_TFLOPS(합성곱 기준)=%.2f  nonfinite_samples=%d\n", 2 * e->macs / (pct(ms, iters, 0.5) * 1e-3) / 1e12, nonfinite);
        return 0;
    }
}

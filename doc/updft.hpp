// updft_refine.cuh
// Upsampled DFT の「細かく求める部分」（行列 DFT）の CUDA 版。整数ピークを求めるところは既存のものを使う前提。
//
// やること（ブロックごと、段ごと）
//   1. E_y(j, ky) = exp(+i·2π·f(ky)·y_j / Ny),  y_j = cy + (j − h)/κ   … カーネル行列（縦）
//      E_x(l, kx) = exp(+i·2π·f(kx)·x_l / Nx),  x_l = cx + (l − h)/κ   … カーネル行列（横）
//   2. T = E_y · R            （M × Nx）
//   3. C = |T · E_xᵀ|         （M × M、相関面の大きさ）
//   4. C の最大の点 (j*, l*) を探し、中心を更新：cy += (j* − h)/κ, cx += (l* − h)/κ
//   を、κ = 10, 100, …, upsample まで繰り返す（upsample = 100 なら 2 段）。
//
// 前提（既存の POC に合わせて変えるところ）
//   ・R：C2C（複素数 → 複素数）の全スペクトル。float2（cufftComplex）。ブロック b の R は R + b·Ny·Nx、行優先（ky が行、kx が列）
//        R2C（横が Nx/2+1 の半分）のときは、全スペクトルに広げてから渡すこと（下の注意を参照）
//   ・R の定義：R = F2·conj(F1)（CPU 版 crossPowerSpectrum と同じ）。逆（F1·conj(F2)）なら、ずれの符号が逆になる
//   ・center：ブロックごとの (cx, cy)。入力は整数ピークを「符号付きのずれ」にしたもの
//        （逆 FFT の出力の添字 i が N/2 以上なら i − N。fftshift した面なら i − N/2）。出力は細かく求めたずれ
//   ・ブロックの大きさ Ny, Nx は 1024 以下（横方向のスレッド数に使う）
//   ・メモリの確保は UpdftWorkspace で事前に行う。updftRefine の中では確保も同期もしない → CUDA グラフに入れられる
//
// 精度：位相の計算だけ double で行い、その後は float。位相は「2·f·d/N（π 単位）」を double で計算して [0, 2) に畳んでから
//       sincospif に渡す（d が大きくても sin/cos の精度が落ちない）
#pragma once
#include <cmath>

#ifdef __CUDACC__
#include <cuda_runtime.h>
#define UPDFT_HD __host__ __device__
#else
// CPU でのテスト用（同じ計算を CPU で確かめるため）
struct float2 { float x, y; };
#define UPDFT_HD
#endif

namespace updft {

// 段の設定（倍率 κ と、片側の点数 h）。CPU 版と同じ：h = ceil(0.8·κ/前の κ)
struct Stage { double kappa; int h; };
inline int makeStages(int upsample, Stage* st, int maxStages = 8)
{
    int n = 0;
    double prevK = 1.0;
    while (prevK < upsample && n < maxStages) {
        const double k = (prevK * 10.0 < upsample) ? prevK * 10.0 : double(upsample);
        st[n].kappa = k;
        st[n].h = static_cast<int>(std::ceil(0.8 * k / prevK));
        prevK = k;
        ++n;
    }
    return n;
}
constexpr int kMaxM = 17;   // 1 段で 10 倍まで → h ≤ 8 → M ≤ 17

UPDFT_HD inline int signedFreq(int k, int n) { return (k < (n + 1) / 2) ? k : k - n; }

// カーネル行列の 1 要素：exp(+i·2π·f(k)·(c + (j − h)/κ) / n)
UPDFT_HD inline float2 kernelElem(int j, int k, int n, double c, int h, double kappa)
{
    const double d = c + (j - h) / kappa;
    double a = 2.0 * signedFreq(k, n) * d / n;   // 位相（π 単位）
    a -= 2.0 * floor(a * 0.5);                   // [0, 2) に畳む
    float s, co;
#ifdef __CUDA_ARCH__
    sincospif(static_cast<float>(a), &s, &co);
#else
    s = static_cast<float>(std::sin(a * M_PI));
    co = static_cast<float>(std::cos(a * M_PI));
#endif
    return float2{co, s};
}

UPDFT_HD inline float2 cmul(float2 a, float2 b) { return float2{a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x}; }

// T(j, kx) = Σ_ky E_y(j, ky) · R(ky, kx)
UPDFT_HD inline float2 rowTransformElem(const float2* Ey, const float2* R, int j, int kx, int Ny, int Nx)
{
    float2 acc{0.f, 0.f};
    for (int ky = 0; ky < Ny; ++ky) {
        const float2 p = cmul(Ey[j * Ny + ky], R[ky * Nx + kx]);
        acc.x += p.x; acc.y += p.y;
    }
    return acc;
}

// |C(j, l)| = |Σ_kx T(j, kx) · E_x(l, kx)|
UPDFT_HD inline float colTransformMag(const float2* T, const float2* Ex, int j, int l, int Nx)
{
    float2 acc{0.f, 0.f};
    for (int kx = 0; kx < Nx; ++kx) {
        const float2 p = cmul(T[j * Nx + kx], Ex[l * Nx + kx]);
        acc.x += p.x; acc.y += p.y;
    }
    return sqrtf(acc.x * acc.x + acc.y * acc.y);
}

#ifdef __CUDACC__
// ---------------------------------------------------------------- CUDA カーネル

// 1. カーネル行列 E（M × n）をブロックごとに作る。中心はデバイスのメモリから読む（CPU に戻らない）
//    axis = 0：縦（center.y、n = Ny） / 1：横（center.x、n = Nx）
__global__ void kBuildKernel(const double2* center, int axis, int n, int M, int h, double kappa, float2* E)
{
    const int b = blockIdx.y;
    const double c = axis ? center[b].x : center[b].y;
    float2* Eb = E + static_cast<size_t>(b) * M * n;
    for (int i = blockIdx.x * blockDim.x + threadIdx.x; i < M * n; i += gridDim.x * blockDim.x)
        Eb[i] = kernelElem(i / n, i % n, n, c, h, kappa);
}

// 2. T = E_y · R。grid = (M, B)、block = Nx スレッド（スレッド kx が R の行を連続で読む）
__global__ void kRowTransform(const float2* Ey, const float2* R, int Ny, int Nx, int M, float2* T)
{
    const int j = blockIdx.x, b = blockIdx.y;
    const size_t rs = static_cast<size_t>(Ny) * Nx;
    for (int kx = threadIdx.x; kx < Nx; kx += blockDim.x)
        T[static_cast<size_t>(b) * M * Nx + j * Nx + kx] =
            rowTransformElem(Ey + static_cast<size_t>(b) * M * Ny, R + b * rs, j, kx, Ny, Nx);
}

// 3＋4. C = |T · E_xᵀ| を計算し、最大の点を探して中心を更新する。grid = B、block = 1 つ（kMaxM² = 289 ≤ 320 スレッド）
__global__ void kColTransformArgmax(const float2* T, const float2* Ex, int Nx, int M, int h, double kappa,
                                    double2* center, float* peak)
{
    __shared__ float sv[kMaxM * kMaxM];
    __shared__ int si[kMaxM * kMaxM];
    const int b = blockIdx.x, t = threadIdx.x, MM = M * M;
    float v = -1.f;
    if (t < MM)
        v = colTransformMag(T + static_cast<size_t>(b) * M * Nx, Ex + static_cast<size_t>(b) * M * Nx, t / M, t % M, Nx);
    if (t < MM) { sv[t] = v; si[t] = t; }
    __syncthreads();
    // 最大値のリダクション（MM ≤ 289 なので 512 から半分ずつ）
    for (int s = 256; s > 0; s >>= 1) {
        if (t < s && t + s < MM && sv[t + s] > sv[t]) { sv[t] = sv[t + s]; si[t] = si[t + s]; }
        __syncthreads();
    }
    if (t == 0) {
        const int j = si[0] / M, l = si[0] % M;
        center[b].y += (j - h) / kappa;
        center[b].x += (l - h) / kappa;
        if (peak) peak[b] = sv[0];
    }
}

// ---------------------------------------------------------------- 呼び出し側

// 作業用のメモリ（最大のバッチ数・大きさで 1 回だけ確保）
struct UpdftWorkspace {
    float2* Ey = nullptr;   // B × kMaxM × Ny
    float2* Ex = nullptr;   // B × kMaxM × Nx
    float2* T = nullptr;    // B × kMaxM × Nx
    void alloc(int B, int Ny, int Nx) {
        cudaMalloc(&Ey, sizeof(float2) * B * kMaxM * Ny);
        cudaMalloc(&Ex, sizeof(float2) * B * kMaxM * Nx);
        cudaMalloc(&T, sizeof(float2) * B * kMaxM * Nx);
    }
    void release() { cudaFree(Ey); cudaFree(Ex); cudaFree(T); Ey = Ex = T = nullptr; }
};

// R（B 個のブロックの相互スペクトル）と、整数ピーク（center）から、細かいずれを求める。
//   center：入力 = 整数ピークの符号付きずれ (x, y)、出力 = 細かく求めたずれ。double2（デバイス）
//   peak  ：最後の段の |相関| の最大値（Σ|R| では割っていない）。不要なら nullptr
// メモリ確保・同期をしないので、stream キャプチャ（CUDA グラフ）の中で呼べる
inline void updftRefine(const float2* R, int B, int Ny, int Nx, int upsample,
                        double2* center, float* peak, UpdftWorkspace& ws, cudaStream_t s = 0)
{
    Stage st[8];
    const int ns = makeStages(upsample, st);
    for (int i = 0; i < ns; ++i) {
        const int h = st[i].h, M = 2 * h + 1;
        const double k = st[i].kappa;
        kBuildKernel<<<dim3((M * Ny + 255) / 256, B), 256, 0, s>>>(center, 0, Ny, M, h, k, ws.Ey);
        kBuildKernel<<<dim3((M * Nx + 255) / 256, B), 256, 0, s>>>(center, 1, Nx, M, h, k, ws.Ex);
        kRowTransform<<<dim3(M, B), (Nx < 1024 ? Nx : 1024), 0, s>>>(ws.Ey, R, Ny, Nx, M, ws.T);
        kColTransformArgmax<<<B, 320, 0, s>>>(ws.T, ws.Ex, Nx, M, h, k, center, (i == ns - 1) ? peak : nullptr);
    }
}
#endif  // __CUDACC__

}  // namespace updft


/ /test_updft_refine_cpu.cpp
// updft_refine.cuh の計算（カーネル行列・行列 DFT・最大値探索・中心の更新）を、GPU なしで CPU で同じ添字の順に実行し、
// 既存の CPU 版 phaseCorrelateUpsampledDft と結果が一致するかを確かめる。
//   g++ -O2 -std=c++17 -I.. test_updft_refine_cpu.cpp $(pkg-config --cflags --libs opencv4)
#include "updft_refine.cuh"
#include "poc_upsampled_dft.hpp"
#include <cstdio>
#include <random>
#include <vector>

// GPU の updftRefine と同じ流れを CPU で（1 ブロック分）
static cv::Point2d refineLikeGpu(const std::vector<float2>& R, int Ny, int Nx, int upsample, double cx, double cy)
{
    updft::Stage st[8];
    const int ns = updft::makeStages(upsample, st);
    for (int i = 0; i < ns; ++i) {
        const int h = st[i].h, M = 2 * h + 1;
        const double k = st[i].kappa;
        std::vector<float2> Ey(M * Ny), Ex(M * Nx), T(M * Nx);
        for (int e = 0; e < M * Ny; ++e) Ey[e] = updft::kernelElem(e / Ny, e % Ny, Ny, cy, h, k);   // kBuildKernel（縦）
        for (int e = 0; e < M * Nx; ++e) Ex[e] = updft::kernelElem(e / Nx, e % Nx, Nx, cx, h, k);   // kBuildKernel（横）
        for (int j = 0; j < M; ++j)                                                                  // kRowTransform
            for (int kx = 0; kx < Nx; ++kx) T[j * Nx + kx] = updft::rowTransformElem(Ey.data(), R.data(), j, kx, Ny, Nx);
        float best = -1.f; int bi = 0;                                                               // kColTransformArgmax
        for (int t = 0; t < M * M; ++t) {
            const float v = updft::colTransformMag(T.data(), Ex.data(), t / M, t % M, Nx);
            if (v > best) { best = v; bi = t; }
        }
        cy += (bi / M - h) / k;
        cx += (bi % M - h) / k;
    }
    return {cx, cy};
}

int main()
{
    const int U = 8, OUT = 160, RS = 128, RO = 16;
    std::mt19937 rng(3);
    std::normal_distribution<double> nd(0, 1);
    const int FS = OUT * U + 12 * U;
    cv::Mat fine(FS, FS, CV_64F);
    for (int i = 0; i < FS * FS; ++i) fine.ptr<double>()[i] = nd(rng);
    cv::GaussianBlur(fine, fine, cv::Size(0, 0), 0.9 * U);
    auto binAt = [&](int ox, int oy) { cv::Mat o; cv::resize(fine(cv::Rect(ox, oy, OUT * U, OUT * U)), o, cv::Size(OUT, OUT), 0, 0, cv::INTER_AREA); return o; };
    const cv::Rect roi(RO, RO, RS, RS);
    const cv::Mat win = pocsub::hanning(roi.size());

    double maxDiff = 0, maxErr = 0;
    int n = 0;
    for (int phaseOnly = 0; phaseOnly < 2; ++phaseOnly)
        for (int sx = -3 * U; sx <= 3 * U; sx += 5)
            for (int sy : {-2 * U + 3, 1, U + 6}) {
                const cv::Mat A = binAt(6 * U, 6 * U) * 30 + 120, B = binAt(6 * U - sx, 6 * U - sy) * 30 + 120;
                const cv::Point2d truth(sx / double(U), sy / double(U));
                // CPU 版（基準）
                const auto ref = pocsub::phaseCorrelateUpsampledDft(A(roi), B(roi), win, 100, phaseOnly, 0.0);
                // GPU 版と同じ入力を作る：R（float2、全スペクトル、行優先）と整数ピーク（符号付き）
                const cv::Mat f1 = pocsub::preprocess(A(roi), win), f2 = pocsub::preprocess(B(roi), win);
                const cv::Mat Rm = pocsub::crossPowerSpectrum(f1, f2, phaseOnly);
                std::vector<float2> R(RS * RS);
                for (int y = 0; y < RS; ++y)
                    for (int x = 0; x < RS; ++x) { const cv::Vec2d v = Rm.at<cv::Vec2d>(y, x); R[y * RS + x] = float2{float(v[0]), float(v[1])}; }
                cv::Point p;
                cv::minMaxLoc(pocsub::correlationSurface(Rm), nullptr, nullptr, nullptr, &p);
                const double x0 = p.x - RS / 2, y0 = p.y - RS / 2;   // 整数ピーク → 符号付きのずれ
                const cv::Point2d gpu = refineLikeGpu(R, RS, RS, 100, x0, y0);
                maxDiff = std::max(maxDiff, cv::norm(gpu - ref.shift));
                maxErr = std::max(maxErr, cv::norm(gpu - truth));
                ++n;
            }
    std::printf("%d 通り（通常・位相限定、ずれ x −3〜+3 画素、y 3 通り）\n", n);
    std::printf("CPU 版との差の最大：%.4f 画素（0.01 刻みの格子 1 目盛り以内なら float の丸めの違い）\n", maxDiff);
    std::printf("真のずれとの差の最大：%.4f 画素\n", maxErr);
    return maxDiff <= 0.0101 ? 0 : 1;
}

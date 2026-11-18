// updft_refine.h
// Upsampled DFT の「細かく求める部分」（行列 DFT）の CUDA 版。整数ピークを求めるところは既存のものを使う前提。
//
// やること（ブロックごと、1 段分）
//   1. E_y(j, ky) = exp(+i·2π·f(ky)·y_j / Ny),  y_j = cy + (j − h)/κ   … カーネル行列（縦）
//      E_x(l, kx) = exp(+i·2π·f(kx)·x_l / Nx),  x_l = cx + (l − h)/κ   … カーネル行列（横）
//   2. T = E_y · R            （M × Nx）
//   3. C = |T · E_xᵀ|         （M × M、相関面の大きさ）を出力する ← ここまで
//   最大値の探索は既存のカーネルで行う。見つけた点 (j*, l*) から中心を更新して（kUpdateCenter、または同じ式を既存側で）、
//   次の段を呼ぶ：cy += (j* − h)/κ, cx += (l* − h)/κ。段は κ = 10, 100, …, upsample（upsample = 100 なら 2 段）
//
//   呼び出しの流れ（upsample = 100 の例）
//     Stage st[8]; int ns = makeStages(100, st);
//     for (int i = 0; i < ns; ++i) {
//         updftCorrStage(R, B, Ny, Nx, st[i], center, C, ws, s);   // C：B × M × M（M = 2h+1）
//         既存の最大値探索（C の各ブロック M×M から、最大の添字 idx = j*·M + l* を求める）
//         kUpdateCenter<<<…>>>(idx, B, st[i], center)              // または既存側で同じ式
//     }
//
// 前提（既存の POC に合わせて変えるところ）
//   ・R：C2C（複素数 → 複素数）の全スペクトル。float2（cufftComplex）。ブロック b の R は R + b·Ny·Nx、行優先（ky が行、kx が列）
//        R2C（横が Nx/2+1 の半分）のときは、全スペクトルに広げてから渡すこと（下の注意を参照）
//   ・R の定義：R = F2·conj(F1)（CPU 版 crossPowerSpectrum と同じ）。逆（F1·conj(F2)）なら、ずれの符号が逆になる
//   ・center：ブロックごとの (cx, cy)。入力は整数ピークを「符号付きのずれ」にしたもの
//        （逆 FFT の出力の添字 i が N/2 以上なら i − N。fftshift した面なら i − N/2）。出力は細かく求めたずれ
//   ・ブロックの大きさ Ny, Nx は 1024 以下（横方向のスレッド数に使う）
//   ・メモリの確保は UpdftWorkspace で事前に行う。updftCorrStage の中では確保も同期もしない → CUDA グラフに入れられる
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

// 3. C = |T · E_xᵀ|（M × M）。grid = B、block = M·M 以上のスレッド。C(j, l) は C[b·M·M + j·M + l]（行 j が y、列 l が x）
__global__ void kColTransform(const float2* T, const float2* Ex, int Nx, int M, float* C)
{
    const int b = blockIdx.x, MM = M * M;
    for (int t = threadIdx.x; t < MM; t += blockDim.x)
        C[static_cast<size_t>(b) * MM + t] =
            colTransformMag(T + static_cast<size_t>(b) * M * Nx, Ex + static_cast<size_t>(b) * M * Nx, t / M, t % M, Nx);
}

// （任意）中心の更新。idx[b] = 既存の最大値探索で見つけた、ブロック b の C の中の添字（j*·M + l*）
//   既存側で同じ式（cy += (j* − h)/κ, cx += (l* − h)/κ）を書くなら不要
__global__ void kUpdateCenter(const int* idx, int B, int h, double kappa, double2* center)
{
    const int b = blockIdx.x * blockDim.x + threadIdx.x;
    if (b >= B) return;
    const int M = 2 * h + 1, j = idx[b] / M, l = idx[b] % M;
    center[b].y += (j - h) / kappa;
    center[b].x += (l - h) / kappa;
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

// 1 段分の相関面 C（B × M × M、M = 2·st.h + 1）を計算する。最大値の探索と中心の更新は呼び出し側で
//   R     ：B 個のブロックの相互スペクトル（float2、全スペクトル、行優先）
//   center：この段の中心 = 符号付きのずれ (x, y)（double2、デバイス）。1 段目は整数ピーク、2 段目以降は前の段で更新したもの
//   C     ：出力（float、デバイス、B·kMaxM·kMaxM 以上確保）。値は |相関|（Σ|R| では割っていない）
// メモリ確保・同期をしないので、stream キャプチャ（CUDA グラフ）の中で呼べる
inline void updftCorrStage(const float2* R, int B, int Ny, int Nx, const Stage& st,
                           const double2* center, float* C, UpdftWorkspace& ws, cudaStream_t s = 0)
{
    const int h = st.h, M = 2 * h + 1;
    kBuildKernel<<<dim3((M * Ny + 255) / 256, B), 256, 0, s>>>(center, 0, Ny, M, h, st.kappa, ws.Ey);
    kBuildKernel<<<dim3((M * Nx + 255) / 256, B), 256, 0, s>>>(center, 1, Nx, M, h, st.kappa, ws.Ex);
    kRowTransform<<<dim3(M, B), (Nx < 1024 ? Nx : 1024), 0, s>>>(ws.Ey, R, Ny, Nx, M, ws.T);
    kColTransform<<<B, 320, 0, s>>>(ws.T, ws.Ex, Nx, M, C);
}
#endif  // __CUDACC__

}  // namespace updft

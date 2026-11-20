// updft_graph_example.cu
// Upsampled DFT を最初から最後まで 1 つの CUDA グラフにする骨組み
//   前処理（平均を引く→窓）→ FFT → 相互スペクトル → 逆 FFT → 整数ピーク → 細かい段（2 段）→ 結果をホストへ
//
// グラフにする条件（全部この骨組みで満たしている）
//   ・キャプチャの中で cudaMalloc / cudaFree / 同期（cudaStreamSynchronize など）をしない → 全部、事前に確保
//   ・cuFFT のプランはキャプチャの前に作り、cufftSetStream でキャプチャするストリームに結び付ける
//   ・ブロック数 B と大きさは固定。入力・出力のバッファーのアドレスも固定（毎回同じバッファーに新しい画像を入れる）
//   ・ホストとのコピーは、ピン留めメモリ（cudaMallocHost）との cudaMemcpyAsync にする
#include "preprocess_blocks.h"
#include "updft_refine.h"
#include <cufft.h>
#include <cstdio>

// 相互スペクトル R = F2·conj(F1)（位相限定なら大きさで割る）。F1：基準ブロック、F2：比較ブロック
__global__ void kCrossPower(const float2* F1, const float2* F2, float2* R, size_t n, int phaseOnly)
{
    for (size_t i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += size_t(gridDim.x) * blockDim.x) {
        const float2 a = F1[i], b = F2[i];
        float2 r = make_float2(b.x * a.x + b.y * a.y, b.y * a.x - b.x * a.y);
        if (phaseOnly) { const float m = hypotf(r.x, r.y); if (m > 0.f) { r.x /= m; r.y /= m; } }
        R[i] = r;
    }
}

// 17×17 の細かい相関面 C の最大の添字（既存の最大値探索の代わり）。grid = B、block = 512（2 のべき乗）
__global__ void kArgmaxC(const float* C, int MM, int* idx)
{
    __shared__ float sv[512]; __shared__ int si[512];
    const int b = blockIdx.x, t = threadIdx.x;
    sv[t] = (t < MM) ? C[size_t(b) * MM + t] : -3.4e38f; si[t] = t;
    __syncthreads();
    for (int s = blockDim.x / 2; s > 0; s >>= 1) {
        if (t < s && (sv[t + s] > sv[t] || (sv[t + s] == sv[t] && si[t + s] < si[t]))) { sv[t] = sv[t + s]; si[t] = si[t + s]; }
        __syncthreads();
    }
    if (t == 0) idx[b] = si[0];
}

struct UpdftGraph {
    int B = 0, Ny = 0, Nx = 0, upsample = 100, phaseOnly = 1;
    // デバイスのバッファー（全部、最初に確保）
    unsigned short *dRef = nullptr, *dTgt = nullptr;   // 入力：切り出し済みのブロック（16bit の例）。B × Ny × pitch
    int pitch = 0;
    float2 *dF1 = nullptr, *dF2 = nullptr, *dR = nullptr, *dCorr = nullptr;
    float* dC = nullptr; int* dIdx = nullptr; double2* dCenter = nullptr;
    updft::UpdftWorkspace ws;
    double2* hCenter = nullptr;                        // 結果（ピン留めのホストメモリ）
    cufftHandle plan = 0;
    cudaStream_t s = nullptr;
    cudaGraphExec_t exec = nullptr;

    void init(int B_, int Ny_, int Nx_, int pitch_)
    {
        B = B_; Ny = Ny_; Nx = Nx_; pitch = pitch_;
        const size_t n = size_t(B) * Ny * Nx;
        cudaStreamCreate(&s);
        cudaMalloc(&dRef, sizeof(unsigned short) * B * Ny * pitch);
        cudaMalloc(&dTgt, sizeof(unsigned short) * B * Ny * pitch);
        cudaMalloc(&dF1, sizeof(float2) * n); cudaMalloc(&dF2, sizeof(float2) * n);
        cudaMalloc(&dR, sizeof(float2) * n);  cudaMalloc(&dCorr, sizeof(float2) * n);   // dCorr は R と別（R を上書きしない）
        cudaMalloc(&dC, sizeof(float) * B * updft::kMaxM * updft::kMaxM);
        cudaMalloc(&dIdx, sizeof(int) * B);
        cudaMalloc(&dCenter, sizeof(double2) * B);
        cudaMallocHost(&hCenter, sizeof(double2) * B);
        ws.alloc(B, Ny, Nx);
        int dims[2] = {Ny, Nx};
        cufftPlanMany(&plan, 2, dims, nullptr, 1, Ny * Nx, nullptr, 1, Ny * Nx, CUFFT_C2C, B);   // 作業領域もここで確保
        cufftSetStream(plan, s);                                                                // キャプチャするストリームに結び付ける

        // ---- ここからキャプチャ（記録するだけで、まだ実行しない）----
        cudaGraph_t g;
        cudaStreamBeginCapture(s, cudaStreamCaptureModeGlobal);
        prep::preprocessBlocks(dRef, pitch, B, Ny, Nx, dF1, 0, s);         // 平均を引く → 窓（出力は FFT の入力）
        prep::preprocessBlocks(dTgt, pitch, B, Ny, Nx, dF2, 0, s);
        cufftExecC2C(plan, dF1, dF1, CUFFT_FORWARD);                        // 順 FFT（その場で上書きしてよい）
        cufftExecC2C(plan, dF2, dF2, CUFFT_FORWARD);
        kCrossPower<<<1024, 256, 0, s>>>(dF1, dF2, dR, n, phaseOnly);       // R = F2·conj(F1)
        cufftExecC2C(plan, dR, dCorr, CUFFT_INVERSE);                       // 逆 FFT（R は残す）
        updft::integerPeak(dCorr, B, Ny, Nx, dCenter, nullptr, s);          // 整数ピーク → 符号付きのずれ
        updft::Stage st[8];
        const int ns = updft::makeStages(upsample, st);
        for (int i = 0; i < ns; ++i) {                                     // 細かい段（中心は GPU の上だけで更新）
            updft::updftCorrStage(dR, B, Ny, Nx, st[i], dCenter, dC, ws, s);
            const int M = 2 * st[i].h + 1;
            kArgmaxC<<<B, 512, 0, s>>>(dC, M * M, dIdx);
            updft::kUpdateCenter<<<(B + 127) / 128, 128, 0, s>>>(dIdx, B, st[i].h, st[i].kappa, dCenter);
        }
        cudaMemcpyAsync(hCenter, dCenter, sizeof(double2) * B, cudaMemcpyDeviceToHost, s);   // 結果をホストへ
        cudaStreamEndCapture(s, &g);
        // ---- キャプチャここまで ----
        cudaGraphInstantiate(&exec, g, 0);
        cudaGraphDestroy(g);
    }

    // 1 回分の処理：入力を同じバッファーにコピー → グラフを流す → 待つ
    const double2* run(const unsigned short* hRef, const unsigned short* hTgt)
    {
        const size_t bytes = sizeof(unsigned short) * B * Ny * pitch;
        cudaMemcpyAsync(dRef, hRef, bytes, cudaMemcpyHostToDevice, s);     // （ピン留めメモリなら速い）
        cudaMemcpyAsync(dTgt, hTgt, bytes, cudaMemcpyHostToDevice, s);
        cudaGraphLaunch(exec, s);
        cudaStreamSynchronize(s);                                          // ここで初めて待つ
        return hCenter;                                                    // ブロックごとのずれ (x, y)
    }

    void release()
    {
        cudaGraphExecDestroy(exec); cufftDestroy(plan); ws.release();
        cudaFree(dRef); cudaFree(dTgt); cudaFree(dF1); cudaFree(dF2); cudaFree(dR); cudaFree(dCorr);
        cudaFree(dC); cudaFree(dIdx); cudaFree(dCenter); cudaFreeHost(hCenter); cudaStreamDestroy(s);
    }
};

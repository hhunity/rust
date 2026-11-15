// test_updft_refine_cpu.cpp
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

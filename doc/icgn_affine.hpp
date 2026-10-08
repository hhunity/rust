
// poc_3point.hpp
// 手法②：POC ＋ 3点近似（パラボラ／ガウス）によるサブピクセル推定
// cv::phaseCorrelate の「5×5 重み付き重心」部分を 3点近似に置き換えたもの。
#pragma once
#include "poc_common.hpp"

namespace pocsub {

enum class ThreePointModel { Parabola, Gaussian };

// 1次元の3点近似。戻り値は整数ピークからの補正量（-0.5〜+0.5 程度）
inline double threePointOffset(double rm, double r0, double rp, ThreePointModel model)
{
    if (model == ThreePointModel::Gaussian) {
        // 対数が取れない（隣接値が 0 以下）場合はパラボラにフォールバック
        if (rm > 0.0 && r0 > 0.0 && rp > 0.0) {
            const double lm = std::log(rm), l0 = std::log(r0), lp = std::log(rp);
            const double den = 2.0 * (lm - 2.0 * l0 + lp);
            if (std::abs(den) > 1e-12) return (lm - lp) / den;
        }
    }
    const double den = 2.0 * (rm - 2.0 * r0 + rp);
    if (std::abs(den) < 1e-12) return 0.0;
    return (rm - rp) / den;
}

struct ThreePointResult {
    cv::Point2d shift;     // 推定ずれ量 [画素]
    double peak = 0.0;     // ピーク値（POCなら 0〜1、信頼度の目安）
};

// src1, src2 : 同サイズの1チャンネル画像
// window     : 窓関数（空なら窓なし。通常は hanning(src1.size())）
inline ThreePointResult phaseCorrelate3Point(const cv::Mat& src1, const cv::Mat& src2,
                                             const cv::Mat& window,
                                             ThreePointModel model = ThreePointModel::Parabola)
{
    CV_Assert(src1.size() == src2.size());
    const cv::Mat f1 = preprocess(src1, window);
    const cv::Mat f2 = preprocess(src2, window);
    const cv::Mat r  = correlationSurface(crossPowerSpectrum(f1, f2, true));

    ThreePointResult res;
    const cv::Point p = integerPeak(r, &res.peak);
    const double r0 = r.at<double>(p.y, p.x);
    const double dx = threePointOffset(atWrap(r, p.y, p.x - 1), r0, atWrap(r, p.y, p.x + 1), model);
    const double dy = threePointOffset(atWrap(r, p.y - 1, p.x), r0, atWrap(r, p.y + 1, p.x), model);
    res.shift = { p.x + dx - r.cols / 2, p.y + dy - r.rows / 2 };
    return res;
}

} // namespace pocsub


// poc_common.hpp
// POC（位相限定相関）の共通処理。
// cv::phaseCorrelate（OpenCV imgproc/src/phasecorr.cpp）の処理の流れ
//   窓関数 → DFT → 正規化相互パワースペクトル → 逆DFT → fftShift → ピーク検出
// を公開APIだけで再構成し、サブピクセル推定部分を差し替えられるようにしたもの。
// 符号の定義：src2 が src1 に対して (+dx, +dy) ずれているとき、結果は (+dx, +dy)。
//            （cv::phaseCorrelate(src1, src2) と同じ向き）
#pragma once
#include <opencv2/core.hpp>
#include <opencv2/imgproc.hpp>
#include <cmath>
#include <stdexcept>

namespace pocsub {

// 入力を CV_64F に変換し、平均を引いてから窓関数を掛ける
inline cv::Mat preprocess(const cv::Mat& src, const cv::Mat& window)
{
    cv::Mat f;
    src.convertTo(f, CV_64F);
    if (f.channels() != 1) throw std::invalid_argument("single-channel image required");
    f -= cv::mean(f)[0];
    if (!window.empty()) {
        cv::Mat w;
        window.convertTo(w, CV_64F);
        f = f.mul(w);
    }
    return f;
}

// ハニング窓（cv::createHanningWindow のラッパー）
inline cv::Mat hanning(cv::Size sz)
{
    cv::Mat w;
    cv::createHanningWindow(w, sz, CV_64F);
    return w;
}

// 相互パワースペクトル R = conj(F1) * F2 を返す（CV_64FC2）
//   phaseOnly = true  : |R| で割る（POC）
//   phaseOnly = false : 割らない（通常の相互相関）
inline cv::Mat crossPowerSpectrum(const cv::Mat& f1, const cv::Mat& f2, bool phaseOnly = true)
{
    cv::Mat F1, F2, R;
    cv::dft(f1, F1, cv::DFT_COMPLEX_OUTPUT);
    cv::dft(f2, F2, cv::DFT_COMPLEX_OUTPUT);
    // mulSpectrums(a, b, conjB=true) は a * conj(b)。ここでは F2 * conj(F1) を作る
    cv::mulSpectrums(F2, F1, R, 0, true);
    if (phaseOnly) {
        std::vector<cv::Mat> ch;
        cv::split(R, ch);
        cv::Mat mag;
        cv::magnitude(ch[0], ch[1], mag);
        mag += 1e-12;              // 0割り防止
        ch[0] /= mag;
        ch[1] /= mag;
        cv::merge(ch, R);
    }
    return R;
}

// スペクトル重み（ガウス型ローパス）を掛ける。sigma は正規化周波数（ナイキスト = 0.5）で指定。
// POC は全周波数を同じ重みで扱うため、S/N の悪い高周波がピーク位置を乱しやすい。
// 高周波を弱めるとピークが少し太くなる代わりに安定する。sigma <= 0 なら何もしない。
inline void applySpectralWeight(cv::Mat& R, double sigma)
{
    if (sigma <= 0.0) return;
    const double inv2s2 = 1.0 / (2.0 * sigma * sigma);
    for (int y = 0; y < R.rows; ++y) {
        const double fy = static_cast<double>((y < (R.rows + 1) / 2) ? y : y - R.rows) / R.rows;
        cv::Vec2d* row = R.ptr<cv::Vec2d>(y);
        for (int x = 0; x < R.cols; ++x) {
            const double fx = static_cast<double>((x < (R.cols + 1) / 2) ? x : x - R.cols) / R.cols;
            row[x] *= std::exp(-(fx * fx + fy * fy) * inv2s2);
        }
    }
}

// 象限入れ替え（numpy.fft.fftshift と同じ定義：ずれ0が (cols/2, rows/2) に来る）
inline void fftShift(cv::Mat& m)
{
    const int hx = m.cols / 2, hy = m.rows / 2;
    cv::Mat out(m.size(), m.type());
    for (int y = 0; y < m.rows; ++y) {
        const double* src = m.ptr<double>((y - hy + m.rows) % m.rows);
        double* dst = out.ptr<double>(y);
        for (int x = 0; x < m.cols; ++x)
            dst[x] = src[(x - hx + m.cols) % m.cols];
    }
    m = out;
}

// 逆DFTで相関面を作る（実部、CV_64F、fftShift 済み）
inline cv::Mat correlationSurface(const cv::Mat& R)
{
    cv::Mat r;
    cv::idft(R, r, cv::DFT_REAL_OUTPUT | cv::DFT_SCALE);
    fftShift(r);
    return r;
}

// 相関面の整数ピーク位置
inline cv::Point integerPeak(const cv::Mat& r, double* peakVal = nullptr)
{
    cv::Point loc;
    double maxv;
    cv::minMaxLoc(r, nullptr, &maxv, nullptr, &loc);
    if (peakVal) *peakVal = maxv;
    return loc;
}

// 周期境界で値を取得
inline double atWrap(const cv::Mat& r, int y, int x)
{
    y = (y % r.rows + r.rows) % r.rows;
    x = (x % r.cols + r.cols) % r.cols;
    return r.at<double>(y, x);
}

} // namespace pocsub


// poc_upsampled_dft.hpp
// 手法③：Upsampled DFT（Guizar-Sicairos, Thurman & Fienup, Opt. Lett. 33(2), 2008）
// 1) 相互パワースペクトルを逆DFTして整数ピークを求める
// 2) ピーク周辺だけ、細かい格子で「行列DFT」を計算する
//      C = Ey (M×Ny) · R (Ny×Nx) · ExT (Nx×M)   （複素数の積を実数の行列積で計算）
//    Ey(j,k) = exp(+i 2π k_signed · dy_j / Ny) など。全体を κ 倍ゼロ詰めするより桁違いに軽い
//    （本実装では倍率を 10 倍ずつ上げて段階的に絞り込む）
// 3) 最終段の格子上の最大値の位置を推定値とする（分解能 1/κ 画素）
#pragma once
#include "poc_common.hpp"
#include <vector>

namespace pocsub {

struct UpsampledDftResult {
    cv::Point2d shift;     // 推定ずれ量 [画素]
    double peak = 0.0;     // 細かい格子上のピーク値（|C| の最大）
};

namespace detail {
// 周波数インデックス（0,1,..,N/2-1,-N/2,..,-1）
inline int signedFreq(int k, int n) { return (k < (n + 1) / 2) ? k : k - n; }

// 行列DFTの核：rows = 出力点数 M、cols = 周波数点数 N
//   K(j,k) = exp(+i 2π f(k) (center + (j - jc)/kappa) / N)
// 実部 Kr と虚部 Ki を別々の CV_64F 行列で返す（複素数の行列積は実数の行列積 4 回で計算する）
inline void dftKernel(int n, int m, double center, double kappa, int jc, cv::Mat& Kr, cv::Mat& Ki)
{
    Kr.create(m, n, CV_64F);
    Ki.create(m, n, CV_64F);
    for (int j = 0; j < m; ++j) {
        const double d = center + (j - jc) / kappa;
        double* re = Kr.ptr<double>(j);
        double* im = Ki.ptr<double>(j);
        for (int k = 0; k < n; ++k) {
            const double ph = 2.0 * CV_PI * signedFreq(k, n) * d / n;
            re[k] = std::cos(ph);
            im[k] = std::sin(ph);
        }
    }
}
} // namespace detail

// src1, src2   : 同サイズの1チャンネル画像
// window       : 窓関数（空なら窓なし）
// upsample     : 倍率 κ（100 なら 0.01 画素分解能）
// phaseOnly    : false = 通常の相互相関（既定。原論文・skimage と同じ）、true = POC（位相限定）
// specSigma    : スペクトル重み（ガウス型ローパス）の幅。正規化周波数で指定（既定 0 = 重みなし）
//                滑らかな実画像で通常の相互相関が偏る場合は、phaseOnly = true ＋ specSigma = 0.1 程度を試す
inline UpsampledDftResult phaseCorrelateUpsampledDft(const cv::Mat& src1, const cv::Mat& src2,
                                                     const cv::Mat& window,
                                                     double upsample = 100.0,
                                                     bool phaseOnly = false,
                                                     double specSigma = 0.0)
{
    CV_Assert(src1.size() == src2.size() && upsample >= 1.0);
    const cv::Mat f1 = preprocess(src1, window);
    const cv::Mat f2 = preprocess(src2, window);
    cv::Mat R = crossPowerSpectrum(f1, f2, phaseOnly);
    applySpectralWeight(R, specSigma);
    CV_Assert(R.channels() == 2);   // 複素スペクトル（実部・虚部の2チャンネル）であること
    const int ny = R.rows, nx = R.cols;

    // 1) 整数ピーク（fftShift 済みの相関面で探し、符号付きずれに直す）
    const cv::Mat r = correlationSurface(R);
    const cv::Point p = integerPeak(r);
    const double y0 = p.y - ny / 2, x0 = p.x - nx / 2;

    UpsampledDftResult res;
    res.shift = cv::Point2d(x0, y0);
    if (upsample <= 1.0) return res;

    // 2) ピーク周辺を行列DFTで細かく評価する
    //    原論文はピーク周辺 1.5 画素を一度に倍率 κ で計算する（κ=100 なら 150×150 点）。
    //    ここでは倍率を 10 倍ずつ上げる段階的な絞り込みにして、各段の評価点を約 16×16 に抑えている。
    //    相関面はピーク近傍で滑らかな単峰形なので、結果は一括計算とほぼ同じで、計算量は大幅に減る。
    // 相互スペクトルを実部・虚部に分け、型を CV_64F にそろえる
    // （cv::gemm は2つの行列の型が一致している必要がある。ここで明示的にそろえておく）
    cv::Mat Rch[2], Rr, Ri;
    cv::split(R, Rch);
    Rch[0].convertTo(Rr, CV_64F);
    Rch[1].convertTo(Ri, CV_64F);

    double cy = y0, cx = x0, prevK = 1.0, maxv = 0.0;
    while (prevK < upsample) {
        const double k  = std::min(prevK * 10.0, upsample);
        const int    m  = static_cast<int>(std::ceil(1.5 * k / prevK)) + 1;
        const int    jc = m / 2;
        cv::Mat Eyr, Eyi, Exr, Exi;
        detail::dftKernel(ny, m, cy, k, jc, Eyr, Eyi);   // M×Ny
        detail::dftKernel(nx, m, cx, k, jc, Exr, Exi);   // M×Nx

        // T = Ey · R          （M×Nx）  T = (Eyr + iEyi)(Rr + iRi)
        cv::Mat Tr = Eyr * Rr - Eyi * Ri;
        cv::Mat Ti = Eyr * Ri + Eyi * Rr;
        // C = T · Ex^T        （M×M）   転置は共役を取らない普通の転置
        cv::Mat Cr = Tr * Exr.t() - Ti * Exi.t();
        cv::Mat Ci = Tr * Exi.t() + Ti * Exr.t();

        cv::Mat mag;
        cv::magnitude(Cr, Ci, mag);
        cv::Point q;
        cv::minMaxLoc(mag, nullptr, &maxv, nullptr, &q);
        cy += (q.y - jc) / k;
        cx += (q.x - jc) / k;
        prevK = k;
    }
    res.peak  = maxv / (static_cast<double>(nx) * ny);
    res.shift = cv::Point2d(cx, cy);
    return res;
}

} // namespace pocsub


// poc_icgn.hpp
// 手法④：POC で粗推定 → IC-GN（逆合成ガウス・ニュートン法）で精密化
//   評価関数：ZNSSD  C(p) = Σ { (f - fm)/Δf - (g(x+p) - gm)/Δg }²
//   変形関数：平行移動のみ  W(x; p) = x + p   （p = (px, py)）
//   補間    ：3次Bスプライン（事前フィルタ付き）
// 参考：Baker & Matthews, IJCV 56(3), 2004 / Pan, Li & Tong, Exp. Mech. 53, 2013
//
// 注意：cv::remap は内部で座標を 1/32 画素に量子化するため、0.01 画素級の用途には使わない。
//       ここでは平行移動のみなので、全画素で共通の小数部を使う分離型補間を自前で実装している。
#pragma once
#include "poc_common.hpp"
#include "poc_3point.hpp"
#include <vector>

namespace pocsub {

namespace detail {

// 1次元の3次Bスプライン事前フィルタ（Unser の再帰フィルタ、鏡映境界）
inline void bsplinePrefilter1D(double* s, int n, int stride)
{
    if (n < 2) return;
    const double z = std::sqrt(3.0) - 2.0;
    const int horizon = std::min(n, 30);          // |z|^30 ≈ 1e-17
    // 因果側の初期値
    double sum = 0.0, zk = 1.0;
    for (int k = 0; k < horizon; ++k) { sum += zk * s[k * stride]; zk *= z; }
    std::vector<double> cp(n);
    cp[0] = sum;
    for (int k = 1; k < n; ++k) cp[k] = s[k * stride] + z * cp[k - 1];
    // 反因果側
    std::vector<double> cm(n);
    cm[n - 1] = (z / (z * z - 1.0)) * (cp[n - 1] + z * cp[n - 2]);
    for (int k = n - 2; k >= 0; --k) cm[k] = z * (cm[k + 1] - cp[k]);
    for (int k = 0; k < n; ++k) s[k * stride] = 6.0 * cm[k];
}

// 画像全体の Bスプライン係数を求める（CV_64F）
inline cv::Mat bsplineCoefficients(const cv::Mat& img)
{
    cv::Mat c;
    img.convertTo(c, CV_64F);
    c = c.clone();
    for (int y = 0; y < c.rows; ++y) bsplinePrefilter1D(c.ptr<double>(y), c.cols, 1);
    const int stride = static_cast<int>(c.step1());
    for (int x = 0; x < c.cols; ++x) bsplinePrefilter1D(c.ptr<double>(0) + x, c.rows, stride);
    return c;
}

// 3次Bスプラインの重み（t ∈ [0,1)、ノード -1, 0, +1, +2）
inline void bsplineWeights(double t, double w[4])
{
    const double t2 = t * t, t3 = t2 * t;
    w[0] = (1.0 - t) * (1.0 - t) * (1.0 - t) / 6.0;
    w[1] = (4.0 - 6.0 * t2 + 3.0 * t3) / 6.0;
    w[2] = (1.0 + 3.0 * t + 3.0 * t2 - 3.0 * t3) / 6.0;
    w[3] = t3 / 6.0;
}

// g(x + px, y + py) を roi 内の全画素について求める（係数画像 coef から）
inline cv::Mat sampleShifted(const cv::Mat& coef, cv::Rect roi, double px, double py)
{
    const int ix = static_cast<int>(std::floor(px)), iy = static_cast<int>(std::floor(py));
    double wx[4], wy[4];
    bsplineWeights(px - ix, wx);
    bsplineWeights(py - iy, wy);

    // 必要範囲が画像内に収まっているか確認
    CV_Assert(roi.x + ix - 1 >= 0 && roi.y + iy - 1 >= 0 &&
              roi.x + roi.width + ix + 2 <= coef.cols && roi.y + roi.height + iy + 2 <= coef.rows);

    // 横方向 → 縦方向の分離型補間
    cv::Mat tmp(roi.height + 3, roi.width, CV_64F);
    for (int y = 0; y < tmp.rows; ++y) {
        const double* src = coef.ptr<double>(roi.y + iy - 1 + y) + roi.x + ix - 1;
        double* dst = tmp.ptr<double>(y);
        for (int x = 0; x < roi.width; ++x)
            dst[x] = wx[0] * src[x] + wx[1] * src[x + 1] + wx[2] * src[x + 2] + wx[3] * src[x + 3];
    }
    cv::Mat out(roi.height, roi.width, CV_64F);
    for (int y = 0; y < roi.height; ++y) {
        const double* a = tmp.ptr<double>(y);
        const double* b = tmp.ptr<double>(y + 1);
        const double* c = tmp.ptr<double>(y + 2);
        const double* d = tmp.ptr<double>(y + 3);
        double* dst = out.ptr<double>(y);
        for (int x = 0; x < roi.width; ++x)
            dst[x] = wy[0] * a[x] + wy[1] * b[x] + wy[2] * c[x] + wy[3] * d[x];
    }
    return out;
}

} // namespace detail

struct IcgnResult {
    cv::Point2d shift;        // 推定ずれ量 [画素]
    cv::Point2d initial;      // POC による初期値
    int iterations = 0;       // 反復回数
    bool converged = false;   // 収束したか
    double zncc = 0.0;        // 最終的な ZNCC（1 に近いほど一致、信頼度の目安）
};

// IC-GN 本体（平行移動のみ）
//   f, g   : 基準画像・比較画像（同サイズ、1チャンネル）
//   init   : 初期値（±0.5〜1 画素以内が目安）
//   roi    : 評価に使う領域（基準画像 f 上の矩形。空なら自動で内側を使う）
inline IcgnResult icgnTranslation(const cv::Mat& f_in, const cv::Mat& g_in, cv::Point2d init,
                                  cv::Rect roi = cv::Rect(),
                                  int maxIter = 30, double tol = 1e-4)
{
    CV_Assert(f_in.size() == g_in.size() && f_in.channels() == 1);
    cv::Mat f, g;
    f_in.convertTo(f, CV_64F);
    g_in.convertTo(g, CV_64F);

    if (roi.area() == 0) {
        const int m = static_cast<int>(std::ceil(std::max(std::abs(init.x), std::abs(init.y)))) + 6;
        roi = cv::Rect(m, m, f.cols - 2 * m, f.rows - 2 * m);
    }
    CV_Assert(roi.width > 8 && roi.height > 8);

    // --- 基準画像側の事前計算（IC の利点：ここは1回だけ） ---
    cv::Mat fr = f(roi).clone();
    const double fm = cv::mean(fr)[0];
    fr -= fm;
    const double fn = std::sqrt(fr.dot(fr));

    cv::Mat gx, gy;   // 中心差分による輝度勾配
    cv::Mat kx = (cv::Mat_<double>(1, 3) << -0.5, 0.0, 0.5);
    cv::filter2D(f, gx, CV_64F, kx, cv::Point(-1, -1), 0, cv::BORDER_REPLICATE);
    cv::filter2D(f, gy, CV_64F, kx.t(), cv::Point(-1, -1), 0, cv::BORDER_REPLICATE);
    gx = gx(roi).clone();
    gy = gy(roi).clone();

    // ヘッセ行列 H = Σ J^T J（J = [gx, gy]）
    const double hxx = gx.dot(gx), hxy = gx.dot(gy), hyy = gy.dot(gy);
    cv::Matx22d H(hxx, hxy, hxy, hyy);
    CV_Assert(cv::determinant(H) > 1e-12);   // テクスチャが乏しいと解けない
    const cv::Matx22d Hinv = H.inv();

    const cv::Mat coef = detail::bsplineCoefficients(g);

    // --- 反復 ---
    IcgnResult res;
    res.initial = init;
    cv::Point2d p = init;
    for (int it = 0; it < maxIter; ++it) {
        cv::Mat gr = detail::sampleShifted(coef, roi, p.x, p.y);
        const double gm = cv::mean(gr)[0];
        gr -= gm;
        const double gn = std::sqrt(gr.dot(gr));
        if (gn < 1e-12) break;

        // 残差 e = (Δf/Δg)(g - gm) - (f - fm)
        cv::Mat e = gr * (fn / gn) - fr;
        const cv::Vec2d b(gx.dot(e), gy.dot(e));
        const cv::Vec2d dp = Hinv * b;

        // 逆合成更新（平行移動では p ← p − Δp）
        p.x -= dp[0];
        p.y -= dp[1];
        res.iterations = it + 1;
        res.zncc = fr.dot(gr) / (fn * gn);
        if (std::max(std::abs(dp[0]), std::abs(dp[1])) < tol) { res.converged = true; break; }
    }
    res.shift = p;
    return res;
}

// POC（3点パラボラ）で初期値を求め、IC-GN で精密化する
inline IcgnResult phaseCorrelateICGN(const cv::Mat& src1, const cv::Mat& src2,
                                     const cv::Mat& window,
                                     cv::Rect roi = cv::Rect(),
                                     int maxIter = 30, double tol = 1e-4)
{
    const ThreePointResult coarse = phaseCorrelate3Point(src1, src2, window, ThreePointModel::Parabola);
    return icgnTranslation(src1, src2, coarse.shift, roi, maxIter, tol);
}

} // namespace pocsub




// poc_icgn_affine.hpp
// 手法④の拡張：POC で粗推定 → IC-GN（アフィン、6パラメータ）で精密化
//   平行移動に加えて、回転・伸縮（x/y 別）・せん断を同時に推定する。
//
//   変形関数（ROI 中心 (cx, cy) からの相対座標 dx = x - cx, dy = y - cy を使う）
//     x' = x + u + ux*dx + uy*dy
//     y' = y + v + vx*dx + vy*dy
//   p = (u, ux, uy, v, vx, vy)。u, v が「ROI 中心でのずれ量」になる。
//   中心基準にしているのは、回転・伸縮とずれ量が混ざりにくく、ヘッセ行列の条件が良くなるため。
//
//   評価関数：ZNSSD（poc_icgn.hpp と同じ）
//   補間    ：3次Bスプライン（poc_icgn.hpp の係数計算を再利用）
//   勾配    ：基準画像の Bスプライン表現から厳密に微分
//   更新    ：逆合成  W(p) ← W(p) ∘ W(Δp)^-1
// 参考：Pan, Li & Tong, Exp. Mech. 53, 2013 / Baker & Matthews, IJCV 56(3), 2004
//
// poc_icgn.hpp（平行移動のみ）はそのまま残しており、こちらは追加のヘッダ。
#pragma once
#include "poc_icgn.hpp"
#include <array>

namespace pocsub {

namespace detail {

// Bスプライン係数画像 coef から点 (x, y) の値を求める（非分離、16点）
inline double bsplineSample(const cv::Mat& coef, double x, double y)
{
    const int ix = static_cast<int>(std::floor(x)), iy = static_cast<int>(std::floor(y));
    double wx[4], wy[4];
    bsplineWeights(x - ix, wx);
    bsplineWeights(y - iy, wy);
    double s = 0.0;
    for (int j = 0; j < 4; ++j) {
        const double* r = coef.ptr<double>(iy - 1 + j) + (ix - 1);
        s += wy[j] * (wx[0] * r[0] + wx[1] * r[1] + wx[2] * r[2] + wx[3] * r[3]);
    }
    return s;
}

// 3x3 同次行列（アフィン）を p から作る
inline cv::Matx33d affineFromParams(const cv::Vec6d& p)
{
    return cv::Matx33d(1.0 + p[1], p[2],       p[0],
                       p[4],       1.0 + p[5], p[3],
                       0.0,        0.0,        1.0);
}

inline cv::Vec6d paramsFromAffine(const cv::Matx33d& M)
{
    return cv::Vec6d(M(0, 2), M(0, 0) - 1.0, M(0, 1), M(1, 2), M(1, 0), M(1, 1) - 1.0);
}

} // namespace detail

struct IcgnAffineResult {
    cv::Point2d shift;          // ROI 中心でのずれ量 [画素]（u, v）
    cv::Point2d center;         // ROI 中心の座標（基準画像上）
    cv::Matx22d A;              // 線形部分 A = I + [[ux, uy], [vx, vy]]
    cv::Vec6d   params;         // (u, ux, uy, v, vx, vy)
    double rotationDeg = 0.0;   // 回転角 [度]（反時計回りが正、画像座標は y 下向きなので見た目は時計回り）
    double scaleX = 1.0;        // x 方向の伸縮率（A の第1列の長さ）
    double scaleY = 1.0;        // y 方向の伸縮率（A の第2列の長さ）
    double shear = 0.0;         // せん断（2列の非直交度、rad）
    cv::Point2d initial;        // 初期値（POC）
    int iterations = 0;
    bool converged = false;
    double zncc = 0.0;          // 最終 ZNCC（信頼度の目安）

    // 基準画像上の点 (x, y) が比較画像のどこに移ったか（ずれ量）
    cv::Point2d displacementAt(double x, double y) const
    {
        const double dx = x - center.x, dy = y - center.y;
        return cv::Point2d(params[0] + params[1] * dx + params[2] * dy,
                           params[3] + params[4] * dx + params[5] * dy);
    }
};

// IC-GN 本体（アフィン）
//   f, g    : 基準画像・比較画像（同サイズ、1チャンネル）
//   init    : 初期値（ROI 中心でのずれ量。±0.5〜1 画素以内が目安）
//   roi     : 評価に使う領域（基準画像上。空なら自動で内側を使う）
//   initA   : 線形部分の初期値（通常は単位行列）
//   tol     : 収束判定。ROI の角での移動量に換算した更新量 [画素]
inline IcgnAffineResult icgnAffine(const cv::Mat& f_in, const cv::Mat& g_in, cv::Point2d init,
                                   cv::Rect roi = cv::Rect(),
                                   cv::Matx22d initA = cv::Matx22d::eye(),
                                   int maxIter = 50, double tol = 1e-4)
{
    CV_Assert(f_in.size() == g_in.size() && f_in.channels() == 1);
    cv::Mat f, g;
    f_in.convertTo(f, CV_64F);
    g_in.convertTo(g, CV_64F);

    if (roi.area() == 0) {
        const int m = static_cast<int>(std::ceil(std::max(std::abs(init.x), std::abs(init.y)))) + 8;
        roi = cv::Rect(m, m, f.cols - 2 * m, f.rows - 2 * m);
    }
    CV_Assert(roi.width > 8 && roi.height > 8);
    CV_Assert(roi.x >= 2 && roi.y >= 2 && roi.x + roi.width + 2 <= f.cols && roi.y + roi.height + 2 <= f.rows);

    const double cx = roi.x + (roi.width - 1) * 0.5;
    const double cy = roi.y + (roi.height - 1) * 0.5;
    const int n = roi.width * roi.height;

    // --- 基準画像側の事前計算（1回だけ） ---
    cv::Mat fr = f(roi).clone();
    const double fm = cv::mean(fr)[0];
    fr -= fm;
    const double fn = std::sqrt(fr.dot(fr));
    CV_Assert(fn > 1e-12);

    // 勾配：Bスプライン表現の厳密な微分（整数格子点では (c[k+1] - c[k-1]) / 2 を直交方向に [1 4 1]/6 で平滑）
    const cv::Mat cf = detail::bsplineCoefficients(f);
    std::vector<double> gxv(n), gyv(n);
    for (int y = 0; y < roi.height; ++y) {
        const int yy = roi.y + y;
        const double* rm = cf.ptr<double>(yy - 1);
        const double* r0 = cf.ptr<double>(yy);
        const double* rp = cf.ptr<double>(yy + 1);
        for (int x = 0; x < roi.width; ++x) {
            const int xx = roi.x + x;
            const double dxm = (rm[xx + 1] - rm[xx - 1]) * 0.5;
            const double dx0 = (r0[xx + 1] - r0[xx - 1]) * 0.5;
            const double dxp = (rp[xx + 1] - rp[xx - 1]) * 0.5;
            const double dym = (rp[xx - 1] - rm[xx - 1]) * 0.5;
            const double dy0 = (rp[xx] - rm[xx]) * 0.5;
            const double dyp = (rp[xx + 1] - rm[xx + 1]) * 0.5;
            gxv[y * roi.width + x] = (dxm + 4.0 * dx0 + dxp) / 6.0;
            gyv[y * roi.width + x] = (dym + 4.0 * dy0 + dyp) / 6.0;
        }
    }

    // 最急降下画像 SD = [gx, gx*dx, gx*dy, gy, gy*dx, gy*dy] とヘッセ行列
    std::vector<std::array<double, 6>> sd(n);
    cv::Matx66d H = cv::Matx66d::zeros();
    for (int y = 0; y < roi.height; ++y) {
        const double dy = roi.y + y - cy;
        for (int x = 0; x < roi.width; ++x) {
            const double dx = roi.x + x - cx;
            const int i = y * roi.width + x;
            const double gx = gxv[i], gy = gyv[i];
            std::array<double, 6>& s = sd[i];
            s = { gx, gx * dx, gx * dy, gy, gy * dx, gy * dy };
            for (int a = 0; a < 6; ++a)
                for (int b = a; b < 6; ++b) H(a, b) += s[a] * s[b];
        }
    }
    for (int a = 0; a < 6; ++a)
        for (int b = 0; b < a; ++b) H(a, b) = H(b, a);
    cv::Matx66d Hinv;
    const double ok = cv::invert(H, Hinv, cv::DECOMP_CHOLESKY);
    CV_Assert(ok != 0.0);   // テクスチャが乏しいと解けない

    const cv::Mat cg = detail::bsplineCoefficients(g);

    // 収束判定用：ROI の半幅
    const double hw = roi.width * 0.5, hh = roi.height * 0.5;

    // --- 反復 ---
    IcgnAffineResult res;
    res.initial = init;
    res.center = cv::Point2d(cx, cy);
    cv::Vec6d p(init.x, initA(0, 0) - 1.0, initA(0, 1), init.y, initA(1, 0), initA(1, 1) - 1.0);
    std::vector<double> gw(n);

    for (int it = 0; it < maxIter; ++it) {
        // g を変形後の座標で補間
        double gsum = 0.0;
        bool inside = true;
        for (int y = 0; y < roi.height && inside; ++y) {
            const double dy = roi.y + y - cy;
            for (int x = 0; x < roi.width; ++x) {
                const double dx = roi.x + x - cx;
                const double xs = roi.x + x + p[0] + p[1] * dx + p[2] * dy;
                const double ys = roi.y + y + p[3] + p[4] * dx + p[5] * dy;
                if (xs < 1.0 || ys < 1.0 || xs >= g.cols - 3.0 || ys >= g.rows - 3.0) { inside = false; break; }
                const double v = detail::bsplineSample(cg, xs, ys);
                gw[y * roi.width + x] = v;
                gsum += v;
            }
        }
        if (!inside) break;   // 変形後の ROI が画像外にはみ出した（発散）

        const double gm = gsum / n;
        double gss = 0.0, fg = 0.0;
        for (int i = 0; i < n; ++i) { gw[i] -= gm; gss += gw[i] * gw[i]; }
        const double gn = std::sqrt(gss);
        if (gn < 1e-12) break;
        const double scale = fn / gn;

        // b = Σ SD^T e,  e = (Δf/Δg)(g - gm) - (f - fm)
        cv::Vec6d b(0, 0, 0, 0, 0, 0);
        for (int y = 0; y < roi.height; ++y) {
            const double* frow = fr.ptr<double>(y);
            for (int x = 0; x < roi.width; ++x) {
                const int i = y * roi.width + x;
                const double e = gw[i] * scale - frow[x];
                fg += frow[x] * gw[i];
                const std::array<double, 6>& s = sd[i];
                for (int a = 0; a < 6; ++a) b[a] += s[a] * e;
            }
        }
        const cv::Vec6d dp = Hinv * b;

        // 逆合成更新
        const cv::Matx33d M = detail::affineFromParams(p) * detail::affineFromParams(dp).inv();
        p = detail::paramsFromAffine(M);

        res.iterations = it + 1;
        res.zncc = fg / (fn * gn);

        // ROI の角での移動量に換算して判定
        const double step = std::sqrt(dp[0] * dp[0] + dp[3] * dp[3] +
                                      (dp[1] * dp[1] + dp[4] * dp[4]) * hw * hw +
                                      (dp[2] * dp[2] + dp[5] * dp[5]) * hh * hh);
        if (step < tol) { res.converged = true; break; }
    }

    // 結果の整理
    res.params = p;
    res.shift = cv::Point2d(p[0], p[3]);
    res.A = cv::Matx22d(1.0 + p[1], p[2], p[4], 1.0 + p[5]);
    const double a00 = res.A(0, 0), a01 = res.A(0, 1), a10 = res.A(1, 0), a11 = res.A(1, 1);
    res.rotationDeg = std::atan2(a10 - a01, a00 + a11) * 180.0 / CV_PI;
    res.scaleX = std::sqrt(a00 * a00 + a10 * a10);
    res.scaleY = std::sqrt(a01 * a01 + a11 * a11);
    res.shear = std::asin(std::max(-1.0, std::min(1.0, (a00 * a01 + a10 * a11) / (res.scaleX * res.scaleY))));
    return res;
}

// POC（3点パラボラ）で初期値を求め、アフィン IC-GN で精密化する
//   POC は ROI を切り出した画像で行う（ROI 中心でのずれ量を初期値にするため）
inline IcgnAffineResult phaseCorrelateICGNAffine(const cv::Mat& src1, const cv::Mat& src2,
                                                 const cv::Mat& window,
                                                 cv::Rect roi = cv::Rect(),
                                                 int maxIter = 50, double tol = 1e-4)
{
    cv::Point2d init;
    if (roi.area() == 0) {
        init = phaseCorrelate3Point(src1, src2, window, ThreePointModel::Parabola).shift;
    } else {
        cv::Mat w = window.empty() ? cv::Mat() : (window.size() == roi.size() ? window : hanning(roi.size()));
        init = phaseCorrelate3Point(src1(roi), src2(roi), w, ThreePointModel::Parabola).shift;
    }
    return icgnAffine(src1, src2, init, roi, cv::Matx22d::eye(), maxIter, tol);
}

} // namespace pocsub

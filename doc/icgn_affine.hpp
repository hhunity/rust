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

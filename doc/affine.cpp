// test_affine.cpp
// アフィン IC-GN（poc_icgn_affine.hpp）の精度と処理時間を、平行移動のみの IC-GN と比べる。
//
// テスト画像の作り方（補間による有利不利を避けるため）
//   合成モード（既定）：ランダムな正弦波 3000 本の和（帯域制限された解析的な絵柄）。
//     任意の小数座標で厳密に値を計算できるので、補間を一切使わずに回転・伸縮した画像を作れる。
//   画像モード（--image）：手持ち画像を基準にし、比較画像は 2次元の窓付き sinc 補間で作る。
//     sinc 補間は Bスプラインとは別の補間なので IC-GN に特別有利にはならないが、完全に中立でもない。
//
// 真値の定義：基準画像の点 x が、比較画像では  x' = c + A (x - c) + t  に移る（c は ROI 中心）。
//   したがって ROI 中心でのずれ量の真値は t、点 x でのずれ量は t + (A - I)(x - c)。
#include "poc_icgn.hpp"
#include "poc_icgn_affine.hpp"
#include <opencv2/imgcodecs.hpp>
#include <CLI/CLI.hpp>
#include <chrono>
#include <cstdio>
#include <random>
#include <complex>

using namespace pocsub;

namespace {

struct Wave { double kx, ky, ph, amp; };

std::vector<Wave> makeWaves(unsigned seed, int count = 3000, double fmax = 0.30)
{
    std::mt19937 rng(seed);
    std::uniform_real_distribution<double> u01(0.0, 1.0);
    std::vector<Wave> w(count);
    for (auto& s : w) {
        const double r = 0.01 + (fmax - 0.01) * u01(rng);      // 周波数 [cycle/画素]
        const double th = 2.0 * CV_PI * u01(rng);
        s.kx = 2.0 * CV_PI * r * std::cos(th);
        s.ky = 2.0 * CV_PI * r * std::sin(th);
        s.ph = 2.0 * CV_PI * u01(rng);
        s.amp = 1.0 / (1.0 + r / 0.08);                          // 低周波ほど強い（実画像に近い）
    }
    return w;
}

// 真の変形：基準の x → 比較の c + A (x - c) + t
struct Truth { cv::Matx22d A; cv::Point2d t; cv::Point2d c; };

Truth makeTruth(double rotDeg, double sx, double sy, cv::Point2d t, cv::Point2d c)
{
    const double th = rotDeg * CV_PI / 180.0;
    const cv::Matx22d R(std::cos(th), -std::sin(th), std::sin(th), std::cos(th));
    const cv::Matx22d S(sx, 0, 0, sy);
    return { R * S, t, c };
}

// 比較画像の画素 y に対応する基準画像上の座標  x = c + A^-1 (y - c - t)
cv::Point2d inverseMap(const Truth& T, double yx, double yy, const cv::Matx22d& Ainv)
{
    const cv::Vec2d d(yx - T.c.x - T.t.x, yy - T.c.y - T.t.y);
    const cv::Vec2d x = Ainv * d;
    return cv::Point2d(T.c.x + x[0], T.c.y + x[1]);
}

// 正弦波の和を画像全体で評価する（変形 y -> x = Minv y + o を各波の周波数に織り込む）
//   cos(k·(Minv y + o) + φ) = cos((Minv^T k)·y + k·o + φ) なので、変形後も正弦波の和のまま。
//   x 方向・y 方向の複素指数を事前計算し、1画素 1波あたり複素数の掛け算1回で済ませる。
cv::Mat renderWaves(const std::vector<Wave>& w, cv::Size sz, const cv::Matx22d& Minv, cv::Point2d o)
{
    std::vector<std::complex<double>> acc(static_cast<size_t>(sz.area()), 0.0);
    std::vector<std::complex<double>> ex(sz.width), ey(sz.height);
    for (const auto& q : w) {
        const double a = Minv(0, 0) * q.kx + Minv(1, 0) * q.ky;   // (Minv^T k).x
        const double b = Minv(0, 1) * q.kx + Minv(1, 1) * q.ky;   // (Minv^T k).y
        const double ph = q.kx * o.x + q.ky * o.y + q.ph;
        for (int x = 0; x < sz.width; ++x) ex[x] = std::polar(1.0, a * x);
        for (int y = 0; y < sz.height; ++y) ey[y] = std::polar(q.amp, b * y + ph);
        for (int y = 0; y < sz.height; ++y) {
            std::complex<double>* r = &acc[static_cast<size_t>(y) * sz.width];
            const std::complex<double> e = ey[y];
            for (int x = 0; x < sz.width; ++x) r[x] += e * ex[x];
        }
    }
    cv::Mat out(sz, CV_64F);
    for (int i = 0; i < sz.area(); ++i) out.ptr<double>()[i] = acc[i].real();
    return out;
}

// 合成画像の組を作る
void makeSynthetic(const std::vector<Wave>& w, cv::Size sz, const Truth& T, cv::Mat& f, cv::Mat& g)
{
    const cv::Matx22d Ainv = T.A.inv();
    f = renderWaves(w, sz, cv::Matx22d::eye(), cv::Point2d(0, 0));
    // 比較画像の画素 y に対応する基準座標  x = c + Ainv (y - c - t) = Ainv y + (c - Ainv (c + t))
    const cv::Vec2d ct(T.c.x + T.t.x, T.c.y + T.t.y);
    const cv::Vec2d o = cv::Vec2d(T.c.x, T.c.y) - Ainv * ct;
    g = renderWaves(w, sz, Ainv, cv::Point2d(o[0], o[1]));
    // 8bit 相当のレンジに合わせる（平均 128、標準偏差 40）
    cv::Scalar m, s;
    cv::meanStdDev(f, m, s);
    f = (f - m[0]) * (40.0 / s[0]) + 128.0;
    g = (g - m[0]) * (40.0 / s[0]) + 128.0;
}

// 窓付き sinc（ハン窓、半径 R）による 2次元補間
double sincInterp(const cv::Mat& img, double x, double y, int R)
{
    auto ker = [R](double t) {
        if (std::abs(t) < 1e-12) return 1.0;
        if (std::abs(t) >= R) return 0.0;
        const double a = CV_PI * t;
        return std::sin(a) / a * (0.5 + 0.5 * std::cos(a / R));
    };
    const int ix = static_cast<int>(std::floor(x)), iy = static_cast<int>(std::floor(y));
    double wx[64], wy[64], sx = 0, sy = 0;
    for (int i = -R + 1; i <= R; ++i) { wx[i + R - 1] = ker(x - (ix + i)); sx += wx[i + R - 1]; }
    for (int j = -R + 1; j <= R; ++j) { wy[j + R - 1] = ker(y - (iy + j)); sy += wy[j + R - 1]; }
    double s = 0.0;
    for (int j = -R + 1; j <= R; ++j) {
        const double* row = img.ptr<double>(iy + j);
        double r = 0.0;
        for (int i = -R + 1; i <= R; ++i) r += wx[i + R - 1] * row[ix + i];
        s += wy[j + R - 1] * r;
    }
    return s / (sx * sy);
}

// 手持ち画像から組を作る（src は CV_64F の全体画像、roi0 は切り出し位置）
bool makeFromImage(const cv::Mat& src, cv::Rect roi0, const Truth& Tlocal, int R, cv::Mat& f, cv::Mat& g)
{
    const cv::Matx22d Ainv = Tlocal.A.inv();
    f = src(roi0).clone();
    g.create(roi0.size(), CV_64F);
    for (int y = 0; y < roi0.height; ++y)
        for (int x = 0; x < roi0.width; ++x) {
            const cv::Point2d q = inverseMap(Tlocal, x, y, Ainv);   // 切り出し座標系
            const double X = q.x + roi0.x, Y = q.y + roi0.y;           // 全体画像座標系
            if (X < R + 1 || Y < R + 1 || X >= src.cols - R - 1 || Y >= src.rows - R - 1) return false;
            g.at<double>(y, x) = sincInterp(src, X, Y, R);
        }
    return true;
}

struct Errors {
    double centerX = 0, centerY = 0;    // ROI 中心のずれ量誤差（表示は長さ √(x²+y²)）
    double cornerMax = 0;               // ROI 内（四隅）のずれ量誤差の最大
};

// 平行移動 IC-GN の誤差（全点で同じずれ量 shift を使ったときの誤差）
Errors errorsTranslation(const Truth& T, cv::Point2d shift, cv::Point2d center, cv::Rect roi)
{
    Errors e;
    const cv::Point2d tc = T.t + cv::Point2d((T.A(0, 0) - 1) * (center.x - T.c.x) + T.A(0, 1) * (center.y - T.c.y),
                                             T.A(1, 0) * (center.x - T.c.x) + (T.A(1, 1) - 1) * (center.y - T.c.y));
    e.centerX = shift.x - tc.x;
    e.centerY = shift.y - tc.y;
    const double xs[2] = { double(roi.x), double(roi.x + roi.width - 1) };
    const double ys[2] = { double(roi.y), double(roi.y + roi.height - 1) };
    for (double x : xs) for (double y : ys) {
        const double dx = x - T.c.x, dy = y - T.c.y;
        const double tx = T.t.x + (T.A(0, 0) - 1) * dx + T.A(0, 1) * dy;
        const double ty = T.t.y + T.A(1, 0) * dx + (T.A(1, 1) - 1) * dy;
        e.cornerMax = std::max(e.cornerMax, std::hypot(shift.x - tx, shift.y - ty));
    }
    return e;
}

Errors errorsAffine(const Truth& T, const IcgnAffineResult& r, cv::Rect roi)
{
    Errors e;
    const cv::Point2d d = r.displacementAt(r.center.x, r.center.y);
    const double dxc = r.center.x - T.c.x, dyc = r.center.y - T.c.y;
    e.centerX = d.x - (T.t.x + (T.A(0, 0) - 1) * dxc + T.A(0, 1) * dyc);
    e.centerY = d.y - (T.t.y + T.A(1, 0) * dxc + (T.A(1, 1) - 1) * dyc);
    const double xs[2] = { double(roi.x), double(roi.x + roi.width - 1) };
    const double ys[2] = { double(roi.y), double(roi.y + roi.height - 1) };
    for (double x : xs) for (double y : ys) {
        const cv::Point2d est = r.displacementAt(x, y);
        const double dx = x - T.c.x, dy = y - T.c.y;
        const double tx = T.t.x + (T.A(0, 0) - 1) * dx + T.A(0, 1) * dy;
        const double ty = T.t.y + T.A(1, 0) * dx + (T.A(1, 1) - 1) * dy;
        e.cornerMax = std::max(e.cornerMax, std::hypot(est.x - tx, est.y - ty));
    }
    return e;
}

template <class F>
double timeMs(F&& fn, int reps)
{
    fn();   // ウォームアップ
    const auto t0 = std::chrono::steady_clock::now();
    for (int i = 0; i < reps; ++i) fn();
    const auto t1 = std::chrono::steady_clock::now();
    return std::chrono::duration<double, std::milli>(t1 - t0).count() / reps;
}

} // namespace

int main(int argc, char** argv)
{
    CLI::App app{ "アフィン IC-GN と平行移動 IC-GN の比較" };
    int roiSize = 128;
    double dx = 3.37, dy = -1.62, noise = 0.0;
    int trials = 1, seed = 1;
    std::string imagePath;
    int imgX = -1, imgY = -1, sincR = 8;
    bool bench = false, benchOnly = false;
    double rot = 0.0, sx = 1.0, sy = 1.0;
    bool custom = false;
    app.add_option("--size", roiSize, "ROI（ブロック）の一辺 [画素]");
    app.add_option("--dx", dx, "ROI 中心での x ずれ [画素]");
    app.add_option("--dy", dy, "ROI 中心での y ずれ [画素]");
    app.add_option("--noise", noise, "加えるガウスノイズの標準偏差（輝度値、絵柄の標準偏差は 40）");
    app.add_option("--trials", trials, "ノイズありのときの試行回数（RMS を表示）");
    app.add_option("--seed", seed, "乱数の種");
    app.add_option("--image", imagePath, "手持ち画像（8/16bit、1チャンネルとして読む）");
    app.add_option("-x,--x", imgX, "画像モードの切り出し左上 x（省略時は中央）");
    app.add_option("-y,--y", imgY, "画像モードの切り出し左上 y（省略時は中央）");
    app.add_option("--radius", sincR, "画像モードの sinc 補間半径");
    auto* oRot = app.add_option("--rot", rot, "回転 [度]（指定すると、この1条件だけ実行）");
    auto* oSx = app.add_option("--sx", sx, "x 方向の伸縮率");
    auto* oSy = app.add_option("--sy", sy, "y 方向の伸縮率（ラインセンサーの副走査方向の速度むら）");
    app.add_flag("--bench", bench, "処理時間も測る");
    app.add_flag("--bench-only", benchOnly, "処理時間だけ測る");
    CLI11_PARSE(app, argc, argv);
    custom = oRot->count() || oSx->count() || oSy->count();

    const int margin = 32;
    const cv::Size sz(roiSize + 2 * margin, roiSize + 2 * margin);
    const cv::Rect roi(margin, margin, roiSize, roiSize);
    const cv::Point2d c(roi.x + (roi.width - 1) * 0.5, roi.y + (roi.height - 1) * 0.5);
    const auto waves = makeWaves(static_cast<unsigned>(seed));

    cv::Mat srcImg;
    if (!imagePath.empty()) {
        cv::Mat raw = cv::imread(imagePath, cv::IMREAD_ANYDEPTH | cv::IMREAD_GRAYSCALE);
        if (raw.empty()) { std::fprintf(stderr, "画像を読めません: %s\n", imagePath.c_str()); return 1; }
        raw.convertTo(srcImg, CV_64F);
        if (imgX < 0) imgX = (srcImg.cols - sz.width) / 2;
        if (imgY < 0) imgY = (srcImg.rows - sz.height) / 2;
        std::printf("画像: %s (%dx%d)、切り出し (%d, %d) から %dx%d、sinc 半径 %d\n",
                    imagePath.c_str(), srcImg.cols, srcImg.rows, imgX, imgY, sz.width, sz.height, sincR);
    } else {
        std::printf("合成画像（正弦波 3000 本の和、補間なしで厳密に変形）、ROI %dx%d\n", roiSize, roiSize);
    }
    std::printf("ROI 中心でのずれ (%.3f, %.3f) 画素、ノイズ σ=%.2f、試行 %d 回\n\n", dx, dy, noise, trials);

    auto makePair = [&](const Truth& T, cv::Mat& f, cv::Mat& g) -> bool {
        if (srcImg.empty()) { makeSynthetic(waves, sz, T, f, g); return true; }
        const cv::Rect r0(imgX, imgY, sz.width, sz.height);
        if (r0.x < 0 || r0.y < 0 || r0.x + r0.width > srcImg.cols || r0.y + r0.height > srcImg.rows) return false;
        return makeFromImage(srcImg, r0, T, sincR, f, g);
    };

    struct Case { double rot, sx, sy; const char* note; };
    std::vector<Case> cases;
    if (custom) cases.push_back({ rot, sx, sy, "指定条件" });
    else cases = {
        { 0.0,  1.0, 1.0,    "平行移動のみ" },
        { 0.1,  1.0, 1.0,    "回転 0.1°" },
        { 0.5,  1.0, 1.0,    "回転 0.5°" },
        { 1.0,  1.0, 1.0,    "回転 1°" },
        { 0.0,  1.0, 1.001,  "副走査 0.1%伸び" },
        { 0.0,  1.0, 1.01,   "副走査 1%伸び" },
        { 0.3,  1.0, 0.995,  "回転0.3°+0.5%縮み" },
    };

    if (!benchOnly) {
        std::printf("誤差 [画素]：中心 = ROI 中心、四隅 = ROI 四隅の最大。いずれも誤差ベクトルの長さ √(ex²+ey²)\n");
        std::printf("%-20s | %-17s | %-17s | %-28s | %s\n", "条件", "平行移動 中心/四隅", "アフィン 中心/四隅", "推定 回転[°] / sx / sy", "反復(並/ア)");
        std::mt19937 rng(static_cast<unsigned>(seed) + 100);
        std::normal_distribution<double> nd(0.0, 1.0);

        for (const auto& cs : cases) {
            const Truth T = makeTruth(cs.rot, cs.sx, cs.sy, cv::Point2d(dx, dy), c);
            cv::Mat f0, g0;
            if (!makePair(T, f0, g0)) { std::printf("%-20s | 画像の端に近すぎて作れません\n", cs.note); continue; }

            double sT = 0, sTc = 0, sA = 0, sAc = 0;
            IcgnAffineResult last;
            IcgnResult lastT;
            for (int t = 0; t < std::max(1, trials); ++t) {
                cv::Mat f = f0.clone(), g = g0.clone();
                if (noise > 0) {
                    for (int i = 0; i < f.rows * f.cols; ++i) {
                        f.ptr<double>()[i] += noise * nd(rng);
                        g.ptr<double>()[i] += noise * nd(rng);
                    }
                }
                const cv::Mat win = hanning(roi.size());
                const cv::Point2d init = phaseCorrelate3Point(f(roi), g(roi), win).shift;
                lastT = icgnTranslation(f, g, init, roi);
                last = icgnAffine(f, g, init, roi);
                const Errors eT = errorsTranslation(T, lastT.shift, c, roi);
                const Errors eA = errorsAffine(T, last, roi);
                sT  += eT.centerX * eT.centerX + eT.centerY * eT.centerY;
                sTc += eT.cornerMax * eT.cornerMax;
                sA  += eA.centerX * eA.centerX + eA.centerY * eA.centerY;
                sAc += eA.cornerMax * eA.cornerMax;
            }
            const int nT = std::max(1, trials);
            char est[64];
            std::snprintf(est, sizeof(est), "%+.4f / %.5f / %.5f", last.rotationDeg, last.scaleX, last.scaleY);
            std::printf("%-20s | %7.4f / %7.4f | %7.4f / %7.4f | %-28s | %d / %d%s\n", cs.note,
                        std::sqrt(sT / nT), std::sqrt(sTc / nT), std::sqrt(sA / nT), std::sqrt(sAc / nT),
                        est, lastT.iterations, last.iterations,
                        last.converged ? "" : " (アフィン未収束)");
        }
        if (trials > 1) std::printf("（ノイズありは %d 回の RMS）\n", trials);
        std::printf("\n");
    }

    if (bench || benchOnly) {
        std::printf("処理時間 [ms/回]（1スレッド、POC による初期値計算を含む）\n");
        std::printf("%-10s | %-12s | %-12s | %-14s | %-14s | %s\n", "ROI", "POC のみ", "平行移動", "アフィン", "アフィン/平行", "反復(並/ア)");
        for (int s : { 64, 128, 256 }) {
            const cv::Size bsz(s + 2 * margin, s + 2 * margin);
            const cv::Rect broi(margin, margin, s, s);
            const cv::Point2d bc(broi.x + (s - 1) * 0.5, broi.y + (s - 1) * 0.5);
            const Truth T = makeTruth(0.3, 1.0, 1.003, cv::Point2d(dx, dy), bc);
            cv::Mat f, g;
            makeSynthetic(waves, bsz, T, f, g);
            const cv::Mat win = hanning(broi.size());
            const int reps = s <= 64 ? 200 : (s <= 128 ? 50 : 15);
            IcgnResult rt; IcgnAffineResult ra;
            const double tPoc = timeMs([&] { volatile double v = phaseCorrelate3Point(f(broi), g(broi), win).shift.x; (void)v; }, reps);
            const double tT = timeMs([&] {
                const cv::Point2d init = phaseCorrelate3Point(f(broi), g(broi), win).shift;
                rt = icgnTranslation(f, g, init, broi); }, reps);
            const double tA = timeMs([&] { ra = phaseCorrelateICGNAffine(f, g, win, broi); }, reps);
            char name[16];
            std::snprintf(name, sizeof(name), "%dx%d", s, s);
            std::printf("%-10s | %10.3f   | %10.3f   | %12.3f   | %12.1f倍  | %d / %d\n", name, tPoc, tT, tA, tA / tT,
                        rt.iterations, ra.iterations);
        }
        std::printf("（ベンチの条件：回転 0.3°、副走査 0.3%%伸び）\n");
    }
    return 0;
}

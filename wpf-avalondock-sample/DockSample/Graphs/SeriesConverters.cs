using DockSample.Settings;

namespace DockSample.Graphs;

/// <summary>グラフの線1本（ScottPlot の型は使わない）</summary>
public record Series(string Name, double[] Xs, double[] Ys);

/// <summary>下の編集欄で選んでいるセグメント（グラフで目立たせる）。Pulse は何番目のパルス、Segment はその中の何番目か</summary>
public record SegmentSelection(int Pulse, int Segment);

/// <summary>DC の編集欄で選んでいる信号（グラフで目立たせる）。Index は何番目の信号か</summary>
public record SignalSelection(int Index);

/// <summary>1方向（X または Y）の動かし方：刻みと範囲</summary>
public record AxisRule(double Step, double Min = double.MinValue, double Max = double.MaxValue)
{
    /// <summary>刻みに合わせて、範囲に収める（0.1 刻みの丸めで出る 0.30000000000000004 のような誤差も消す）</summary>
    public double Snap(double value) =>
        Math.Clamp(Math.Round(Math.Round(value / Step) * Step, 10), Min, Max);
}

/// <summary>
/// グラフ上の動かせる点（ハンドル）。
/// Id はどのデータの点かを表す（機種ごとのクラスだけが意味を知っている）。XRule / YRule が null の方向には動かせない。
/// 両方 null なら動かせない点（印だけ付ける。グラフでは灰色の丸）。
/// </summary>
public record Handle(string Id, double X, double Y, AxisRule? XRule, AxisRule? YRule)
{
    /// <summary>どちらの方向にも動かせない点か</summary>
    public bool IsFixed => XRule is null && YRule is null;

    /// <summary>マウスの位置を、動かせる方向・刻み・範囲に合わせた位置にする（動かせない方向は元のまま）</summary>
    public (double X, double Y) Snap(double x, double y) =>
        (XRule?.Snap(x) ?? X, YRule?.Snap(y) ?? Y);
}

/// <summary>
/// 機種ごとのグラフの定義。機種ごとに1クラス作り、DI に ISeriesConverter として登録する。
/// ・データ → 線（Convert）
/// ・データ → 動かせる点（GetHandles）と、動かしたときのデータの書き換え方（Move）
/// グラフ側（GraphViewModel / GraphView）は Series と Handle だけを扱うので、機種が増えても変えなくてよい。
/// </summary>
public interface ISeriesConverter
{
    bool CanConvert(DeviceData data);
    IReadOnlyList<Series> Convert(DeviceData data);
    IReadOnlyList<Handle> GetHandles(DeviceData data);

    /// <summary>handle を (x, y) に動かした結果をデータに書き込む（x, y は Snap 済み）</summary>
    void Move(DeviceData data, Handle handle, double x, double y);

    /// <summary>選んでいるもの（SegmentSelection など）がグラフのどこかを、線（点付き）で返す。目立たせて描かれる</summary>
    IReadOnlyList<Series> GetHighlight(DeviceData data, object? selection);

    /// <summary>
    /// 書き換えたあとのデータを、その機種の決まりに合わせて直す（点が重ならないように最小の間隔を空ける など）。
    /// GraphViewModel が書き換えるたびに、同じ Undo 1回分の中で呼ぶ
    /// </summary>
    void Normalize(DeviceData data);
}

/// <summary>型の判定とキャストをまとめた基底。派生クラスは T 版だけ書けばよい（動かせる点がない機種は Convert だけ）</summary>
public abstract class SeriesConverter<T> : ISeriesConverter where T : DeviceData
{
    public bool CanConvert(DeviceData data) => data is T;
    public IReadOnlyList<Series> Convert(DeviceData data) => Convert((T)data);
    public IReadOnlyList<Handle> GetHandles(DeviceData data) => GetHandles((T)data);
    public void Move(DeviceData data, Handle handle, double x, double y) => Move((T)data, handle, x, y);
    public IReadOnlyList<Series> GetHighlight(DeviceData data, object? selection) => GetHighlight((T)data, selection);
    public void Normalize(DeviceData data) => Normalize((T)data);

    protected abstract IReadOnlyList<Series> Convert(T data);
    protected virtual IReadOnlyList<Handle> GetHandles(T data) => Array.Empty<Handle>();
    protected virtual void Move(T data, Handle handle, double x, double y) { }
    protected virtual IReadOnlyList<Series> GetHighlight(T data, object? selection) => Array.Empty<Series>();
    protected virtual void Normalize(T data) { }
}

/// <summary>
/// パルスを持つ機種：パルスを順番につなげて、1本の波形にする。
/// 次のパルスは、前のパルスが終わった時刻・電圧から続ける。
///
/// グラフの時間は µs（ファイルの Duration は ns なので換算する。SlewRate は V/µs のまま使える）。
///
/// 時間 0 の電圧は restingVoltage（最初のセグメントはそこから始まる）。
/// 動かせる点：最初の点 (0, restingVoltage) 以外の全部の点。縦横とも 0.1 刻み（横は 0.1µs = 100ns、縦は 0.1V）。
/// 点が重ならないように、隣り合う点の間は少なくとも 0.1µs 空ける（Normalize・ドラッグの範囲）。
/// ・傾斜の終わりの点（Id "k.ramp"）と、セグメントの終わりの点（Id "k.end"）。k は全パルスを通したセグメントの通し番号
/// ・動かすのはその点だけ。ほかの点の位置が変わらないように、前後のセグメントのスルーレート・時間を計算し直す
///   （ただし同じセグメントの「傾斜の終わり」と「終わり」は同じ電圧なので、縦に動かすと両方動く）
/// </summary>
public class WaveformSeriesConverter : SeriesConverter<WaveformData>
{
    /// <summary>縦横の刻み（横は µs、縦は V）</summary>
    private const double Step = 0.1;

    /// <summary>
    /// セグメント1つ分の形（全パルスを通した時刻。単位は µs）。
    /// restingVoltage（または前の終わりの電圧）の Start から、RampEnd まで傾斜し、End まで VEnd を保つ。
    /// SlewRate は V/µs の傾き（0 以下なら即座に変わる）、Duration はそのセグメントの長さ（ファイルは ns）。
    ///   傾斜が Duration に収まらなければ途中で打ち切る（そのときは RampEnd == End、VEnd は途中の電圧）。
    /// </summary>
    private sealed record Shape(SegmentData Segment, double Start, double RampEnd, double End, double VStart, double VEnd)
    {
        public bool Truncated => RampEnd >= End;
    }

    /// <summary>全パルスのセグメントを順に並べ、それぞれの形を計算する</summary>
    private static List<Shape> Layout(WaveformData data)
    {
        var list = new List<Shape>();
        double t = 0, v = data.RestingVoltage;   // 今の時刻と電圧（時間 0 は restingVoltage。パルスをまたいで引き継ぐ）
        foreach (var seg in data.Pulses.SelectMany(p => p.Segments))
        {
            var end = t + Math.Max(0, TimeUnit.NsToUs(seg.Duration));
            var diff = seg.TargetVoltage - v;
            var rampTime = seg.SlewRate > 0 ? Math.Abs(diff) / seg.SlewRate : 0;

            double rampEnd, vEnd;
            if (t + rampTime >= end && rampTime > 0)
            {
                rampEnd = end;
                vEnd = v + Math.Sign(diff) * seg.SlewRate * (end - t);   // 傾斜の途中で終わる
            }
            else
            {
                rampEnd = t + rampTime;
                vEnd = seg.TargetVoltage;
            }

            list.Add(new Shape(seg, t, rampEnd, end, v, vEnd));
            t = end;
            v = vEnd;
        }
        return list;
    }

    protected override IReadOnlyList<Series> Convert(WaveformData data)
    {
        var xs = new List<double> { 0 };
        var ys = new List<double> { data.RestingVoltage };
        foreach (var s in Layout(data))
        {
            xs.Add(s.RampEnd); ys.Add(s.VEnd);                 // 傾斜の終わり
            if (!s.Truncated) { xs.Add(s.End); ys.Add(s.VEnd); }   // 保持の終わり
        }
        return new[] { new Series("波形", xs.ToArray(), ys.ToArray()) };
    }

    protected override IReadOnlyList<Handle> GetHandles(WaveformData data)
    {
        var shapes = Layout(data);
        // 始まりの (0, restingVoltage)。グラフでは動かせない（値は編集欄の「初期電圧」で変える）
        var handles = new List<Handle> { new("start", 0, data.RestingVoltage, null, null) };
        var y = new AxisRule(Step);   // 電圧：範囲の指定なし

        for (var k = 0; k < shapes.Count; k++)
        {
            var s = shapes[k];
            // 隣の点とは少なくとも Step（0.1µs）空ける（重ならないように）。
            // 次のセグメントの傾斜の終わりの手前まで。次に傾斜がない（電圧が変わらない）なら、次の終わりの 2 Step 手前まで
            //（次の電圧が変わって傾斜ができたときに、傾斜と保持それぞれに Step 分の時間を残すため）
            var nextLimit = k + 1 >= shapes.Count ? double.MaxValue
                          : HasRamp(shapes[k + 1]) ? shapes[k + 1].RampEnd - Step
                          : shapes[k + 1].End - 2 * Step;

            // 傾斜の終わり：傾斜がある（電圧が変わる）ときだけ。始まり＋Step 〜 終わり－Step
            if (!s.Truncated && HasRamp(s))
                handles.Add(new Handle($"{k}.ramp", s.RampEnd, s.VEnd, XRule(s.Start + Step, s.End - Step, s.RampEnd), y));
            // 終わり：傾斜の終わり（傾斜がなければ始まり）＋Step 〜 nextLimit
            var endMin = (s.Truncated || !HasRamp(s) ? s.Start : s.RampEnd) + Step;
            handles.Add(new Handle($"{k}.end", s.End, s.VEnd, XRule(endMin, nextLimit, s.End), y));
        }
        return handles;
    }

    protected override void Move(WaveformData data, Handle handle, double x, double y)
    {
        var shapes = Layout(data);
        var parts = handle.Id.Split('.');
        if (parts.Length != 2 || !int.TryParse(parts[0], out var k) || k < 0 || k >= shapes.Count) return;

        var s = shapes[k];
        var next = k + 1 < shapes.Count ? shapes[k + 1] : null;

        if (parts[1] == "ramp")
        {
            // 傾斜の終わりを (x, y) に。セグメントの終わりの時刻はそのまま
            SetRamp(s.Segment, s.VStart, y, x - s.Start);
            if (next is not null) SetRamp(next.Segment, y, next.VEnd, next.RampEnd - s.End);   // 次の傾斜の終わりを保つ
        }
        else
        {
            // セグメントの終わりを (x, y) に。傾斜の終わりの時刻はそのまま（打ち切りのときは終わりまで傾斜）
            var rampEnd = s.Truncated ? x : s.RampEnd;
            s.Segment.Duration = TimeUnit.UsToNs(x - s.Start);
            SetRamp(s.Segment, s.VStart, y, rampEnd - s.Start);

            if (next is not null)
            {
                // 次のセグメントは、終わりの時刻と傾斜の終わりの時刻を保つ（そのぶん長さが縮む・伸びる）。
                // 傾斜なし（即座に変わる）なら傾斜なしのまま
                next.Segment.Duration = TimeUnit.UsToNs(next.End - x);
                var nextRampEnd = next.Truncated ? next.End : HasRamp(next) ? next.RampEnd : x;
                SetRamp(next.Segment, y, next.VEnd, nextRampEnd - x);
            }
        }
    }

    /// <summary>選んでいるセグメントの部分（始まり → 傾斜の終わり → 終わり）</summary>
    protected override IReadOnlyList<Series> GetHighlight(WaveformData data, object? selection)
    {
        if (selection is not SegmentSelection sel || sel.Pulse >= data.Pulses.Count) return Array.Empty<Series>();

        // 全パルスを通した通し番号にする（Layout はパルスをまたいで並べているので）
        var k = data.Pulses.Take(sel.Pulse).Sum(p => p.Segments.Count) + sel.Segment;
        var shapes = Layout(data);
        if (sel.Segment >= data.Pulses[sel.Pulse].Segments.Count || k >= shapes.Count) return Array.Empty<Series>();

        var s = shapes[k];
        var xs = new List<double> { s.Start, s.RampEnd };
        var ys = new List<double> { s.VStart, s.VEnd };
        if (!s.Truncated) { xs.Add(s.End); ys.Add(s.VEnd); }
        return new[] { new Series("選択中", xs.ToArray(), ys.ToArray()) };
    }

    /// <summary>横方向の決まり。範囲が作れない（最小 &gt; 最大）ほど詰まっているときは、今の位置から動かさない</summary>
    private static AxisRule XRule(double min, double max, double current) =>
        min <= max ? new AxisRule(Step, min, max) : new AxisRule(Step, current, current);

    /// <summary>
    /// 点が重ならないように、隣り合う点の間を少なくとも Step（0.1µs）空ける。
    /// ・電圧が変わるセグメント：傾斜の時間を Step 以上にする（短すぎればスルーレートを下げる）。
    ///   保持の時間（duration − 傾斜）も Step 以上にする（短すぎれば duration を伸ばす）
    /// ・電圧が変わらないセグメント：duration を Step 以上にする
    /// </summary>
    protected override void Normalize(WaveformData data)
    {
        const double eps = 1e-9;
        var v = data.RestingVoltage;
        foreach (var seg in data.Pulses.SelectMany(p => p.Segments))
        {
            var dv = Math.Abs(seg.TargetVoltage - v);
            var duration = Math.Max(0, TimeUnit.NsToUs(seg.Duration));
            if (dv > eps)
            {
                var ramp = seg.SlewRate > 0 ? dv / seg.SlewRate : 0;
                if (ramp < Step - eps)
                {
                    seg.SlewRate = dv / Step;   // 傾斜を Step にする（スルーレートを下げる）
                    ramp = Step;
                }
                if (duration < ramp + Step - eps)
                    seg.Duration = TimeUnit.UsToNs(Math.Round(ramp + Step, 10));   // 保持を Step 残す
            }
            else if (duration < Step - eps)
            {
                seg.Duration = TimeUnit.UsToNs(Step);
            }
            v = seg.TargetVoltage;
        }
    }

    /// <summary>傾斜があるか（傾斜なし＝セグメントの始まりで即座に変わる）</summary>
    private static bool HasRamp(Shape s) => s.RampEnd > s.Start;

    /// <summary>vFrom から vTo まで rampTime（µs）かけて変化するように、目標電圧とスルーレート（V/µs）を決める</summary>
    private static void SetRamp(SegmentData seg, double vFrom, double vTo, double rampTime)
    {
        seg.TargetVoltage = vTo;
        seg.SlewRate = rampTime > 0 && vTo != vFrom ? Math.Abs(vTo - vFrom) / rampTime : 0;   // 0 = 即座に変わる
    }
}

/// <summary>
/// DC の機種：信号（Hi = 2V / Low = −2V / Zero = 0V）を順に並べた矩形波。
/// 各信号はその電圧を duration の間保ち、次の信号との境目で垂直に切り替わる。時間は µs（ファイルの duration は ns）。
/// 今はグラフでは動かせない（点は印だけ。動かせない点として灰色の四角で出す）
/// </summary>
public class DcSourceSeriesConverter : SeriesConverter<DcSourceData>
{
    protected override IReadOnlyList<Series> Convert(DcSourceData data)
    {
        var points = Corners(data);
        if (points.Count == 0) return Array.Empty<Series>();
        return new[] { new Series("信号", points.Select(p => p.X).ToArray(), points.Select(p => p.Y).ToArray()) };
    }

    /// <summary>選んでいる信号の部分（始まり → 終わりの横線）</summary>
    protected override IReadOnlyList<Series> GetHighlight(DcSourceData data, object? selection)
    {
        if (selection is not SignalSelection sel) return Array.Empty<Series>();
        var points = Corners(data);   // 信号ごとに2点ずつ（始まり・終わり）
        var i = sel.Index * 2;
        if (i + 1 >= points.Count) return Array.Empty<Series>();
        return new[]
        {
            new Series("選択中", new[] { points[i].X, points[i + 1].X }, new[] { points[i].Y, points[i + 1].Y }),
        };
    }

    /// <summary>点が重ならないように、信号の時間を少なくとも 0.1µs にする</summary>
    protected override void Normalize(DcSourceData data)
    {
        const double minUs = 0.1;
        foreach (var signal in data.Signals)
            if (TimeUnit.NsToUs(signal.Duration) < minUs - 1e-9)
                signal.Duration = TimeUnit.UsToNs(minUs);
    }

    /// <summary>角の点に印を付ける（動かせない点）</summary>
    protected override IReadOnlyList<Handle> GetHandles(DcSourceData data) =>
        Corners(data).Select((p, i) => new Handle($"corner{i}", p.X, p.Y, null, null)).ToList();

    /// <summary>矩形波の角の点：信号ごとに (始まり, 電圧) と (終わり, 電圧)</summary>
    private static List<(double X, double Y)> Corners(DcSourceData data)
    {
        var points = new List<(double X, double Y)>();
        double t = 0;
        foreach (var signal in data.Signals)
        {
            var v = DcLevel.ToVoltage(signal.Level);
            var end = t + Math.Max(0, TimeUnit.NsToUs(signal.Duration));
            points.Add((t, v));     // 始まり（前の信号の終わりと同じ時刻なので、そこで垂直に切り替わる）
            points.Add((end, v));   // 終わり
            t = end;
        }
        return points;
    }
}

using DockSample.Settings;

namespace DockSample.Graphs;

/// <summary>グラフの線1本（ScottPlot の型は使わない）</summary>
public record Series(string Name, double[] Xs, double[] Ys);

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
/// </summary>
public record Handle(string Id, double X, double Y, AxisRule? XRule, AxisRule? YRule)
{
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
}

/// <summary>型の判定とキャストをまとめた基底。派生クラスは T 版だけ書けばよい（動かせる点がない機種は Convert だけ）</summary>
public abstract class SeriesConverter<T> : ISeriesConverter where T : DeviceData
{
    public bool CanConvert(DeviceData data) => data is T;
    public IReadOnlyList<Series> Convert(DeviceData data) => Convert((T)data);
    public IReadOnlyList<Handle> GetHandles(DeviceData data) => GetHandles((T)data);
    public void Move(DeviceData data, Handle handle, double x, double y) => Move((T)data, handle, x, y);

    protected abstract IReadOnlyList<Series> Convert(T data);
    protected virtual IReadOnlyList<Handle> GetHandles(T data) => Array.Empty<Handle>();
    protected virtual void Move(T data, Handle handle, double x, double y) { }
}

/// <summary>
/// パルスを持つ機種：パルスを順番につなげて、1本の波形にする。
/// 次のパルスは、前のパルスが終わった時刻・電圧から続ける。
///
/// 動かせる点：最初の (0, 0) 以外の全部の点。縦横とも 0.1 刻み。
/// ・傾斜の終わりの点（Id "k.ramp"）と、セグメントの終わりの点（Id "k.end"）。k は全パルスを通したセグメントの通し番号
/// ・動かすのはその点だけ。ほかの点の位置が変わらないように、前後のセグメントのスルーレート・時間を計算し直す
///   （ただし同じセグメントの「傾斜の終わり」と「終わり」は同じ電圧なので、縦に動かすと両方動く）
/// </summary>
public class WaveformSeriesConverter : SeriesConverter<WaveformData>
{
    /// <summary>縦横の刻み</summary>
    private const double Step = 0.1;

    /// <summary>
    /// セグメント1つ分の形（全パルスを通した時刻）。
    /// 0V（または前の終わりの電圧）の Start から、RampEnd まで傾斜し、End まで VEnd を保つ。
    /// ※ 仮の解釈：SlewRate は 電圧/時間 の傾き（0 以下なら即座に変わる）、Duration はそのセグメントの長さ。
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
        double t = 0, v = 0;   // 今の時刻と電圧（パルスをまたいで引き継ぐ）
        foreach (var seg in data.Pulses.SelectMany(p => p.Segments))
        {
            var end = t + Math.Max(0, seg.Duration);
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
        var ys = new List<double> { 0 };
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
        var handles = new List<Handle>();
        var y = new AxisRule(Step);   // 電圧：範囲の指定なし

        for (var k = 0; k < shapes.Count; k++)
        {
            var s = shapes[k];
            // 次のセグメントの傾斜の終わりより右には行けない（追い越すと形が作れない）。
            // 次が即座に変わる（傾斜なし）なら、次のセグメントの終わりまで動かせる（次は傾斜なしのまま）
            var nextLimit = k + 1 >= shapes.Count ? double.MaxValue
                          : HasRamp(shapes[k + 1]) ? shapes[k + 1].RampEnd
                          : shapes[k + 1].End;

            if (!s.Truncated)
                handles.Add(new Handle($"{k}.ramp", s.RampEnd, s.VEnd, new AxisRule(Step, s.Start, s.End), y));
            handles.Add(new Handle($"{k}.end", s.End, s.VEnd,
                                   new AxisRule(Step, s.Truncated ? s.Start : s.RampEnd, nextLimit), y));
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
            s.Segment.Duration = Math.Round(x - s.Start, 10);
            SetRamp(s.Segment, s.VStart, y, rampEnd - s.Start);

            if (next is not null)
            {
                // 次のセグメントは、終わりの時刻と傾斜の終わりの時刻を保つ（そのぶん長さが縮む・伸びる）。
                // 傾斜なし（即座に変わる）なら傾斜なしのまま
                next.Segment.Duration = Math.Round(next.End - x, 10);
                var nextRampEnd = next.Truncated ? next.End : HasRamp(next) ? next.RampEnd : x;
                SetRamp(next.Segment, y, next.VEnd, nextRampEnd - x);
            }
        }
    }

    /// <summary>傾斜があるか（傾斜なし＝セグメントの始まりで即座に変わる）</summary>
    private static bool HasRamp(Shape s) => s.RampEnd > s.Start;

    /// <summary>vFrom から vTo まで rampTime かけて変化するように、目標電圧とスルーレートを決める</summary>
    private static void SetRamp(SegmentData seg, double vFrom, double vTo, double rampTime)
    {
        seg.TargetVoltage = vTo;
        seg.SlewRate = rampTime > 0 && vTo != vFrom ? Math.Abs(vTo - vFrom) / rampTime : 0;   // 0 = 即座に変わる
    }
}

/// <summary>
/// 電圧源：一定電圧の横線1本（時間の情報がないので、横幅は仮に 0〜1000）。
/// 動かせる点：線の真ん中の1点を縦にだけ（0.1 刻み）。電圧が変わる
/// </summary>
public class DcSourceSeriesConverter : SeriesConverter<DcSourceData>
{
    private const double Width = 1000;

    protected override IReadOnlyList<Series> Convert(DcSourceData data) =>
        new[] { new Series("出力", new[] { 0.0, Width }, new[] { data.Voltage, data.Voltage }) };

    protected override IReadOnlyList<Handle> GetHandles(DcSourceData data) =>
        new[] { new Handle("voltage", Width / 2, data.Voltage, XRule: null, YRule: new AxisRule(0.1)) };

    protected override void Move(DcSourceData data, Handle handle, double x, double y) => data.Voltage = y;
}

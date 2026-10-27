using DockSample.Settings;

namespace DockSample.Graphs;

/// <summary>グラフの線1本（ScottPlot の型は使わない）</summary>
public record Series(string Name, double[] Xs, double[] Ys);

/// <summary>
/// 機種のデータ → グラフの線。機種ごとに1クラス作り、DI に ISeriesConverter として登録する。
/// グラフ側（GraphViewModel / GraphView）は Series だけを扱うので、機種が増えても変えなくてよい。
/// </summary>
public interface ISeriesConverter
{
    bool CanConvert(DeviceData data);
    IReadOnlyList<Series> Convert(DeviceData data);
}

/// <summary>型の判定とキャストをまとめた基底。派生クラスは Convert(T) だけ書けばよい</summary>
public abstract class SeriesConverter<T> : ISeriesConverter where T : DeviceData
{
    public bool CanConvert(DeviceData data) => data is T;
    public IReadOnlyList<Series> Convert(DeviceData data) => Convert((T)data);
    protected abstract IReadOnlyList<Series> Convert(T data);
}

/// <summary>パルスを持つ機種：パルス1つ = 線1本（1つのグラフに重ねて描かれる）</summary>
public class WaveformSeriesConverter : SeriesConverter<WaveformData>
{
    protected override IReadOnlyList<Series> Convert(WaveformData data) =>
        data.Pulses.Select(p =>
        {
            var (xs, ys) = ToPoints(p);
            return new Series(p.Name, xs, ys);
        }).ToList();

    /// <summary>
    /// セグメントの並びを、グラフの点（時刻, 電圧）にする。
    /// ※ 仮の解釈：0V から始め、各セグメントは前の電圧から SlewRate（電圧/時間）の傾きで TargetVoltage まで変化し、
    ///   Duration（そのセグメントの長さ）が終わるまでその電圧を保つ。傾斜が Duration に収まらなければ途中で打ち切る。
    ///   単位や意味が違えばここを直す。
    /// </summary>
    private static (double[] Xs, double[] Ys) ToPoints(PulseData pulse)
    {
        var xs = new List<double> { 0 };
        var ys = new List<double> { 0 };
        double t = 0, v = 0;

        foreach (var seg in pulse.Segments)
        {
            var end = t + Math.Max(0, seg.Duration);
            var diff = seg.TargetVoltage - v;
            var rampTime = seg.SlewRate > 0 ? Math.Abs(diff) / seg.SlewRate : 0;   // 0 以下なら即座に変わる

            if (t + rampTime >= end)
            {
                // 傾斜の途中でセグメントが終わる
                v += seg.SlewRate > 0 ? Math.Sign(diff) * seg.SlewRate * (end - t) : diff;
                xs.Add(end); ys.Add(v);
            }
            else
            {
                v = seg.TargetVoltage;
                xs.Add(t + rampTime); ys.Add(v);   // 傾斜の終わり
                xs.Add(end);          ys.Add(v);   // 保持の終わり
            }
            t = end;
        }
        return (xs.ToArray(), ys.ToArray());
    }
}

/// <summary>電圧源：一定電圧の横線1本（時間の情報がないので、横幅は仮に 0〜1000）</summary>
public class DcSourceSeriesConverter : SeriesConverter<DcSourceData>
{
    protected override IReadOnlyList<Series> Convert(DcSourceData data) =>
        new[] { new Series("出力", new[] { 0.0, 1000 }, new[] { data.Voltage, data.Voltage }) };
}

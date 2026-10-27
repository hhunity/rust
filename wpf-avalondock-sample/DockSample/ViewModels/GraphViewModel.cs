using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Services;
using DockSample.Settings;

namespace DockSample.ViewModels;

/// <summary>一覧の1件。データそのものではなく、場所（キー）だけを覚える</summary>
public record PulseEntry(string Device, string Uuid, int Index, string Name)
{
    public override string ToString() => $"{Device} / {Uuid} / {Name}";
}

/// <summary>
/// 「グラフ」ペイン。設定の waveforms.json にあるパルスを選んで、波形をグラフに出す。
///
/// 設定は「元に戻す」「開く」で Current が丸ごと差し替わるので、PulseData そのものは覚えておかず、
/// 場所（デバイス名・uuid・番号）で覚えて、使うたびに Current から探し直す（CurrentPulse）。
/// グラフを描くのは View（GraphView のコードビハインド）。ここは描くデータ（Xs / Ys）だけを用意する。
/// </summary>
public partial class GraphViewModel : PaneViewModel
{
    private readonly ISettingsService _settings;
    private bool _loading;

    public GraphViewModel(ISettingsService settings)
        : base("Document_Graph", "グラフ", DockLocation.Document, buttonText: "グラフ")
    {
        _settings = settings;
        settings.Reloaded += (_, _) => Load();   // 元に戻す・開く → 一覧を作り直す
        Load();
    }

    /// <summary>パルスの一覧（パルスを持つ機種のものだけ）</summary>
    public ObservableCollection<PulseEntry> Pulses { get; } = new();

    [ObservableProperty] private PulseEntry? _selectedPulse;

    [ObservableProperty] private string _yLabel = "電圧 (V)";

    /// <summary>グラフに出すデータ（ScottPlot の型は使わない）</summary>
    public double[] Xs { get; private set; } = Array.Empty<double>();
    public double[] Ys { get; private set; } = Array.Empty<double>();

    /// <summary>データが変わった（View はこれを受けて描き直す）</summary>
    public event EventHandler? DataChanged;

    /// <summary>選んでいるパルスの今のデータ（毎回 Current から探す）</summary>
    public PulseData? CurrentPulse =>
        SelectedPulse is { } p
        && _settings.Current.Waveforms.Waveforms.TryGetValue(p.Device, out var byUuid)
        && byUuid.TryGetValue(p.Uuid, out var data)
        && data is WaveformData waveform
        && p.Index < waveform.Pulses.Count
            ? waveform.Pulses[p.Index]
            : null;

    partial void OnSelectedPulseChanged(PulseEntry? value) => UpdatePoints();

    private void Load()
    {
        var selected = SelectedPulse;
        _loading = true;
        try
        {
            Pulses.Clear();
            foreach (var (device, byUuid) in _settings.Current.Waveforms.Waveforms)
                foreach (var (uuid, data) in byUuid)
                    if (data is WaveformData waveform)
                        for (var i = 0; i < waveform.Pulses.Count; i++)
                            Pulses.Add(new PulseEntry(device, uuid, i, waveform.Pulses[i].Name));

            // 同じ場所のパルスがあれば選び直す（record なので中身で比べられる）
            SelectedPulse = Pulses.FirstOrDefault(p => p == selected) ?? Pulses.FirstOrDefault();
        }
        finally { _loading = false; }

        UpdatePoints();   // 選び直したパルスが同じでも、中身は変わっているかもしれないので描き直す
    }

    /// <summary>
    /// 画面 → 設定。書き換える前の状態を Undo 用に記録し、Current を書き換えて「未保存」を更新する。
    /// （設定ページの Write と同じ形）
    /// </summary>
    private void Write(Action<PulseData> write, string key)
    {
        if (_loading || CurrentPulse is not { } pulse) return;
        _settings.RecordUndo($"Waveforms.{SelectedPulse}.{key}");
        write(pulse);
        _settings.NotifyChanged();
        UpdatePoints();
    }

    /// <summary>例：セグメントの電圧を変える</summary>
    public void SetTargetVoltage(int segment, double value) =>
        Write(p => p.Segments[segment].TargetVoltage = value, $"{segment}.TargetVoltage");

    private void UpdatePoints()
    {
        (Xs, Ys) = ToPoints(CurrentPulse);
        DataChanged?.Invoke(this, EventArgs.Empty);
    }

    /// <summary>
    /// セグメントの並びを、グラフの点（時刻, 電圧）にする。
    /// ※ 仮の解釈：0V から始め、各セグメントは前の電圧から SlewRate（電圧/時間）の傾きで TargetVoltage まで変化し、
    ///   Duration（そのセグメントの長さ）が終わるまでその電圧を保つ。傾斜が Duration に収まらなければ途中で打ち切る。
    ///   単位や意味が違えばここを直す。
    /// </summary>
    private static (double[] Xs, double[] Ys) ToPoints(PulseData? pulse)
    {
        if (pulse is null) return (Array.Empty<double>(), Array.Empty<double>());

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

using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Graphs;
using DockSample.Services;
using DockSample.Settings;

namespace DockSample.ViewModels;

/// <summary>一覧の1件（デバイス名 / uuid）。データそのものではなく、場所（キー）だけを覚える</summary>
public record DeviceEntry(string Device, string Uuid)
{
    public override string ToString() => $"{Device} / {Uuid}";
}

/// <summary>
/// 「グラフ」ペイン。設定の waveforms.json にあるデバイス / uuid を選ぶと、その中身を1つのグラフに描く
/// （パルスを持つ機種なら、パルスを順番につなげた1本の波形になる）。
///
/// ・データ → 線 の変換は機種ごとの ISeriesConverter（Graphs/SeriesConverters.cs）。ここは機種を知らない
/// ・設定は「元に戻す」「開く」で Current が丸ごと差し替わるので、データそのものは覚えておかず、
///   場所（デバイス名・uuid）で覚えて、使うたびに Current から探し直す（CurrentData）
/// ・グラフを描くのは View（GraphView のコードビハインド）。ここは描く線（Lines）だけを用意する
/// </summary>
public partial class GraphViewModel : PaneViewModel
{
    private readonly ISettingsService _settings;
    private readonly IReadOnlyList<ISeriesConverter> _converters;

    public GraphViewModel(ISettingsService settings, IEnumerable<ISeriesConverter> converters)
        : base("Document_Graph", "グラフ", DockLocation.Document, buttonText: "グラフ")
    {
        _settings = settings;
        _converters = converters.ToList();
        settings.Reloaded += (_, _) => Load();   // 元に戻す・開く → 一覧を作り直す
        Load();
    }

    /// <summary>デバイス / uuid の一覧</summary>
    public ObservableCollection<DeviceEntry> Entries { get; } = new();

    [ObservableProperty] private DeviceEntry? _selectedEntry;

    [ObservableProperty] private string _yLabel = "電圧 (V)";

    /// <summary>グラフに出す線（選んだ uuid の中身を変換したもの）</summary>
    public IReadOnlyList<Series> Lines { get; private set; } = Array.Empty<Series>();

    /// <summary>選んだ uuid に、グラフにできるもの（変換クラスがある機種）がないとき true</summary>
    public bool HasNoSeries => SelectedEntry is not null && Lines.Count == 0;

    /// <summary>線が変わった（View はこれを受けて描き直す）</summary>
    public event EventHandler? DataChanged;

    /// <summary>選んでいる uuid の今のデータ（毎回 Current から探す）</summary>
    public DeviceData? CurrentData =>
        SelectedEntry is { } e
        && _settings.Current.Waveforms.Waveforms.TryGetValue(e.Device, out var byUuid)
        && byUuid.TryGetValue(e.Uuid, out var data)
            ? data
            : null;

    partial void OnSelectedEntryChanged(DeviceEntry? value) => UpdateSeries();

    private void Load()
    {
        var selected = SelectedEntry;
        Entries.Clear();
        foreach (var (device, byUuid) in _settings.Current.Waveforms.Waveforms)
            foreach (var uuid in byUuid.Keys)
                Entries.Add(new DeviceEntry(device, uuid));

        // 同じ場所があれば選び直す（record なので中身で比べられる）
        SelectedEntry = Entries.FirstOrDefault(e => e == selected) ?? Entries.FirstOrDefault();

        UpdateSeries();   // 選び直した場所が同じでも、中身は変わっているかもしれないので描き直す
    }

    /// <summary>
    /// 画面 → 設定。選んでいる uuid の中身が T 型のときだけ書き換える（Undo・「未保存」は ISettingsService.Edit が扱う）
    /// </summary>
    private void Write<T>(Action<T> write, string key) where T : DeviceData
    {
        if (CurrentData is not T data) return;
        _settings.Edit($"Waveforms.{SelectedEntry}.{key}", _ => write(data));
        UpdateSeries();
    }

    /// <summary>例：パルスのセグメントの電圧を変える</summary>
    public void SetTargetVoltage(int pulse, int segment, double value) =>
        Write<WaveformData>(d => d.Pulses[pulse].Segments[segment].TargetVoltage = value,
                            $"{pulse}.{segment}.TargetVoltage");

    private void UpdateSeries()
    {
        Lines = CurrentData is { } data
            ? _converters.FirstOrDefault(c => c.CanConvert(data))?.Convert(data) ?? Array.Empty<Series>()
            : Array.Empty<Series>();
        OnPropertyChanged(nameof(HasNoSeries));
        DataChanged?.Invoke(this, EventArgs.Empty);
    }
}

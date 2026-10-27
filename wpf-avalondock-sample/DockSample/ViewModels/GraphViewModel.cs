using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using DockSample.Graphs;
using DockSample.Services;
using DockSample.Settings;

namespace DockSample.ViewModels;

/// <summary>
/// 一覧の1件。データそのものではなく、場所（デバイス名・uuid）と、一覧に出す名前（DeviceData.Name）だけを覚える。
/// 名前は変わることがあるので、同じものかどうかは IsSameLocation（場所）で比べる。
/// </summary>
public partial class DeviceEntry : ObservableObject
{
    public DeviceEntry(string device, string uuid, string name)
    {
        Device = device;
        Uuid = uuid;
        _name = name;
    }

    public string Device { get; }
    public string Uuid { get; }

    /// <summary>表示名（名前を変えたら、コンボボックスの表示もすぐ変わる）</summary>
    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Display))]
    private string _name;

    /// <summary>コンボボックスに出す文字（名前が空なら uuid）</summary>
    public string Display => $"{Device} / {(string.IsNullOrEmpty(Name) ? Uuid : Name)}";

    public bool IsSameLocation(DeviceEntry? other) => other is not null && other.Device == Device && other.Uuid == Uuid;

    public override string ToString() => Display;
}

/// <summary>
/// 「グラフ」ペイン。設定の waveforms.json にあるデバイス / uuid を選ぶと、その中身を1つのグラフに描く
/// （パルスを持つ機種なら、パルスを順番につなげた1本の波形になる）。下の編集欄で中身を編集できる。
///
/// ・データ → 線 の変換は機種ごとの ISeriesConverter（Graphs/SeriesConverters.cs）。ここは機種を知らない
/// ・設定は「元に戻す」「開く」で Current が丸ごと差し替わるので、データそのものは覚えておかず、
///   場所（デバイス名・uuid）で覚えて、使うたびに Current から探し直す（CurrentData）
/// ・編集欄（Editor）は機種ごとの ViewModel（WaveformEditors.cs）。値は持たず、毎回 Current を読み書きする
/// ・グラフを描くのは View（GraphView のコードビハインド）。ここは描く線（Lines）だけを用意する
/// </summary>
public partial class GraphViewModel : PaneViewModel
{
    private readonly ISettingsService _settings;
    private readonly IDialogService _dialogs;
    private readonly IReadOnlyList<ISeriesConverter> _converters;

    public GraphViewModel(ISettingsService settings, IDialogService dialogs, IEnumerable<ISeriesConverter> converters)
        : base("Document_Graph", "グラフ", DockLocation.Document, buttonText: "グラフ")
    {
        _settings = settings;
        _dialogs = dialogs;
        _converters = converters.ToList();
        settings.Reloaded += (_, _) => Load();   // 元に戻す・開く → 一覧を作り直す
        Load();
    }

    /// <summary>デバイス / uuid の一覧</summary>
    public ObservableCollection<DeviceEntry> Entries { get; } = new();

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(RemoveWaveformCommand))]
    private DeviceEntry? _selectedEntry;

    [ObservableProperty] private string _yLabel = "電圧 (V)";

    /// <summary>「追加 ▾」に出すデバイス名</summary>
    public IReadOnlyCollection<string> DeviceNames => DeviceTypes.Names;

    /// <summary>選んでいるものの編集欄（機種ごとに型が違う。View は型ごとの DataTemplate で表示する）</summary>
    [ObservableProperty] private object? _editor;

    /// <summary>選んでいるものの表示名（どの機種にもある）</summary>
    public string Name
    {
        get => CurrentData?.Name ?? "";
        set
        {
            if (CurrentData is null || value == Name) return;
            Write<DeviceData>(d => d.Name = value, "Name");
            if (SelectedEntry is { } e) e.Name = value;   // コンボボックスの表示も変える
            OnPropertyChanged();
        }
    }

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

    partial void OnSelectedEntryChanged(DeviceEntry? value)
    {
        RebuildEditor(keepPulse: null);
        OnPropertyChanged(nameof(Name));
        UpdateSeries();
    }

    private void Load()
    {
        var selected = SelectedEntry;
        var pulse = (Editor as WaveformEditorViewModel)?.SelectedPulse?.Index;   // 編集中のパルスも選び直す

        Entries.Clear();
        foreach (var (device, byUuid) in _settings.Current.Waveforms.Waveforms)
            foreach (var (uuid, data) in byUuid)
                Entries.Add(new DeviceEntry(device, uuid, data.Name));

        // 同じ場所があれば選び直す（名前が変わっていても、場所が同じなら同じもの）
        var same = Entries.FirstOrDefault(e => e.IsSameLocation(selected));
        SelectedEntry = same ?? Entries.FirstOrDefault();

        // 選び直した場所が同じでも、中身は変わっているかもしれないので作り直す
        RebuildEditor(same is not null ? pulse : null);
        OnPropertyChanged(nameof(Name));
        UpdateSeries();
    }

    // ===== 追加・削除 =====

    /// <summary>追加：選んだ機種に、新しい uuid の波形を1つ足して、それを選ぶ</summary>
    [RelayCommand]
    private void AddWaveform(string device)
    {
        var uuid = Guid.NewGuid().ToString();

        _settings.Edit($"Waveforms.Add.{uuid}", s =>
        {
            var all = s.Waveforms.Waveforms;
            if (!all.TryGetValue(device, out var byUuid))
                all[device] = byUuid = new Dictionary<string, DeviceData>();   // その機種が初めてなら辞書から作る

            var data = DeviceTypes.Create(device);   // 機種に合った型（WaveformData など）で作る
            data.Name = $"新しい波形 {byUuid.Count + 1}";
            byUuid[uuid] = data;
        });

        // Edit では Reloaded が出ないので、一覧は自分で作り直す
        Load();
        SelectedEntry = Entries.FirstOrDefault(e => e.Device == device && e.Uuid == uuid);
    }

    /// <summary>削除：選んでいる波形を消す（元に戻すで取り消せる）</summary>
    [RelayCommand(CanExecute = nameof(CanRemoveWaveform))]
    private void RemoveWaveform()
    {
        if (SelectedEntry is not { } e) return;
        if (!_dialogs.Confirm($"「{e.Display}」を削除しますか？\n（元に戻す で取り消せます）")) return;

        _settings.Edit($"Waveforms.Remove.{e.Device}.{e.Uuid}", s =>
        {
            var all = s.Waveforms.Waveforms;
            if (!all.TryGetValue(e.Device, out var byUuid)) return;
            byUuid.Remove(e.Uuid);
            if (byUuid.Count == 0) all.Remove(e.Device);   // 空になった機種は消す
        });
        Load();
    }

    private bool CanRemoveWaveform() => SelectedEntry is not null;

    // ===== 編集欄から使う =====

    /// <summary>選んでいる uuid の中身（T 型でなければ null）。編集欄はこれで毎回 Current を読む</summary>
    public T? Data<T>() where T : DeviceData => CurrentData as T;

    /// <summary>
    /// 画面 → 設定。選んでいる uuid の中身が T 型のときだけ書き換える（Undo・「未保存」は ISettingsService.Edit が扱う）
    /// </summary>
    public void Write<T>(Action<T> write, string key) where T : DeviceData
    {
        if (CurrentData is not T data) return;
        _settings.Edit($"Waveforms.{SelectedEntry!.Device}.{SelectedEntry.Uuid}.{key}", _ => write(data));
        UpdateSeries();
    }

    /// <summary>選んでいる機種に合った編集欄を作り直す</summary>
    private void RebuildEditor(int? keepPulse) =>
        Editor = CurrentData switch
        {
            WaveformData => new WaveformEditorViewModel(this, keepPulse),
            DcSourceData => new DcSourceEditorViewModel(this),
            _            => null,   // 編集欄のない機種（表にない機種など）
        };

    private void UpdateSeries()
    {
        Lines = CurrentData is { } data
            ? _converters.FirstOrDefault(c => c.CanConvert(data))?.Convert(data) ?? Array.Empty<Series>()
            : Array.Empty<Series>();
        OnPropertyChanged(nameof(HasNoSeries));
        DataChanged?.Invoke(this, EventArgs.Empty);
    }
}

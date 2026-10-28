using System.Collections.ObjectModel;
using System.Runtime.CompilerServices;
using System.Text.Json;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using DockSample.Services;
using DockSample.Settings;

namespace DockSample.ViewModels;

// グラフペインの下の編集欄（機種ごと）。
//
// どれも値を自分では持たず、プロパティの get / set で毎回 GraphViewModel 経由で Current を読み書きする。
// ・値を写して持たないので、画面と設定がずれない（読み込み中に書き戻さないための仕組みも要らない）
// ・パルス・セグメントは「何番目か」で覚える。追加・削除で番号がずれたら、行を作り直す
// ・「元に戻す」「開く」のあとは、GraphViewModel が編集欄ごと作り直す

/// <summary>パルスを持つ機種の編集欄：左にパルスの一覧、右に選んだパルスのセグメントの表</summary>
public partial class WaveformEditorViewModel : ObservableObject
{
    private readonly GraphViewModel _owner;

    public WaveformEditorViewModel(GraphViewModel owner, int? selectPulse)
    {
        _owner = owner;
        Rebuild(selectPulse ?? 0);
    }

    public ObservableCollection<PulseRowViewModel> Pulses { get; } = new();

    [ObservableProperty]
    [NotifyCanExecuteChangedFor(nameof(AddSegmentCommand))]
    private PulseRowViewModel? _selectedPulse;

    /// <summary>パルスを追加（最後に足して、それを選ぶ）</summary>
    [RelayCommand]
    private void AddPulse()
    {
        var index = _owner.Data<WaveformData>()?.Pulses.Count ?? 0;
        _owner.Write<WaveformData>(d =>
        {
            d.Pulses.Add(new PulseData());
            Renumber(d);
        }, $"AddPulse.{Guid.NewGuid()}");   // 追加は毎回別の Undo にする
        Rebuild(index);
    }

    /// <summary>パルスを複製（行の「コピー」ボタンから。すぐ下に同じ中身のパルスを入れて、それを選ぶ）</summary>
    [RelayCommand]
    private void CopyPulse(PulseRowViewModel row)
    {
        _owner.Write<WaveformData>(d =>
        {
            d.Pulses.Insert(row.Index + 1, Clone(d.Pulses[row.Index]));
            Renumber(d);   // 後ろのパルスの番号が1つずつずれる
        }, $"CopyPulse.{Guid.NewGuid()}");
        Rebuild(row.Index + 1);
    }

    /// <summary>
    /// パルスの名前を並び順に振り直す（Pulse0, Pulse1, ...）。名前は画面からは変えられず、追加・コピー・削除のたびにこれで決まる
    /// </summary>
    private static void Renumber(WaveformData data)
    {
        for (var i = 0; i < data.Pulses.Count; i++)
            data.Pulses[i].Name = $"Pulse{i}";
    }

    /// <summary>
    /// パルスを丸ごと複製する（セグメントや、クラスにない項目 Extra も含めて）。
    /// 同じオブジェクトを2か所に入れると片方の編集がもう片方にも効いてしまうので、JSON に書いて読み直して別物にする
    /// </summary>
    private static PulseData Clone(PulseData pulse) =>
        JsonSerializer.Deserialize<PulseData>(JsonSerializer.Serialize(pulse, JsonSettingsService.JsonOptions),
                                              JsonSettingsService.JsonOptions)!;

    /// <summary>パルスを削除（行の「削除」ボタンから、行そのものが渡される）</summary>
    [RelayCommand]
    private void RemovePulse(PulseRowViewModel row)
    {
        _owner.Write<WaveformData>(d =>
        {
            d.Pulses.RemoveAt(row.Index);
            Renumber(d);   // 後ろのパルスの番号を詰める
        }, $"RemovePulse.{Guid.NewGuid()}");
        Rebuild(Math.Max(0, row.Index - 1));
    }

    /// <summary>選んでいるパルスにセグメントを追加</summary>
    [RelayCommand(CanExecute = nameof(CanAddSegment))]
    private void AddSegment()
    {
        if (SelectedPulse is not { } pulse) return;
        _owner.Write<WaveformData>(
            d => d.Pulses[pulse.Index].Segments.Add(new SegmentData { Name = "segment", TargetVoltage = 5, SlewRate = 1, Duration = TimeUnit.UsToNs(10) }),
            $"AddSegment.{Guid.NewGuid()}");
        Rebuild(pulse.Index);
    }

    private bool CanAddSegment() => SelectedPulse is not null;

    /// <summary>セグメントを削除（行の「削除」ボタンから、行そのものが渡される）</summary>
    [RelayCommand]
    private void RemoveSegment(SegmentRowViewModel row)
    {
        _owner.Write<WaveformData>(d => d.Pulses[row.PulseIndex].Segments.RemoveAt(row.Index),
                                   $"RemoveSegment.{Guid.NewGuid()}");
        Rebuild(row.PulseIndex);
    }

    /// <summary>今の Current から行を作り直し、select 番目のパルスを選ぶ（番号がずれる追加・削除のあとに呼ぶ）</summary>
    private void Rebuild(int select)
    {
        Pulses.Clear();
        var data = _owner.Data<WaveformData>();
        for (var i = 0; i < (data?.Pulses.Count ?? 0); i++)
            Pulses.Add(new PulseRowViewModel(_owner, i));
        SelectedPulse = Pulses.ElementAtOrDefault(Math.Min(select, Pulses.Count - 1));
    }
}

/// <summary>パルス1つ（一覧の1行）。名前と、セグメントの行を持つ</summary>
public class PulseRowViewModel : ObservableObject
{
    private readonly GraphViewModel _owner;

    public PulseRowViewModel(GraphViewModel owner, int index)
    {
        _owner = owner;
        Index = index;
        for (var i = 0; i < (Pulse?.Segments.Count ?? 0); i++)
            Segments.Add(new SegmentRowViewModel(owner, index, i));
    }

    /// <summary>何番目のパルスか</summary>
    public int Index { get; }

    public ObservableCollection<SegmentRowViewModel> Segments { get; } = new();

    private PulseData? Pulse => _owner.Data<WaveformData>()?.Pulses.ElementAtOrDefault(Index);

    /// <summary>名前（Pulse0, Pulse1, ... と並び順で自動で付く。画面からは変えられない）</summary>
    public string Name => Pulse?.Name ?? "";
}

/// <summary>セグメント1つ（表の1行）</summary>
public class SegmentRowViewModel : ObservableObject
{
    private readonly GraphViewModel _owner;

    public SegmentRowViewModel(GraphViewModel owner, int pulseIndex, int index)
    {
        _owner = owner;
        PulseIndex = pulseIndex;
        Index = index;
    }

    public int PulseIndex { get; }
    public int Index { get; }

    /// <summary>表の「#」列（1 から）</summary>
    public int Number => Index + 1;

    private SegmentData? Segment =>
        _owner.Data<WaveformData>()?.Pulses.ElementAtOrDefault(PulseIndex)?.Segments.ElementAtOrDefault(Index);

    public string Name
    {
        get => Segment?.Name ?? "";
        set => Set(Name, value, s => s.Name = value);
    }

    public double TargetVoltage
    {
        get => Segment?.TargetVoltage ?? 0;
        set => Set(TargetVoltage, value, s => s.TargetVoltage = value);
    }

    public double SlewRate
    {
        get => Segment?.SlewRate ?? 0;
        set => Set(SlewRate, value, s => s.SlewRate = value);
    }

    /// <summary>時間（µs で見せる。ファイルには ns で書く）</summary>
    public double DurationUs
    {
        get => TimeUnit.NsToUs(Segment?.Duration ?? 0);
        set => Set(DurationUs, value, s => s.Duration = TimeUnit.UsToNs(value));
    }

    /// <summary>値が変わっていれば Current に書き込む（同じ行の同じ項目の続けての変更は1回分の Undo にまとまる）</summary>
    private void Set<T>(T current, T value, Action<SegmentData> write, [CallerMemberName] string name = "")
    {
        if (Segment is null || EqualityComparer<T>.Default.Equals(current, value)) return;
        _owner.Write<WaveformData>(d => write(d.Pulses[PulseIndex].Segments[Index]), $"{PulseIndex}.{Index}.{name}");
        OnPropertyChanged(name);
    }
}

/// <summary>電圧源の編集欄</summary>
public class DcSourceEditorViewModel : ObservableObject
{
    private readonly GraphViewModel _owner;

    public DcSourceEditorViewModel(GraphViewModel owner) => _owner = owner;

    private DcSourceData? Data => _owner.Data<DcSourceData>();

    public double Voltage
    {
        get => Data?.Voltage ?? 0;
        set
        {
            if (Data is null || value == Voltage) return;
            _owner.Write<DcSourceData>(d => d.Voltage = value, nameof(Voltage));
            OnPropertyChanged();
        }
    }

    public double LimitCurrent
    {
        get => Data?.LimitCurrent ?? 0;
        set
        {
            if (Data is null || value == LimitCurrent) return;
            _owner.Write<DcSourceData>(d => d.LimitCurrent = value, nameof(LimitCurrent));
            OnPropertyChanged();
        }
    }
}

using System.Collections.ObjectModel;
using System.Runtime.CompilerServices;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using DockSample.Graphs;
using DockSample.Settings;

namespace DockSample.ViewModels;

// グラフペインの下の編集欄（DC）。見た目は Views/WaveformEditors/DcSourceEditorView.xaml。
// 値は自分では持たず、プロパティの get / set で毎回 GraphViewModel 経由で Current を読み書きする。
// 信号は「何番目か」で覚える。追加・削除で番号がずれたら、行を作り直す。

/// <summary>DC の編集欄：信号（Hi / Low / Zero と時間）の表。選んだ信号はグラフで目立たせる</summary>
public partial class DcSourceEditorViewModel : ObservableObject
{
    private readonly GraphViewModel _owner;

    public DcSourceEditorViewModel(GraphViewModel owner, int? selectSignal = null)
    {
        _owner = owner;
        Rebuild(selectSignal);
    }

    /// <summary>レベルの選択肢（表のコンボボックスに出す）</summary>
    public IReadOnlyList<string> Levels { get; } = new[] { DcLevel.Hi, DcLevel.Low, DcLevel.Zero };

    public ObservableCollection<DcSignalRowViewModel> Signals { get; } = new();

    /// <summary>表で選んでいる信号（グラフで目立たせる）</summary>
    [ObservableProperty] private DcSignalRowViewModel? _selectedSignal;

    partial void OnSelectedSignalChanged(DcSignalRowViewModel? value) =>
        _owner.SetSelection(value is null ? null : new SignalSelection(value.Index));

    /// <summary>最後に追加（表の上のボタン）</summary>
    [RelayCommand]
    private void AddSignal() => InsertSignal(Signals.Count);

    /// <summary>その行の前に追加</summary>
    [RelayCommand]
    private void InsertSignalBefore(DcSignalRowViewModel row) => InsertSignal(row.Index);

    /// <summary>その行の後ろに追加</summary>
    [RelayCommand]
    private void InsertSignalAfter(DcSignalRowViewModel row) => InsertSignal(row.Index + 1);

    /// <summary>削除（最後の1つは消せない）</summary>
    [RelayCommand(CanExecute = nameof(CanRemoveSignal))]
    private void RemoveSignal(DcSignalRowViewModel row)
    {
        _owner.Write<DcSourceData>(d => d.Signals.RemoveAt(row.Index), $"RemoveSignal.{Guid.NewGuid()}", this);
        Rebuild(Math.Max(0, row.Index - 1));
    }

    private bool CanRemoveSignal(DcSignalRowViewModel? row) => Signals.Count > 1;

    /// <summary>index 番目に新しい信号（初期値は DcSignal.CreateNew）を入れて、それを選ぶ</summary>
    private void InsertSignal(int index)
    {
        _owner.Write<DcSourceData>(
            d => d.Signals.Insert(Math.Clamp(index, 0, d.Signals.Count), DcSignal.CreateNew()),
            $"AddSignal.{Guid.NewGuid()}", this);   // 追加は毎回別の Undo にする
        Rebuild(index);
    }

    /// <summary>表示している値を全部読み直させる（データ側で値が直されたとき用）</summary>
    public void RefreshValues()
    {
        foreach (var row in Signals) row.Refresh();
    }

    /// <summary>今の Current から行を作り直し、select 番目を選ぶ（番号がずれる追加・削除のあとに呼ぶ）</summary>
    private void Rebuild(int? select)
    {
        Signals.Clear();
        var count = _owner.Data<DcSourceData>()?.Signals.Count ?? 0;
        for (var i = 0; i < count; i++)
            Signals.Add(new DcSignalRowViewModel(_owner, this, i));
        SelectedSignal = select is { } s ? Signals.ElementAtOrDefault(Math.Min(s, Signals.Count - 1)) : null;
        RemoveSignalCommand.NotifyCanExecuteChanged();
    }
}

/// <summary>DC の信号1つ（表の1行）</summary>
public class DcSignalRowViewModel : ObservableObject
{
    private readonly GraphViewModel _owner;

    /// <summary>この行を作った編集欄（作り直されて使われなくなった編集欄の行からは書き込まない）</summary>
    private readonly DcSourceEditorViewModel _editor;

    public DcSignalRowViewModel(GraphViewModel owner, DcSourceEditorViewModel editor, int index)
    {
        _owner = owner;
        _editor = editor;
        Index = index;
    }

    public int Index { get; }

    /// <summary>表の「#」列（1 から）</summary>
    public int Number => Index + 1;

    private DcSignal? Signal => _owner.Data<DcSourceData>()?.Signals.ElementAtOrDefault(Index);

    /// <summary>レベル（"Hi" / "Low" / "Zero"）</summary>
    public string Level
    {
        get => Signal?.Level ?? "";
        set => Set(Level, value, s => s.Level = value);
    }

    /// <summary>時間（µs で見せる。ファイルには ns で書く）</summary>
    public double DurationUs
    {
        get => TimeUnit.NsToUs(Signal?.Duration ?? 0);
        set => Set(DurationUs, value, s => s.Duration = TimeUnit.UsToNs(value));
    }

    /// <summary>全部の値を読み直させる</summary>
    public void Refresh() => OnPropertyChanged(string.Empty);

    /// <summary>値が変わっていれば Current に書き込む（同じ行の同じ項目の続けての変更は1回分の Undo にまとまる）</summary>
    private void Set<T>(T current, T value, Action<DcSignal> write, [CallerMemberName] string name = "")
    {
        if (Signal is null || EqualityComparer<T>.Default.Equals(current, value)) return;
        _owner.Write<DcSourceData>(d => write(d.Signals[Index]), $"Signal.{Index}.{name}", _editor);
        OnPropertyChanged(name);
    }
}

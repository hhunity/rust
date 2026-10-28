using CommunityToolkit.Mvvm.ComponentModel;
using DockSample.Settings;

namespace DockSample.ViewModels;

// グラフペインの下の編集欄（電圧源）。見た目は Views/WaveformEditors/DcSourceEditorView.xaml。
// 値は自分では持たず、プロパティの get / set で毎回 GraphViewModel 経由で Current を読み書きする。

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

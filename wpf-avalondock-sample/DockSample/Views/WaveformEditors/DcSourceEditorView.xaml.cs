using System.Windows.Controls;
using DockSample.ViewModels;

namespace DockSample.Views.WaveformEditors;

/// <summary>DC の編集欄（信号の表）</summary>
public partial class DcSourceEditorView : UserControl
{
    public DcSourceEditorView() => InitializeComponent();

    /// <summary>
    /// 信号の表でセルを移った：その行を選んだことにする（グラフで目立たせる）。
    /// クリックしたセルによっては行の選択（SelectedItem）が変わらないことがあるので、今いるセルの行からも拾う
    /// </summary>
    private void SignalGrid_CurrentCellChanged(object? sender, EventArgs e)
    {
        if (sender is DataGrid { DataContext: DcSourceEditorViewModel editor } grid &&
            grid.CurrentItem is DcSignalRowViewModel row && editor.SelectedSignal != row)
            editor.SelectedSignal = row;
    }
}

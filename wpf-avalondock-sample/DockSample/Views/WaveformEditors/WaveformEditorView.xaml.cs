using System.Windows.Controls;
using DockSample.ViewModels;

namespace DockSample.Views.WaveformEditors;

/// <summary>パルスを持つ機種の編集欄（初期電圧・パルスの一覧・セグメントの表）</summary>
public partial class WaveformEditorView : UserControl
{
    public WaveformEditorView() => InitializeComponent();

    /// <summary>
    /// セグメントの表でセルを移った：その行を選んだことにする（グラフで目立たせる）。
    /// クリックしたセルによっては行の選択（SelectedItem）が変わらないことがあるので、今いるセルの行からも拾う
    /// </summary>
    private void SegmentGrid_CurrentCellChanged(object? sender, EventArgs e)
    {
        if (sender is DataGrid { DataContext: WaveformEditorViewModel editor } grid &&
            grid.CurrentItem is SegmentRowViewModel row && editor.SelectedSegment != row)
            editor.SelectedSegment = row;
    }
}

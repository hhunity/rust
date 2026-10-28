using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using DockSample.ViewModels;

namespace DockSample.Views;

/// <summary>
/// グラフ（ScottPlot）。WpfPlot はバインドで描けないので、ViewModel のデータを見てここで描く。
/// ・表示されたら（Loaded）ViewModel のイベントにつなぎ、外れたら（Unloaded）外す
///   （外し忘れると、ずっと残る ViewModel が消えたはずの View を握ったままになる）
/// </summary>
public partial class GraphView : UserControl
{
    private GraphViewModel? _vm;

    public GraphView()
    {
        InitializeComponent();
        Loaded += (_, _) => Attach(DataContext as GraphViewModel);
        Unloaded += (_, _) => Attach(null);
        DataContextChanged += (_, e) => { if (IsLoaded) Attach(e.NewValue as GraphViewModel); };
    }

    /// <summary>ViewModel のイベントを付け替える</summary>
    private void Attach(GraphViewModel? vm)
    {
        if (_vm is not null)
        {
            _vm.DataChanged -= OnDataChanged;
            _vm.PropertyChanged -= OnVmPropertyChanged;
        }
        _vm = vm;
        if (_vm is not null)
        {
            _vm.DataChanged += OnDataChanged;
            _vm.PropertyChanged += OnVmPropertyChanged;
            Redraw();
        }
    }

    private void OnDataChanged(object? sender, EventArgs e) => Redraw();

    private void OnVmPropertyChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(GraphViewModel.YLabel)) Redraw();
    }

    // ===== 拡大・縮小（表示範囲を変えるだけで、データは変えないので ViewModel は通さない） =====

    /// <summary>1回押したときの倍率</summary>
    private const double ZoomStep = 1.5;

    /// <summary>拡大：今の表示の中心を保ったまま、縦横とも 1.5 倍に拡大</summary>
    private void ZoomIn_Click(object sender, RoutedEventArgs e) => Zoom(ZoomStep);

    /// <summary>縮小：今の表示の中心を保ったまま、縦横とも 1/1.5 に縮小</summary>
    private void ZoomOut_Click(object sender, RoutedEventArgs e) => Zoom(1 / ZoomStep);

    /// <summary>全体表示：データが全部入るように表示範囲を合わせ直す</summary>
    private void ZoomFit_Click(object sender, RoutedEventArgs e)
    {
        Chart.Plot.Axes.AutoScale();
        Chart.Refresh();
    }

    /// <summary>frac &gt; 1 で拡大、&lt; 1 で縮小</summary>
    private void Zoom(double frac)
    {
        Chart.Plot.Axes.Zoom(frac, frac);
        Chart.Refresh();
    }

    private void Redraw()
    {
        if (_vm is null) return;
        var plot = Chart.Plot;
        plot.Clear();
        foreach (var s in _vm.Lines)   // 選んだ uuid の線を全部描く（機種によっては複数本）
        {
            var line = plot.Add.Scatter(s.Xs, s.Ys);
            line.LegendText = s.Name;   // 凡例に出す名前
        }
        if (_vm.Lines.Count > 0) plot.ShowLegend();
        else plot.HideLegend();
        plot.XLabel("時間");
        plot.YLabel(_vm.YLabel);
        plot.Axes.AutoScale();
        Chart.Refresh();   // これを呼ばないと画面に出ない
    }
}

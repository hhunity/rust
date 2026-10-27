using System.ComponentModel;
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

    private void Redraw()
    {
        if (_vm is null) return;
        var plot = Chart.Plot;
        plot.Clear();
        if (_vm.Xs.Length > 0)
            plot.Add.Scatter(_vm.Xs, _vm.Ys);
        plot.XLabel("時間");
        plot.YLabel(_vm.YLabel);
        plot.Axes.AutoScale();
        Chart.Refresh();   // これを呼ばないと画面に出ない
    }
}

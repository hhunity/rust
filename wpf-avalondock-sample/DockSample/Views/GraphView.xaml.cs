using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using DockSample.Graphs;
using DockSample.ViewModels;
using Coordinates = ScottPlot.Coordinates;
using Pixel = ScottPlot.Pixel;

namespace DockSample.Views;

/// <summary>
/// グラフ（ScottPlot）。WpfPlot はバインドで描けないので、ViewModel のデータを見てここで描く。
/// ・表示されたら（Loaded）ViewModel のイベントにつなぎ、外れたら（Unloaded）外す
///   （外し忘れると、ずっと残る ViewModel が消えたはずの View を握ったままになる）
/// ・動かせる点（ViewModel の Handles）を丸で描き、マウスでつまんで動かせる。
///   どの点をどう動かせるか（方向・刻み・範囲）は Handle が持っている（機種ごとの定義が決める）。
///   ドラッグ中は丸だけを動かし、離したときに1回だけ ViewModel に書き込む
/// </summary>
public partial class GraphView : UserControl
{
    private GraphViewModel? _vm;

    /// <summary>動かせる点の丸の位置（ScottPlot に渡した配列。中身を書き換えて Refresh すると丸が動く）</summary>
    private double[] _handleXs = Array.Empty<double>();
    private double[] _handleYs = Array.Empty<double>();

    /// <summary>つまんでいる点（Handles の何番目か）と、今の位置（Snap 済み）</summary>
    private int? _dragging;
    private (double X, double Y) _dragTo;

    /// <summary>次の描き直しで表示範囲を保つ（点を動かしたあとに、全体表示に戻らないように）</summary>
    private bool _keepView;

    /// <summary>この距離（画面のピクセル）以内なら、点をつまんだとみなす</summary>
    private const double GrabRadius = 10;

    public GraphView()
    {
        InitializeComponent();
        Loaded += (_, _) => Attach(DataContext as GraphViewModel);
        Unloaded += (_, _) => Attach(null);
        DataContextChanged += (_, e) => { if (IsLoaded) Attach(e.NewValue as GraphViewModel); };

        // ScottPlot 自身のマウス操作（ドラッグで画面移動など）より先に受け取る。点をつまんだときだけ Handled にして横取りする
        Chart.PreviewMouseLeftButtonDown += Chart_MouseDown;
        Chart.PreviewMouseMove += Chart_MouseMove;
        Chart.PreviewMouseLeftButtonUp += Chart_MouseUp;
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

    // ===== 点をマウスで動かす =====

    /// <summary>マウスの位置（WPF の座標）→ ScottPlot の画面のピクセル（高 DPI の拡大率を掛ける）</summary>
    private Pixel ToPixel(MouseEventArgs e)
    {
        var p = e.GetPosition(Chart);
        return new Pixel(p.X * Chart.DisplayScale, p.Y * Chart.DisplayScale);
    }

    private void Chart_MouseDown(object sender, MouseButtonEventArgs e)
    {
        if (_vm is null || _vm.Handles.Count == 0) return;

        // 一番近い点を探す（画面のピクセルで比べる。グラフの座標だと縦横の縮尺が違うので）
        var mouse = ToPixel(e);
        int? nearest = null;
        var best = GrabRadius * Chart.DisplayScale;
        for (var i = 0; i < _vm.Handles.Count; i++)
        {
            var h = _vm.Handles[i];
            var px = Chart.Plot.GetPixel(new Coordinates(h.X, h.Y));
            var d = Math.Sqrt(Math.Pow(px.X - mouse.X, 2) + Math.Pow(px.Y - mouse.Y, 2));
            if (d <= best) { best = d; nearest = i; }
        }
        if (nearest is null) return;   // 点の上でなければ、ScottPlot の普段の操作（画面移動など）に任せる

        _dragging = nearest;
        _dragTo = (_vm.Handles[nearest.Value].X, _vm.Handles[nearest.Value].Y);
        Chart.CaptureMouse();   // グラフの外に出ても離すまで追いかける
        e.Handled = true;       // ScottPlot に渡さない（画面が動かないように）
    }

    private void Chart_MouseMove(object sender, MouseEventArgs e)
    {
        if (_vm is null || _dragging is not { } i || i >= _vm.Handles.Count) return;

        // マウスの位置をグラフの座標にし、その点の決まり（方向・刻み・範囲）に合わせる
        var c = Chart.Plot.GetCoordinates(ToPixel(e));
        _dragTo = _vm.Handles[i].Snap(c.X, c.Y);

        // 丸だけ動かす（データにはまだ書かない）
        _handleXs[i] = _dragTo.X;
        _handleYs[i] = _dragTo.Y;
        Chart.Refresh();
        e.Handled = true;
    }

    private void Chart_MouseUp(object sender, MouseButtonEventArgs e)
    {
        if (_dragging is not { } i) return;
        _dragging = null;
        Chart.ReleaseMouseCapture();
        e.Handled = true;

        if (_vm is null || i >= _vm.Handles.Count) return;
        var h = _vm.Handles[i];
        if (_dragTo.X == h.X && _dragTo.Y == h.Y) return;   // 動いていない

        _keepView = true;   // 書き込み → 描き直し で全体表示に戻らないように
        _vm.MoveHandle(h, _dragTo.X, _dragTo.Y);
    }

    private void Redraw()
    {
        if (_vm is null) return;
        var plot = Chart.Plot;
        var keepView = _keepView;
        var limits = plot.Axes.GetLimits();
        _keepView = false;
        plot.Clear();
        foreach (var s in _vm.Lines)   // 選んだ uuid の線を全部描く（機種によっては複数本）
        {
            var line = plot.Add.Scatter(s.Xs, s.Ys);
            line.LegendText = s.Name;   // 凡例に出す名前
        }
        if (_vm.Lines.Count > 0) plot.ShowLegend();
        else plot.HideLegend();

        // 動かせる点を丸で描く（線なし。凡例には出さない）
        _handleXs = _vm.Handles.Select(h => h.X).ToArray();
        _handleYs = _vm.Handles.Select(h => h.Y).ToArray();
        if (_handleXs.Length > 0)
        {
            var dots = plot.Add.Scatter(_handleXs, _handleYs);
            dots.LineWidth = 0;
            dots.MarkerSize = 9;
            dots.Color = ScottPlot.Color.FromHex("#E67E22");
        }

        plot.XLabel("時間 (µs)");   // ファイルの Duration は ns だが、グラフは µs（換算は機種ごとの定義が行う）
        plot.YLabel(_vm.YLabel);
        if (keepView) plot.Axes.SetLimits(limits);   // 点を動かしたあとは、今の表示範囲のまま
        else plot.Axes.AutoScale();
        Chart.Refresh();   // これを呼ばないと画面に出ない
    }
}

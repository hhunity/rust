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
/// ・動かせる点（ViewModel の Handles）を青い丸で描き、マウスでつまんで動かせる（動かせない点は灰色の四角）。
///   どの点をどう動かせるか（方向・刻み・範囲）は Handle が持っている（機種ごとの定義が決める）。
///   ドラッグ中は丸だけを動かし、離したときに1回だけ ViewModel に書き込む
/// </summary>
public partial class GraphView : UserControl
{
    private GraphViewModel? _vm;

    /// <summary>動かせる点の丸の位置（ScottPlot に渡した配列。中身を書き換えて Refresh すると丸が動く）</summary>
    private double[] _handleXs = Array.Empty<double>();
    private double[] _handleYs = Array.Empty<double>();

    /// <summary>Handles の i 番目が、上の配列の何番目か（動かせない点は -1）</summary>
    private int[] _dotIndex = Array.Empty<int>();

    /// <summary>色：動かせる点（青） / 動かせない点（灰） / 軸への補助線 / 時間 0・電圧 0 の線 / 選んでいるセグメント（オレンジ）</summary>
    private static readonly ScottPlot.Color MovableColor = ScottPlot.Color.FromHex("#2E86DE");
    private static readonly ScottPlot.Color FixedColor = ScottPlot.Color.FromHex("#888888");
    private static readonly ScottPlot.Color GuideColor = ScottPlot.Color.FromHex("#BBBBBB");
    private static readonly ScottPlot.Color ZeroLineColor = ScottPlot.Color.FromHex("#555555");
    private static readonly ScottPlot.Color HighlightColor = ScottPlot.Color.FromHex("#E67E22");

    /// <summary>
    /// 波形の線の色（機種によって線が複数あるときは、この順に使う）。
    /// 指定しないと ScottPlot が「足した順」に色を割り当てるので、補助線の数などで色が変わってしまう
    /// </summary>
    private static readonly ScottPlot.Color[] LineColors =
    {
        ScottPlot.Color.FromHex("#34495E"),   // 濃い紺
        ScottPlot.Color.FromHex("#27AE60"),   // 緑
        ScottPlot.Color.FromHex("#8E44AD"),   // 紫
        ScottPlot.Color.FromHex("#C0392B"),   // 赤
    };

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

        // Ctrl+Z / Ctrl+Y をこのペインのどこで押しても、設定全体の元に戻す / やり直しにする
        PreviewKeyDown += OnPreviewKeyDown;
    }

    /// <summary>
    /// Ctrl+Z（元に戻す）・Ctrl+Y / Ctrl+Shift+Z（やり直し）。
    /// 入力欄（TextBox）は Ctrl+Z を自分の「文字の元に戻す」に使ってしまい、データ側の Undo に届かない。
    /// しかも戻した文字がまた書き込まれて新しい変更になる。なので、入力欄より先（Preview）で受け取って横取りする
    /// </summary>
    private void OnPreviewKeyDown(object sender, KeyEventArgs e)
    {
        if (_vm is null || !Keyboard.Modifiers.HasFlag(ModifierKeys.Control)) return;

        var redo = e.Key == Key.Y || (e.Key == Key.Z && Keyboard.Modifiers.HasFlag(ModifierKeys.Shift));
        var command = redo ? _vm.RedoCommand : e.Key == Key.Z ? _vm.UndoCommand : null;
        if (command is null) return;

        if (command.CanExecute(null)) command.Execute(null);
        e.Handled = true;
    }

    /// <summary>ViewModel のイベントを付け替える</summary>
    private void Attach(GraphViewModel? vm)
    {
        if (_vm is not null)
        {
            _vm.DataChanged -= OnDataChanged;
            _vm.HighlightChanged -= OnHighlightChanged;
            _vm.PropertyChanged -= OnVmPropertyChanged;
        }
        _vm = vm;
        if (_vm is not null)
        {
            _vm.DataChanged += OnDataChanged;
            _vm.HighlightChanged += OnHighlightChanged;
            _vm.PropertyChanged += OnVmPropertyChanged;
            Redraw();
        }
    }

    private void OnDataChanged(object? sender, EventArgs e) => Redraw();

    /// <summary>選んでいるセグメントが変わった：表示範囲はそのままで描き直す</summary>
    private void OnHighlightChanged(object? sender, EventArgs e)
    {
        _keepView = true;
        Redraw();
    }

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
            if (h.IsFixed) continue;   // 動かせない点はつまめない
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
        var dot = _dotIndex[i];
        _handleXs[dot] = _dragTo.X;
        _handleYs[dot] = _dragTo.Y;
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

        // 時間 0（縦）と電圧 0（横）の線
        var zeroX = plot.Add.VerticalLine(0);
        zeroX.Color = ZeroLineColor;
        zeroX.LineWidth = 1;
        var zeroY = plot.Add.HorizontalLine(0);
        zeroY.Color = ZeroLineColor;
        zeroY.LineWidth = 1;

        // 点から軸（時間 0 の縦軸・電圧 0 の横軸）への補助線（点線）。一番下に描くので先に足す
        foreach (var h in _vm.Handles)
        {
            if (h.Y != 0) AddGuide(plot, h.X, h.Y, h.X, 0);   // 下（横軸）へ
            if (h.X != 0) AddGuide(plot, h.X, h.Y, 0, h.Y);   // 左（縦軸）へ
        }

        // 波形：点同士を線でつなぐ（点の印は付けない。点は下の丸で描く）
        for (var n = 0; n < _vm.Lines.Count; n++)   // 選んだ uuid の線を全部描く（機種によっては複数本）
        {
            var s = _vm.Lines[n];
            var line = plot.Add.Scatter(s.Xs, s.Ys);
            line.Color = LineColors[n % LineColors.Length];
            line.LegendText = s.Name;   // 凡例に出す名前
            line.LineWidth = 2;
            line.MarkerSize = 0;
        }
        if (_vm.Lines.Count > 0) plot.ShowLegend();
        else plot.HideLegend();

        // 下の表で選んでいるセグメント：太いオレンジの線と大きめの丸で重ねる（その上に普段の点が乗るので、輪のように見える）
        foreach (var s in _vm.Highlight)
        {
            var hl = plot.Add.Scatter(s.Xs, s.Ys);
            hl.Color = HighlightColor;
            hl.LineWidth = 5;
            hl.MarkerSize = 15;
        }

        // 点（線なし。凡例には出さない）：動かせない点は灰色の四角、動かせる点は青い丸
        // ※ ScottPlot の散布図は1つにつき1色・1形なので、色・形ごとに1つずつ作る
        var fixedPoints = _vm.Handles.Where(h => h.IsFixed).ToArray();
        if (fixedPoints.Length > 0)
            AddDots(plot, fixedPoints.Select(h => h.X).ToArray(), fixedPoints.Select(h => h.Y).ToArray(),
                    FixedColor, ScottPlot.MarkerShape.FilledSquare);

        var movable = _vm.Handles.Where(h => !h.IsFixed).ToArray();
        _handleXs = movable.Select(h => h.X).ToArray();
        _handleYs = movable.Select(h => h.Y).ToArray();
        _dotIndex = new int[_vm.Handles.Count];
        for (int i = 0, dot = 0; i < _vm.Handles.Count; i++)
            _dotIndex[i] = _vm.Handles[i].IsFixed ? -1 : dot++;
        if (_handleXs.Length > 0)
            AddDots(plot, _handleXs, _handleYs, MovableColor, ScottPlot.MarkerShape.FilledCircle);

        plot.XLabel("時間 (µs)");   // ファイルの Duration は ns だが、グラフは µs（換算は機種ごとの定義が行う）
        plot.YLabel(_vm.YLabel);
        if (keepView) plot.Axes.SetLimits(limits);   // 点を動かしたあとは、今の表示範囲のまま
        else plot.Axes.AutoScale();
        Chart.Refresh();   // これを呼ばないと画面に出ない
    }

    /// <summary>
    /// 点だけの散布図（線なし・凡例なし）を足す。
    /// 渡した配列はそのまま使われるので、あとで中身を書き換えて Refresh すると点が動く（ドラッグ中の表示に使う）
    /// </summary>
    private static void AddDots(ScottPlot.Plot plot, double[] xs, double[] ys,
                                ScottPlot.Color color, ScottPlot.MarkerShape shape, float size = 9)
    {
        var dots = plot.Add.Scatter(xs, ys);
        dots.LineWidth = 0;
        dots.MarkerSize = size;
        dots.MarkerShape = shape;
        dots.Color = color;
    }

    /// <summary>補助線（細い灰色の点線）を1本足す</summary>
    private static void AddGuide(ScottPlot.Plot plot, double x1, double y1, double x2, double y2)
    {
        var guide = plot.Add.Line(x1, y1, x2, y2);
        guide.Color = GuideColor;
        guide.LineWidth = 1;
        guide.LinePattern = ScottPlot.LinePattern.Dotted;
    }
}

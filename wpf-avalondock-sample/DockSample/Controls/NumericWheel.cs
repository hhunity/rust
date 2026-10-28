using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;

namespace DockSample.Controls;

/// <summary>
/// TextBox に付けると、フォーカスがあるときにマウスホイールで数値を増減できるようにする（添付プロパティ）。
/// NumericUpDown と同じ操作：ホイールで LargeIncrement ずつ、Ctrl を押しながらで Increment ずつ（微調整）。
///
/// 使い方（DataGrid の数値の列。セルを編集中の TextBox に付く）：
///   &lt;DataGridTextColumn.EditingElementStyle&gt;
///     &lt;Style TargetType="TextBox"&gt;
///       &lt;Setter Property="controls:NumericWheel.Increment" Value="0.1" /&gt;
///     &lt;/Style&gt;
///   &lt;/DataGridTextColumn.EditingElementStyle&gt;
///
/// ・Increment を付けた TextBox だけが対象（0 以下なら何もしない）
/// ・回すたびに値をすぐ反映する（バインド先に書き込む）。同じ項目の続けての変更なので、Undo は1回分にまとまる
/// </summary>
public static class NumericWheel
{
    /// <summary>Ctrl＋ホイールの増減幅（これを付けると有効になる）</summary>
    public static readonly DependencyProperty IncrementProperty = DependencyProperty.RegisterAttached(
        "Increment", typeof(double), typeof(NumericWheel), new PropertyMetadata(0.0, OnIncrementChanged));

    public static double GetIncrement(DependencyObject d) => (double)d.GetValue(IncrementProperty);
    public static void SetIncrement(DependencyObject d, double value) => d.SetValue(IncrementProperty, value);

    /// <summary>ホイールの増減幅。未指定（NaN）なら Increment の10倍</summary>
    public static readonly DependencyProperty LargeIncrementProperty = DependencyProperty.RegisterAttached(
        "LargeIncrement", typeof(double), typeof(NumericWheel), new PropertyMetadata(double.NaN));

    public static double GetLargeIncrement(DependencyObject d) => (double)d.GetValue(LargeIncrementProperty);
    public static void SetLargeIncrement(DependencyObject d, double value) => d.SetValue(LargeIncrementProperty, value);

    /// <summary>これより小さくしない（時間やスルーレートを 0 未満にしないなど）</summary>
    public static readonly DependencyProperty MinimumProperty = DependencyProperty.RegisterAttached(
        "Minimum", typeof(double), typeof(NumericWheel), new PropertyMetadata(double.MinValue));

    public static double GetMinimum(DependencyObject d) => (double)d.GetValue(MinimumProperty);
    public static void SetMinimum(DependencyObject d, double value) => d.SetValue(MinimumProperty, value);

    /// <summary>表示の小数の桁数（回したあとの文字。表の StringFormat=F2 とそろえる）</summary>
    public static readonly DependencyProperty DecimalPlacesProperty = DependencyProperty.RegisterAttached(
        "DecimalPlaces", typeof(int), typeof(NumericWheel), new PropertyMetadata(2));

    public static int GetDecimalPlaces(DependencyObject d) => (int)d.GetValue(DecimalPlacesProperty);
    public static void SetDecimalPlaces(DependencyObject d, int value) => d.SetValue(DecimalPlacesProperty, value);

    private static void OnIncrementChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        if (d is not TextBox box) return;
        box.PreviewMouseWheel -= OnMouseWheel;
        if ((double)e.NewValue > 0) box.PreviewMouseWheel += OnMouseWheel;
    }

    private static void OnMouseWheel(object sender, MouseWheelEventArgs e)
    {
        var box = (TextBox)sender;
        if (!box.IsKeyboardFocusWithin) return;   // フォーカスがないときは普通のスクロールに任せる
        if (!double.TryParse(box.Text, NumberStyles.Float, CultureInfo.CurrentCulture, out var value)) return;

        var small = GetIncrement(box);
        var large = GetLargeIncrement(box);
        var step = Keyboard.Modifiers.HasFlag(ModifierKeys.Control) ? small : double.IsNaN(large) ? small * 10 : large;

        // 刻みに合わせて丸める（0.1 を足し続けたときの 0.30000000000000004 のような誤差を残さない）
        var next = value + Math.Sign(e.Delta) * step;
        next = Math.Max(GetMinimum(box), Math.Round(Math.Round(next / small) * small, 10));

        var digits = GetDecimalPlaces(box);
        box.Text = next.ToString("F" + digits, CultureInfo.CurrentCulture);
        box.SelectAll();
        box.GetBindingExpression(TextBox.TextProperty)?.UpdateSource();   // すぐ反映（グラフも描き直される）
        e.Handled = true;   // 表がスクロールしないように
    }
}

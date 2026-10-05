using System.Globalization;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;

namespace DockSample.Controls;

/// <summary>
/// 数値入力の部品（テキストボックス＋上下ボタン）。
/// ・上下ボタン / ↑↓キー / マウスホイールで Increment ずつ増減（PageUp/PageDown は10倍）
/// ・Minimum～Maximum の範囲外にはならない（ボタンも端で無効になる）
/// ・直接入力した値は Enter かフォーカスが外れたときに確定。数値でなければ元に戻す
/// ・DecimalPlaces の桁で丸める（0.1 を足し続けたときの 0.30000000000000004 のような誤差を防ぐ）
/// </summary>
public partial class NumericUpDown : UserControl
{
    public NumericUpDown()
    {
        InitializeComponent();

        PART_Text.PreviewKeyDown += OnTextPreviewKeyDown;
        PART_Text.LostKeyboardFocus += (_, _) => CommitText();
        PART_Text.PreviewMouseWheel += OnTextMouseWheel;

        UpdateText();
        UpdateButtons();
    }

    // ===== 公開プロパティ（XAML から指定・バインドできる） =====

    /// <summary>現在の値（既定で双方向バインド）</summary>
    public double Value
    {
        get => (double)GetValue(ValueProperty);
        set => SetValue(ValueProperty, value);
    }
    public static readonly DependencyProperty ValueProperty = DependencyProperty.Register(
        nameof(Value), typeof(double), typeof(NumericUpDown),
        new FrameworkPropertyMetadata(0.0,
            FrameworkPropertyMetadataOptions.BindsTwoWayByDefault,
            OnValueChanged, CoerceValue));

    /// <summary>最小値</summary>
    public double Minimum
    {
        get => (double)GetValue(MinimumProperty);
        set => SetValue(MinimumProperty, value);
    }
    public static readonly DependencyProperty MinimumProperty = DependencyProperty.Register(
        nameof(Minimum), typeof(double), typeof(NumericUpDown),
        new PropertyMetadata(double.MinValue, OnRangeChanged));

    /// <summary>最大値</summary>
    public double Maximum
    {
        get => (double)GetValue(MaximumProperty);
        set => SetValue(MaximumProperty, value);
    }
    public static readonly DependencyProperty MaximumProperty = DependencyProperty.Register(
        nameof(Maximum), typeof(double), typeof(NumericUpDown),
        new PropertyMetadata(double.MaxValue, OnRangeChanged));

    /// <summary>1回の増減幅</summary>
    public double Increment
    {
        get => (double)GetValue(IncrementProperty);
        set => SetValue(IncrementProperty, value);
    }
    public static readonly DependencyProperty IncrementProperty = DependencyProperty.Register(
        nameof(Increment), typeof(double), typeof(NumericUpDown),
        new PropertyMetadata(1.0));

    /// <summary>小数点以下の桁数（表示と丸め）。0 なら整数</summary>
    public int DecimalPlaces
    {
        get => (int)GetValue(DecimalPlacesProperty);
        set => SetValue(DecimalPlacesProperty, value);
    }
    public static readonly DependencyProperty DecimalPlacesProperty = DependencyProperty.Register(
        nameof(DecimalPlaces), typeof(int), typeof(NumericUpDown),
        new PropertyMetadata(2, OnRangeChanged),
        v => v is int n && n is >= 0 and <= 15);

    // ===== 値の補正・変更通知 =====

    /// <summary>Value に何が入っても、丸めて範囲内に収める</summary>
    private static object CoerceValue(DependencyObject d, object baseValue)
    {
        var c = (NumericUpDown)d;
        var v = (double)baseValue;
        if (double.IsNaN(v) || double.IsInfinity(v)) return c.Value;
        return c.Normalize(v);
    }

    private static void OnValueChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var c = (NumericUpDown)d;
        c.UpdateText();
        c.UpdateButtons();
    }

    private static void OnRangeChanged(DependencyObject d, DependencyPropertyChangedEventArgs e)
    {
        var c = (NumericUpDown)d;
        c.CoerceValue(ValueProperty);   // 範囲や桁数が変わったら今の値も収め直す
        c.UpdateText();
        c.UpdateButtons();
    }

    private double Normalize(double v)
    {
        v = Math.Round(v, DecimalPlaces, MidpointRounding.AwayFromZero);
        var min = Math.Min(Minimum, Maximum);
        var max = Math.Max(Minimum, Maximum);
        return Math.Clamp(v, min, max);
    }

    // ===== 操作 =====

    private void OnUpClick(object sender, RoutedEventArgs e) => Step(+1);
    private void OnDownClick(object sender, RoutedEventArgs e) => Step(-1);

    /// <summary>Increment × count だけ増減する</summary>
    private void Step(int count)
    {
        CommitText();   // 入力途中の値があれば、それを基準にする
        Value = Normalize(Value + Increment * count);
    }

    private void OnTextPreviewKeyDown(object sender, KeyEventArgs e)
    {
        switch (e.Key)
        {
            case Key.Up:       Step(+1);  e.Handled = true; break;
            case Key.Down:     Step(-1);  e.Handled = true; break;
            case Key.PageUp:   Step(+10); e.Handled = true; break;
            case Key.PageDown: Step(-10); e.Handled = true; break;
            case Key.Enter:    CommitText(); PART_Text.SelectAll(); e.Handled = true; break;
            case Key.Escape:   UpdateText(); PART_Text.SelectAll(); e.Handled = true; break;
        }
    }

    // ホイールはフォーカスがあるときだけ（スクロール中に誤って値が変わらないように）
    private void OnTextMouseWheel(object sender, MouseWheelEventArgs e)
    {
        if (!PART_Text.IsKeyboardFocusWithin) return;
        Step(e.Delta > 0 ? +1 : -1);
        e.Handled = true;
    }

    /// <summary>テキストボックスの文字を数値として確定する。数値でなければ元の表示に戻す</summary>
    private void CommitText()
    {
        if (double.TryParse(PART_Text.Text, NumberStyles.Float, CultureInfo.CurrentCulture, out var v))
            Value = Normalize(v);
        UpdateText();   // 値が変わらなかった場合（範囲外で丸められた等）も表示を整える
    }

    // ===== 表示の更新 =====

    private void UpdateText() =>
        PART_Text.Text = Value.ToString("F" + DecimalPlaces, CultureInfo.CurrentCulture);

    private void UpdateButtons()
    {
        PART_Up.IsEnabled = Value < Math.Max(Minimum, Maximum);
        PART_Down.IsEnabled = Value > Math.Min(Minimum, Maximum);
    }
}

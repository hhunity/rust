using System.Globalization;
using System.Windows;
using System.Windows.Data;

namespace DockSample.Converters;

/// <summary>
/// null → Visible / null 以外 → Collapsed。Invert="True" で逆（null 以外のときだけ表示）。
/// 「何も選ばれていないときは案内を出す」などに使う。
/// </summary>
public class NullToVisibilityConverter : IValueConverter
{
    public bool Invert { get; set; }

    public object Convert(object? value, Type targetType, object? parameter, CultureInfo culture)
    {
        var isNull = value is null;
        return isNull != Invert ? Visibility.Visible : Visibility.Collapsed;
    }

    public object ConvertBack(object? value, Type targetType, object? parameter, CultureInfo culture) =>
        throw new NotSupportedException();
}

using System.Globalization;
using System.Windows;
using System.Windows.Data;

namespace DockSample.Converters;

/// <summary>
/// true → Visible / false → Hidden。
/// AvalonDock の LayoutAnchorableItem は Collapsed ではなく Hidden で「隠す」動作になるため、
/// 標準の BooleanToVisibilityConverter（Collapsed を返す）は使わない。
/// </summary>
public class BoolToVisibilityConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, CultureInfo culture) =>
        value is true ? Visibility.Visible : Visibility.Hidden;

    public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) =>
        value is Visibility.Visible;
}

using System.Windows;
using System.Windows.Controls;
using DockSample.ViewModels;

namespace DockSample.Docking;

public class PanesStyleSelector : StyleSelector
{
    public Style? ToolStyle { get; set; }
    public Style? DocumentStyle { get; set; }

    public override Style? SelectStyle(object item, DependencyObject container) => item switch
    {
        ToolViewModel     => ToolStyle,
        DocumentViewModel => DocumentStyle,
        _                 => base.SelectStyle(item, container),
    };
}

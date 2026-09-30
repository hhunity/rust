using AvalonDock.Layout;
using DockSample.ViewModels;

namespace DockSample.Docking;

public class LayoutInitializer : ILayoutUpdateStrategy
{
    public bool BeforeInsertAnchorable(LayoutRoot layout, LayoutAnchorable anchorableToShow,
                                       ILayoutContainer destinationContainer)
    {
        // レイアウト復元時など、行き先が決まっている場合は任せる
        if (destinationContainer != null) return false;
        if (anchorableToShow.Content is not ToolViewModel tool) return false;

        var (paneName, strategy) = tool.PreferredLocation switch
        {
            ToolLocation.Left  => ("LeftPane",   AnchorableShowStrategy.Left),
            ToolLocation.Right => ("RightPane",  AnchorableShowStrategy.Right),
            _                  => ("BottomPane", AnchorableShowStrategy.Bottom),
        };

        var pane = layout.Descendents()
                         .OfType<LayoutAnchorablePane>()
                         .FirstOrDefault(p => p.Name == paneName);

        if (pane != null)
        {
            pane.Children.Add(anchorableToShow);
        }
        else
        {
            // 空になったペインは AvalonDock に自動削除されることがあるので、その場合は端に新規作成
            anchorableToShow.AddToLayout(layout.Manager, strategy);
            if (anchorableToShow.Parent is LayoutAnchorablePane created)
                created.Name = paneName;
        }
        return true;
    }

    public void AfterInsertAnchorable(LayoutRoot layout, LayoutAnchorable anchorableShown) { }

    public bool BeforeInsertDocument(LayoutRoot layout, LayoutDocument anchorableToShow,
                                     ILayoutContainer destinationContainer) => false;

    public void AfterInsertDocument(LayoutRoot layout, LayoutDocument anchorableShown) { }
}
